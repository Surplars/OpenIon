#![no_std]
#![no_main]

pub mod irq;

use core::arch::global_asm;
use kernel::driver::manager::AnyDriver;
use kernel::platform::{Platform, PlatformConfig};

// Chip startup assembly: sets up stack pointer and calls platform_init() to enter Rust
global_asm!(include_str!("../startup.s"));

#[unsafe(no_mangle)]
pub extern "C" fn platform_init() -> ! {
    kernel::boot::<QemuAn521, arch::arm::cortex_m::CortexM>();
}

#[unsafe(no_mangle)]
pub extern "C" fn uart0_rx_handler() {
    kernel::irq::handle_irq(bsp::arm::an521::uart_irq() as usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn eth_handler() {
    kernel::irq::handle_irq(bsp::arm::an521::lan9118_irq() as usize);
}

struct QemuAn521;

impl Platform for QemuAn521 {
    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn _ebss();
        }

        PlatformConfig {
            cpu_freq_hz: bsp::arm::an521::CPU_FREQ_HZ,
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: kernel::generated_config::OPENION_EXTERNAL_IRQ_COUNT,
            memory_base: bsp::arm::an521::MEMORY_BASE,
            memory_size: bsp::arm::an521::MEMORY_SIZE,
            kernel_end: _ebss as *const () as usize,
        }
    }

    fn init_console() {
        bsp::arm::an521::init_console();
    }

    fn early_init() {
        arch::arm::cortex_m::irq::disable();
        kernel::kdebug!("QEMU AN521 early init complete");
    }

    fn init_irqs() {
        bsp::arm::an521::init_irqs();
    }

    fn init_timer() {
        bsp::arm::an521::init_timer();
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        bsp::arm::an521::drivers()
    }

    fn net_device() -> Option<&'static kernel::driver::net::DynNetDevice> {
        bsp::arm::an521::net_device()
    }
}
