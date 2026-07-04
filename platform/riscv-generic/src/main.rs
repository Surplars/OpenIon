#![no_std]
#![no_main]

pub mod clic;
pub mod mmu;
pub mod plic;
pub mod timer;

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use kernel::driver::manager::{AnyDriver, DriverManager};
use kernel::driver::net::DynNetDevice;
use kernel::log::{CpuIdProvider, FunctionConsole, set_console, set_cpu_id_provider};
use kernel::platform::{Platform, PlatformConfig, SmpStatus};
#[cfg(feature = "driver_ns16550a")]
use ns16550a::Ns16550aFactory;
use spin::Once;
#[cfg(feature = "driver_virtio_blk")]
use virtio_blk::VirtioBlkFactory;
#[cfg(feature = "driver_virtio_gpu")]
use virtio_gpu::VirtioGpuFactory;
#[cfg(feature = "driver_virtio_rng")]
use virtio_rng::VirtioRngFactory;

pub struct RiscvGeneric;

const FALLBACK_CPU_FREQ_HZ: u32 = 10_000_000;
const FALLBACK_MEMORY_BASE: usize = 0x8000_0000;
const FALLBACK_MEMORY_SIZE: usize = 128 * 1024 * 1024;
const FALLBACK_EXTERNAL_IRQ_COUNT: usize = 64;
const DEFAULT_DTB_ADDR: usize = 0x8006_8000;

static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];
static DTB_INFO: Once<RiscvDtbInfo> = Once::new();
static ONLINE_HART_MASK: AtomicUsize = AtomicUsize::new(0);
static PARKED_HART_MASK: AtomicUsize = AtomicUsize::new(0);
static SMP_START_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static SMP_START_FAILURES: AtomicU32 = AtomicU32::new(0);
static BOOT_HART: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct RiscvDtbInfo {
    cpu_freq_hz: u32,
    memory_base: usize,
    memory_size: usize,
    plic_base: usize,
    plic_irq_sources: usize,
    clic_base: usize,
    clic_irq_sources: usize,
    clint_base: usize,
}

impl RiscvDtbInfo {
    const fn fallback() -> Self {
        Self {
            cpu_freq_hz: FALLBACK_CPU_FREQ_HZ,
            memory_base: FALLBACK_MEMORY_BASE,
            memory_size: FALLBACK_MEMORY_SIZE,
            plic_base: 0,
            plic_irq_sources: FALLBACK_EXTERNAL_IRQ_COUNT.saturating_sub(1),
            clic_base: 0,
            clic_irq_sources: 0,
            clint_base: 0,
        }
    }

    fn external_irq_count(self) -> usize {
        self.plic_irq_sources
            .max(self.clic_irq_sources)
            .saturating_add(1)
    }
}

struct RiscvCpuId {
    hartid: core::sync::atomic::AtomicU32,
}

impl RiscvCpuId {
    const fn new() -> Self {
        Self {
            hartid: core::sync::atomic::AtomicU32::new(0),
        }
    }

    fn set(&self, id: u32) {
        self.hartid.store(id, core::sync::atomic::Ordering::Relaxed);
    }
}

impl CpuIdProvider for RiscvCpuId {
    fn cpu_id(&self) -> u32 {
        if kernel::generated_config::OPENION_SMP {
            <arch::riscv::RiscvArch as kernel::arch::Arch>::current_cpu_id()
        } else {
            self.hartid.load(core::sync::atomic::Ordering::Relaxed)
        }
    }
}

static CPU_ID: RiscvCpuId = RiscvCpuId::new();

fn sbi_putc(ch: u8) {
    arch::riscv::sbi::debug_console_putchar(ch);
}

static UART_CONSOLE: FunctionConsole = FunctionConsole { putc_fn: sbi_putc };

fn external_irq_handler() {
    let irq = plic::claim();
    if irq == 0 {
        return;
    }

    kernel::irq::handle_irq(irq as usize);
    plic::complete(irq);
}

fn external_irq_id_handler(irq: u32) {
    clic::handle_irq(irq);
}

impl Platform for RiscvGeneric {
    fn init_console() {
        set_console(&UART_CONSOLE);
        set_cpu_id_provider(&CPU_ID);
    }

    fn register_driver_factories() {
        #[cfg(feature = "driver_ns16550a")]
        if let Err(_) = DriverManager::register_factory(&Ns16550aFactory) {
            kernel::kwarn!("ns16550a_uart: factory register failed");
        }
        #[cfg(feature = "driver_virtio_blk")]
        if let Err(_) = DriverManager::register_factory(&VirtioBlkFactory) {
            kernel::kwarn!("virtio_blk: factory register failed");
        }
        #[cfg(feature = "driver_virtio_gpu")]
        if let Err(_) = DriverManager::register_factory(&VirtioGpuFactory) {
            kernel::kwarn!("virtio_gpu: factory register failed");
        }
        #[cfg(feature = "driver_virtio_rng")]
        if let Err(_) = DriverManager::register_factory(&VirtioRngFactory) {
            kernel::kwarn!("virtio_rng: factory register failed");
        }
    }

    fn early_init() {
        #[cfg(feature = "hypervisor")]
        arch::riscv::hypervisor::init_kernel_state();

        let dtb = kernel::platform::dtb_addr();
        if dtb == 0 {
            kernel::kwarn!("FDT: no DTB address configured");
        } else {
            kernel::kdebug!("FDT: DTB address = {:#x}", dtb);
        }

        start_secondary_harts();
    }

    fn init_irqs() {
        let info = discover_dtb_info();
        if info.clic_base != 0 {
            clic::configure(info.clic_base, info.clic_irq_sources);
            clic::init();
            if clic::is_configured() {
                arch::riscv::trap::set_trap_vector_clic();
                kernel::arch::set_external_irq_id_handler(external_irq_id_handler);
            }
        }

        if !clic::is_configured() {
            plic::configure(info.plic_base, info.plic_irq_sources);
            plic::init();
            if plic::is_configured() {
                kernel::arch::set_external_irq_handler(external_irq_handler);
                arch::riscv::irq::enable_external_interrupts();
            }
        }
    }

    fn init_memory() {
        mmu::init_identity_map();
    }

    fn init_timer() {
        let info = discover_dtb_info();
        timer::configure_clint(info.clint_base);
        timer::init_timer(clic::is_configured());
    }

    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn ekernel();
        }

        let info = discover_dtb_info();

        PlatformConfig {
            cpu_freq_hz: info.cpu_freq_hz,
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: info.external_irq_count(),
            memory_base: info.memory_base,
            memory_size: info.memory_size,
            kernel_end: ekernel as *const () as usize,
        }
    }

    fn smp_status() -> SmpStatus {
        riscv_smp_status()
    }

    fn net_device() -> Option<&'static DynNetDevice> {
        None
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        &PLATFORM_DRIVERS
    }
}

fn start_secondary_harts() {
    if !kernel::generated_config::OPENION_SMP {
        return;
    }

    unsafe extern "C" {
        fn secondary_entry();
    }

    let boot_hart = <arch::riscv::RiscvArch as kernel::arch::Arch>::current_cpu_id() as usize;
    let max_cpus = kernel::generated_config::OPENION_SMP_MAX_CPUS;
    let entry = secondary_entry as *const () as usize;

    for hartid in 0..max_cpus {
        if hartid == boot_hart {
            continue;
        }

        SMP_START_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        let ret = arch::riscv::sbi::hart_start(hartid, entry, 0);
        if ret.error != 0 {
            SMP_START_FAILURES.fetch_add(1, Ordering::AcqRel);
            kernel::kwarn!("SMP: hart{} start failed: {}", hartid, ret.error);
        }
    }
}
fn hart_bit(hartid: usize) -> Option<usize> {
    if hartid < usize::BITS as usize {
        Some(1usize << hartid)
    } else {
        None
    }
}

fn mark_hart_online(hartid: usize, parked: bool) {
    if let Some(bit) = hart_bit(hartid) {
        ONLINE_HART_MASK.fetch_or(bit, Ordering::AcqRel);
        if parked {
            PARKED_HART_MASK.fetch_or(bit, Ordering::AcqRel);
        } else {
            PARKED_HART_MASK.fetch_and(!bit, Ordering::AcqRel);
        }
    }
}
fn riscv_smp_status() -> SmpStatus {
    let enabled = kernel::generated_config::OPENION_SMP;
    let online_mask = ONLINE_HART_MASK.load(Ordering::Acquire);
    let parked_mask = PARKED_HART_MASK.load(Ordering::Acquire);
    let active_mask = online_mask & !parked_mask;
    let start_attempts = SMP_START_ATTEMPTS.load(Ordering::Acquire) as usize;
    let start_failures = SMP_START_FAILURES.load(Ordering::Acquire) as usize;

    SmpStatus {
        enabled,
        possible_cpus: if enabled {
            kernel::generated_config::OPENION_SMP_MAX_CPUS
        } else {
            1
        },
        online_cpus: online_mask.count_ones() as usize,
        active_cpus: active_mask.count_ones() as usize,
        parked_cpus: parked_mask.count_ones() as usize,
        boot_cpu: BOOT_HART.load(Ordering::Acquire),
        current_cpu: <arch::riscv::RiscvArch as kernel::arch::Arch>::current_cpu_id(),
        online_mask,
        active_mask,
        parked_mask,
        start_attempts,
        start_failures,
    }
}
fn discover_dtb_info() -> &'static RiscvDtbInfo {
    DTB_INFO.call_once(|| {
        let dtb = kernel::platform::dtb_addr();
        if dtb == 0 {
            return RiscvDtbInfo::fallback();
        }

        let mut info = RiscvDtbInfo::fallback();
        unsafe {
            kernel::fdt::walk_nodes(dtb, |node| {
                if !node.is_available() {
                    return;
                }

                if node.device_type() == Some("memory") {
                    if let Some(reg) = node.first_reg() {
                        info.memory_base = reg.base;
                        info.memory_size = reg.size;
                    }
                }

                if let Some(freq) = node.timebase_frequency() {
                    info.cpu_freq_hz = freq;
                }

                if node.compatible_matches("riscv,plic0")
                    || node.compatible_matches("sifive,plic-1.0.0")
                {
                    if let Some(reg) = node.first_reg() {
                        info.plic_base = reg.base;
                    }
                    if let Some(ndev) = node.prop_u32("riscv,ndev") {
                        info.plic_irq_sources = ndev as usize;
                    }
                }

                if node.compatible_matches("riscv,clic0")
                    || node.compatible_matches("sifive,clic-1.0.0")
                {
                    if let Some(reg) = node.first_reg() {
                        info.clic_base = reg.base;
                    }
                    if let Some(numints) = node
                        .prop_u32("riscv,numints")
                        .or_else(|| node.prop_u32("riscv,num-interrupts"))
                        .or_else(|| node.prop_u32("riscv,ndev"))
                    {
                        info.clic_irq_sources = numints as usize;
                    }
                }

                if node.compatible_matches("riscv,clint0")
                    || node.compatible_matches("sifive,clint0")
                {
                    if let Some(reg) = node.first_reg() {
                        info.clint_base = reg.base;
                    }
                }
            });
        }
        info
    })
}

use core::arch::global_asm;

#[cfg(target_pointer_width = "64")]
global_asm!(include_str!("../startup.s"));
#[cfg(target_pointer_width = "32")]
global_asm!(include_str!("../startup_rv32.s"));

fn install_direct_trap_vector() {
    unsafe extern "C" {
        fn trap_vector();
    }
    arch::riscv::trap::set_trap_vector(trap_vector as *const () as usize);
}

fn init_boot_hart(hartid: usize) {
    CPU_ID.set(hartid as u32);
    BOOT_HART.store(hartid as u32, Ordering::Release);
    mark_hart_online(hartid, false);
    install_direct_trap_vector();
}

fn init_secondary_hart(hartid: usize) {
    <arch::riscv::RiscvArch as kernel::arch::Arch>::disable_global_irq();
    CPU_ID.set(hartid as u32);
    install_direct_trap_vector();
    mark_hart_online(hartid, true);
}

fn park_current_hart() -> ! {
    loop {
        <arch::riscv::RiscvArch as kernel::arch::Arch>::idle_hint();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_secondary_main(hartid: usize, _opaque: usize) -> ! {
    init_secondary_hart(hartid);
    park_current_hart();
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main(hartid: usize, dtb_pa: usize) -> ! {
    clear_bss();
    init_boot_hart(hartid);

    let dtb_addr = if dtb_pa == 0 {
        DEFAULT_DTB_ADDR
    } else {
        dtb_pa
    };
    kernel::platform::set_dtb_addr(dtb_addr);

    kernel::boot::<RiscvGeneric, arch::riscv::RiscvArch>();
}

fn clear_bss() {
    unsafe extern "C" {
        fn sbss();
        fn ebss();
    }

    unsafe {
        core::ptr::write_bytes(
            sbss as *mut u8,
            0,
            ebss as *const () as usize - sbss as *const () as usize,
        );
    }
}
