#![no_std]
#![no_main]

pub mod plic;
pub mod timer;

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use kernel::driver::manager::{AnyDriver, DriverManager};
use kernel::driver::net::DynNetDevice;
use kernel::log::{CpuIdProvider, FunctionConsole, set_console, set_cpu_id_provider};
use kernel::platform::{Platform, PlatformConfig};
#[cfg(feature = "driver_ns16550a")]
use ns16550a::Ns16550aFactory;

pub struct IonSocVerilator;

const DEFAULT_CPU_FREQ_HZ: u32 = 10_000_000;
const DEFAULT_MEMORY_BASE: usize = 0x4010_0000;
const DEFAULT_MEMORY_SIZE: usize = 15 * 1024 * 1024;
const MAX_STDOUT_PATH: usize = 96;

static CPU_FREQ_HZ: AtomicU32 = AtomicU32::new(DEFAULT_CPU_FREQ_HZ);
static MEMORY_BASE: AtomicUsize = AtomicUsize::new(DEFAULT_MEMORY_BASE);
static MEMORY_SIZE: AtomicUsize = AtomicUsize::new(DEFAULT_MEMORY_SIZE);
static UART0_BASE: AtomicUsize = AtomicUsize::new(0);
static UART0_IRQ: AtomicU32 = AtomicU32::new(0);
static STDOUT_PATH: StdoutPath = StdoutPath::new();

static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

struct RiscvCpuId {
    hartid: AtomicU32,
}

impl RiscvCpuId {
    const fn new() -> Self {
        Self {
            hartid: AtomicU32::new(0),
        }
    }

    fn set(&self, id: u32) {
        self.hartid.store(id, Ordering::Relaxed);
    }
}

impl CpuIdProvider for RiscvCpuId {
    fn cpu_id(&self) -> u32 {
        self.hartid.load(Ordering::Relaxed)
    }
}

static CPU_ID: RiscvCpuId = RiscvCpuId::new();

static UART_CONSOLE: FunctionConsole = FunctionConsole {
    putc_fn: early_uart_putc,
};

fn uart_base() -> usize {
    UART0_BASE.load(Ordering::Relaxed)
}

fn uart_irq() -> u32 {
    UART0_IRQ.load(Ordering::Relaxed)
}

fn early_uart_reg(offset: usize) -> *mut u8 {
    (uart_base() + offset) as *mut u8
}

fn early_uart_putc(ch: u8) {
    if uart_base() == 0 {
        return;
    }

    const THR: usize = 0;
    const LSR: usize = 5;
    const LSR_THRE: u8 = 1 << 5;

    unsafe {
        while early_uart_reg(LSR).read_volatile() & LSR_THRE == 0 {
            core::hint::spin_loop();
        }
        early_uart_reg(THR).write_volatile(ch);
    }
}

fn early_uart_init() {
    if uart_base() == 0 {
        return;
    }

    const IER: usize = 1;
    const FCR: usize = 2;
    const LCR: usize = 3;

    unsafe {
        early_uart_reg(IER).write_volatile(0x00);
        early_uart_reg(LCR).write_volatile(0x80);
        early_uart_reg(0).write_volatile(0x03);
        early_uart_reg(1).write_volatile(0x00);
        early_uart_reg(LCR).write_volatile(0x03);
        early_uart_reg(FCR).write_volatile(0xc7);
        early_uart_reg(IER).write_volatile(0x01);
    }
}

fn early_uart_getc() -> Option<u8> {
    if uart_base() == 0 {
        return None;
    }

    const RBR: usize = 0;
    const LSR: usize = 5;
    const LSR_DR: u8 = 1 << 0;

    unsafe {
        if early_uart_reg(LSR).read_volatile() & LSR_DR != 0 {
            Some(early_uart_reg(RBR).read_volatile())
        } else {
            None
        }
    }
}

fn early_uart_irq_pending() -> bool {
    if uart_base() == 0 {
        return false;
    }

    const IIR: usize = 2;
    const IIR_NO_INT: u8 = 1 << 0;

    unsafe { early_uart_reg(IIR).read_volatile() & IIR_NO_INT == 0 }
}

fn poll_uart_rx() -> Option<u8> {
    early_uart_getc()
}

fn drain_platform_uart_rx() -> bool {
    let mut handled = false;
    while let Some(byte) = early_uart_getc() {
        kernel::driver::char::push_to_rx_buf(byte);
        handled = true;
    }
    handled
}

fn external_irq_handler() {
    let irq = plic::claim();
    if irq == 0 {
        return;
    }

    let handled = kernel::irq::handle_irq(irq as usize);
    if irq == uart_irq() && !handled {
        let _ = drain_platform_uart_rx();
        let _ = early_uart_irq_pending();
    }
    plic::complete(irq);
}

impl Platform for IonSocVerilator {
    fn early_init() {
        discover_from_fdt();
        early_uart_init();
        log_dtb_status();
    }

    fn init_console() {
        setup_console_hooks();
    }

    fn register_driver_factories() {
        setup_driver_factories();
    }

    fn init_irqs() {
        setup_irqs();
    }

    fn init_timer() {
        setup_timer();
    }

    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn ekernel();
        }

        PlatformConfig {
            cpu_freq_hz: CPU_FREQ_HZ.load(Ordering::Relaxed),
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: kernel::generated_config::OPENION_EXTERNAL_IRQ_COUNT,
            memory_base: MEMORY_BASE.load(Ordering::Relaxed),
            memory_size: MEMORY_SIZE.load(Ordering::Relaxed),
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

fn setup_console_hooks() {
    set_console(&UART_CONSOLE);
    kernel::driver::char::set_rx_poll_fn(poll_uart_rx);
    set_cpu_id_provider(&CPU_ID);
}

fn setup_driver_factories() {
    #[cfg(feature = "driver_ns16550a")]
    if let Err(_) = DriverManager::register_factory(&Ns16550aFactory) {
        kernel::kwarn!("ns16550a_uart: factory register failed");
    }
}

fn log_dtb_status() {
    let dtb = kernel::platform::dtb_addr();
    if dtb == 0 {
        kernel::kwarn!("FDT: no DTB address configured");
    } else {
        kernel::kdebug!("FDT: DTB address = {:#x}", dtb);
    }
}

fn setup_irqs() {
    if plic::is_ready() {
        plic::init();
        let irq = uart_irq();
        if irq != 0 {
            plic::enable_irq(irq, 1);
        }
        arch::riscv::irq::enable_external_interrupts();
        kernel::arch::set_external_irq_handler(external_irq_handler);
    } else {
        kernel::kwarn!("PLIC: base address not found in FDT");
    }
}

fn setup_timer() {
    if timer::is_ready() {
        timer::init_timer();
    } else {
        kernel::kwarn!("CLINT: base address not found in FDT");
    }
}

fn discover_from_fdt() {
    let dtb = kernel::platform::dtb_addr();
    if dtb == 0 {
        return;
    }

    unsafe {
        kernel::fdt::walk_nodes(dtb, |node| {
            if let Some(stdout_path) = node.stdout_path() {
                STDOUT_PATH.set(stdout_path);
            }
        });

        kernel::fdt::walk_nodes(dtb, |node| {
            if let Some(freq) = node.timebase_frequency() {
                CPU_FREQ_HZ.store(freq, Ordering::Relaxed);
            }

            let Some(reg) = node.first_reg() else {
                return;
            };

            if is_memory_node(node) {
                MEMORY_BASE.store(reg.base, Ordering::Relaxed);
                MEMORY_SIZE.store(reg.size, Ordering::Relaxed);
            } else if is_uart_node(node) && should_select_uart(node) {
                UART0_BASE.store(reg.base, Ordering::Relaxed);
                UART0_IRQ.store(node.interrupt_or_zero(), Ordering::Relaxed);
            } else if is_plic_node(node) {
                plic::set_base(reg.base);
            } else if is_clint_node(node) {
                timer::set_base(reg.base);
            }
        });

        if uart_base() == 0 && STDOUT_PATH.is_configured() {
            kernel::fdt::walk_nodes(dtb, |node| {
                let Some(reg) = node.first_reg() else {
                    return;
                };
                if is_uart_node(node) && uart_base() == 0 {
                    UART0_BASE.store(reg.base, Ordering::Relaxed);
                    UART0_IRQ.store(node.interrupt_or_zero(), Ordering::Relaxed);
                }
            });
        }
    }
}

struct StdoutPath {
    bytes: [AtomicU8; MAX_STDOUT_PATH],
    len: AtomicUsize,
}

impl StdoutPath {
    const fn new() -> Self {
        Self {
            bytes: [const { core::sync::atomic::AtomicU8::new(0) }; MAX_STDOUT_PATH],
            len: AtomicUsize::new(0),
        }
    }

    fn set(&self, path: &str) {
        let raw = path.as_bytes();
        let len = raw.len().min(MAX_STDOUT_PATH);
        for (i, byte) in raw.iter().copied().take(len).enumerate() {
            self.bytes[i].store(byte, Ordering::Relaxed);
        }
        self.len.store(len, Ordering::Release);
    }

    fn matches_node(&self, node_name: &str) -> bool {
        let len = self.len.load(Ordering::Acquire);
        if len == 0 {
            return false;
        }

        let mut path = [0u8; MAX_STDOUT_PATH];
        for (i, slot) in path.iter_mut().enumerate().take(len) {
            *slot = self.bytes[i].load(Ordering::Relaxed);
        }

        let path = &path[..len];
        let path = strip_stdout_options(path);
        let Some(last) = last_path_component(path) else {
            return false;
        };

        last == node_name.as_bytes()
    }

    fn is_configured(&self) -> bool {
        self.len.load(Ordering::Acquire) != 0
    }
}

fn strip_stdout_options(path: &[u8]) -> &[u8] {
    match path.iter().position(|&b| b == b':') {
        Some(idx) => &path[..idx],
        None => path,
    }
}

fn last_path_component(path: &[u8]) -> Option<&[u8]> {
    let end = path.len();
    let start = path
        .iter()
        .rposition(|&b| b == b'/')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    if start >= end {
        None
    } else {
        Some(&path[start..end])
    }
}

fn should_select_uart(node: kernel::fdt::FdtNode<'_>) -> bool {
    if STDOUT_PATH.is_configured() {
        STDOUT_PATH.matches_node(node.name())
    } else {
        uart_base() == 0
    }
}

fn is_memory_node(node: kernel::fdt::FdtNode<'_>) -> bool {
    node.device_type() == Some("memory") || node.name().starts_with("memory@")
}

fn is_uart_node(node: kernel::fdt::FdtNode<'_>) -> bool {
    node.compatible_matches("ns16550a")
        || node.compatible_matches("ns16550")
        || node.compatible_matches("ns16550a-uart")
        || node.compatible_matches("snps,dw-apb-uart")
}

fn is_plic_node(node: kernel::fdt::FdtNode<'_>) -> bool {
    node.compatible_matches("riscv,plic0")
        || node.compatible_matches("sifive,plic-1.0.0")
        || node.compatible_matches("thead,c900-plic")
}

fn is_clint_node(node: kernel::fdt::FdtNode<'_>) -> bool {
    node.compatible_matches("riscv,clint0")
        || node.compatible_matches("sifive,clint0")
        || node.compatible_matches("sifive,clint-1.0.0")
}

use core::arch::global_asm;

global_asm!(include_str!("../startup.s"));

#[unsafe(no_mangle)]
pub extern "C" fn rust_main(hartid: usize, dtb_pa: usize) -> ! {
    CPU_ID.set(hartid as u32);

    clear_bss();

    kernel::platform::set_dtb_addr(dtb_pa);

    unsafe extern "C" {
        fn trap_vector();
    }
    arch::riscv::trap::set_trap_vector(trap_vector as *const () as usize);

    kernel::boot::<IonSocVerilator, arch::riscv::RiscvArch>();
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
