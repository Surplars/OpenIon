#![no_std]
#![no_main]

pub mod clic;
pub mod mmu;
pub mod plic;
pub mod timer;

#[cfg(feature = "driver_esp32s31_uart")]
use esp32s31_uart::Esp32s31UartFactory;
use kernel::driver::manager::{AnyDriver, DriverManager};
use kernel::driver::net::DynNetDevice;
use kernel::log::{CpuIdProvider, PlatformConsole, set_console, set_cpu_id_provider};
use kernel::platform::{Platform, PlatformConfig};
#[cfg(feature = "driver_ns16550a")]
use ns16550a::Ns16550aFactory;
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
        self.hartid.load(core::sync::atomic::Ordering::Relaxed)
    }
}

static CPU_ID: RiscvCpuId = RiscvCpuId::new();

struct UartConsole;

unsafe impl Sync for UartConsole {}

impl PlatformConsole for UartConsole {
    fn putc(&self, ch: u8) {
        arch::riscv::sbi::debug_console_putchar(ch);
    }
}

static UART_CONSOLE: UartConsole = UartConsole;

fn external_irq_handler() {
    let irq = plic::claim();
    if irq == 0 {
        return;
    }

    let _ = DriverManager::dispatch_irq(irq);
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
        #[cfg(feature = "driver_esp32s31_uart")]
        let _ = DriverManager::register_factory(&Esp32s31UartFactory);
        #[cfg(feature = "driver_ns16550a")]
        let _ = DriverManager::register_factory(&Ns16550aFactory);
        #[cfg(feature = "driver_virtio_blk")]
        let _ = DriverManager::register_factory(&VirtioBlkFactory);
        #[cfg(feature = "driver_virtio_gpu")]
        let _ = DriverManager::register_factory(&VirtioGpuFactory);
        #[cfg(feature = "driver_virtio_rng")]
        let _ = DriverManager::register_factory(&VirtioRngFactory);
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
    }

    fn init_irqs() {
        let info = discover_dtb_info();
        if info.clic_base != 0 {
            clic::configure(info.clic_base, info.clic_irq_sources);
            clic::init();
            if clic::is_configured() {
                arch::riscv::trap::set_trap_vector_clic();
                unsafe {
                    kernel::arch::EXTERNAL_IRQ_ID_HANDLER = Some(external_irq_id_handler);
                }
            }
        }

        if !clic::is_configured() {
            plic::configure(info.plic_base, info.plic_irq_sources);
            plic::init();
            if plic::is_configured() {
                unsafe {
                    kernel::arch::EXTERNAL_IRQ_HANDLER = Some(external_irq_handler);
                }
                arch::riscv::irq::enable_external_interrupts();
            }
        }
    }

    fn init_memory() {
        mmu::init_sv32_identity_map();
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

    fn net_device() -> Option<&'static DynNetDevice> {
        None
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        &PLATFORM_DRIVERS
    }
}

fn discover_dtb_info() -> RiscvDtbInfo {
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

            if node.compatible_matches("riscv,clint0") || node.compatible_matches("sifive,clint0") {
                if let Some(reg) = node.first_reg() {
                    info.clint_base = reg.base;
                }
            }
        });
    }
    info
}

use core::arch::global_asm;

#[cfg(target_pointer_width = "64")]
global_asm!(include_str!("../startup.s"));
#[cfg(target_pointer_width = "32")]
global_asm!(include_str!("../startup_rv32.s"));

#[unsafe(no_mangle)]
pub extern "C" fn rust_main(hartid: usize, dtb_pa: usize) -> ! {
    CPU_ID.set(hartid as u32);

    clear_bss();

    let dtb_addr = if dtb_pa == 0 {
        DEFAULT_DTB_ADDR
    } else {
        dtb_pa
    };
    kernel::platform::set_dtb_addr(dtb_addr);

    unsafe extern "C" {
        fn trap_vector();
    }
    arch::riscv::trap::set_trap_vector(trap_vector as *const () as usize);

    kernel::boot::<RiscvGeneric, arch::riscv::RiscvArch>(app::root_task);
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
