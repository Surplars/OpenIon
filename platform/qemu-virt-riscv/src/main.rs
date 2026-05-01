#![no_std]
#![no_main]

pub mod plic;
pub mod timer;

use kernel::platform::{Platform, PlatformConfig};
use kernel::driver::manager::{AnyDriver, DriverManager};
use kernel::driver::net::DynNetDevice;
use kernel::log::{set_console, set_cpu_id_provider, CpuIdProvider, PlatformConsole};
use ns16550a::{Ns16550a, Ns16550aFactory};
use virtio_blk::VirtioBlkFactory;

pub struct QemuVirtRiscv;

const UART0_BASE: usize = 0x1000_0000;
const UART0_IRQ: u32 = 10;
const VIRTIO_BLK0_BASE: usize = 0x10001000;
const VIRTIO_BLK0_IRQ: u32 = 1;

static UART_DRIVER: Ns16550a = Ns16550a::new(UART0_BASE, UART0_IRQ);

/// Platform drivers: on FDT-capable systems, most drivers are auto-probed via FDT.
/// This array is for platform-specific drivers not discoverable via FDT (e.g. MCU).
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

struct RiscvCpuId {
    hartid: core::sync::atomic::AtomicU32,
}
impl RiscvCpuId {
    const fn new() -> Self {
        Self { hartid: core::sync::atomic::AtomicU32::new(0) }
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
        // Use SBI DBCN putchar (RustSBI 0.4.0) to bypass PMP
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a0") ch as usize,
                in("a6") 0x2usize,
                in("a7") 0x4442434Eusize,
            );
        }
    }
}

static UART_CONSOLE: UartConsole = UartConsole;

fn external_irq_handler() {
    let irq = plic::claim();
    if irq != 0 {
        kernel::driver::manager::DriverManager::dispatch_irq(irq);
        plic::complete(irq);
    }
}

impl Platform for QemuVirtRiscv {
    fn early_init() {
        // 初始化 UART 硬件，设置为控制台
        UART_DRIVER.init_hw();
        set_console(&UART_CONSOLE);
        set_cpu_id_provider(&CPU_ID);

        // Register driver factories for FDT auto-probing
        let _ = DriverManager::register_factory(&Ns16550aFactory);
        let _ = DriverManager::register_factory(&VirtioBlkFactory);

        // Debug: scan FDT and print all compatible strings
        let dtb = kernel::platform::dtb_addr();
        kernel::kdebug!("FDT: DTB address = {:#x}", dtb);
        if dtb != 0 {
            unsafe {
                kernel::fdt::parse(dtb, |_name, compat, _reg, _interrupt| {
                    kernel::kdebug!("FDT node: compatible='{}'", compat);
                });
            }
        }

        plic::init();
        plic::enable_irq(UART0_IRQ, 1);
        plic::enable_irq(VIRTIO_BLK0_IRQ, 1);

        // 使能 CPU 外部中断 (SEIE for S-mode, MEIE for M-mode)
        unsafe {
            #[cfg(feature = "s-mode")]
            riscv::register::sie::set_sext();
            #[cfg(feature = "m-mode")]
            riscv::register::mie::set_mext();
        }

        // Set the external IRQ handler for the trap handler
        unsafe {
            kernel::arch::EXTERNAL_IRQ_HANDLER = Some(external_irq_handler);
        }

        // Timer 初始化
        #[cfg(feature = "m-mode")]
        timer::clint::init_timer();

        #[cfg(feature = "s-mode")]
        timer::sbi_timer::init_timer();
    }

    fn config() -> PlatformConfig {
        PlatformConfig {
            cpu_freq_hz: 10_000_000,
            systick_hz: 1000,
            external_irq_count: 64,
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
pub extern "C" fn rust_main(hartid: usize, _dtb_pa: usize) -> ! {
    // Store hartid for CPU ID provider (can't read mhartid from S-mode)
    CPU_ID.set(hartid as u32);

    clear_bss();

    // DTB address: must be set AFTER clear_bss (DTB_ADDR is in BSS)
    kernel::platform::set_dtb_addr(0x8006_8000);

    // Set stvec early so traps go to a known handler instead of 0x0
    unsafe extern "C" {
        fn trap_vector();
    }
    unsafe {
        riscv::register::stvec::write(trap_vector as *const () as usize, riscv::register::stvec::TrapMode::Direct);
    }

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
