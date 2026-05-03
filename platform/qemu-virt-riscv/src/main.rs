#![no_std]
#![no_main]

pub mod plic;
pub mod timer;

use kernel::driver::manager::{AnyDriver, DriverManager};
use kernel::driver::net::DynNetDevice;
use kernel::log::{CpuIdProvider, PlatformConsole, set_console, set_cpu_id_provider};
use kernel::platform::{Platform, PlatformConfig};
use ns16550a::{Ns16550a, Ns16550aFactory};
use virtio_blk::VirtioBlkFactory;

pub struct QemuVirtRiscv;

const UART0_BASE: usize = 0x1000_0000;
const UART0_IRQ: u32 = 10;
const VIRTIO_BLK0_IRQ: u32 = 1;
const DEFAULT_DTB_ADDR: usize = 0x8006_8000;

static UART_DRIVER: Ns16550a = Ns16550a::new(UART0_BASE, UART0_IRQ);

static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

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
        self.hartid
            .store(id, core::sync::atomic::Ordering::Relaxed);
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

fn poll_uart_rx() -> Option<u8> {
    UART_DRIVER.getc()
}

fn drain_platform_uart_rx() -> bool {
    let mut handled = false;
    while let Some(byte) = UART_DRIVER.getc() {
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

    let handled = DriverManager::dispatch_irq(irq);
    if irq == UART0_IRQ {
        let _ = drain_platform_uart_rx();
        let _ = handled || UART_DRIVER.irq_pending();
    }
    plic::complete(irq);
}

impl Platform for QemuVirtRiscv {
    fn early_init() {
        UART_DRIVER.init_hw();
        set_console(&UART_CONSOLE);
        kernel::driver::char::set_rx_poll_fn(poll_uart_rx);
        set_cpu_id_provider(&CPU_ID);

        let _ = DriverManager::register_factory(&Ns16550aFactory);
        let _ = DriverManager::register_factory(&VirtioBlkFactory);

        let dtb = kernel::platform::dtb_addr();
        if dtb == 0 {
            kernel::kwarn!("FDT: no DTB address configured");
        } else {
            kernel::kdebug!("FDT: DTB address = {:#x}", dtb);
        }

        plic::init();
        plic::enable_irq(UART0_IRQ, 1);
        plic::enable_irq(VIRTIO_BLK0_IRQ, 1);
        arch::riscv::irq::enable_external_interrupts();

        unsafe {
            kernel::arch::EXTERNAL_IRQ_HANDLER = Some(external_irq_handler);
        }

        timer::init_timer();
    }

    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn ekernel();
        }

        PlatformConfig {
            cpu_freq_hz: 10_000_000,
            systick_hz: 1000,
            external_irq_count: 64,
            memory_base: 0x8000_0000,
            memory_size: 128 * 1024 * 1024,
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

use core::arch::global_asm;

global_asm!(include_str!("../startup.s"));

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

    kernel::boot::<QemuVirtRiscv, arch::riscv::RiscvArch>(app::root_task);
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
