#![no_std]
#![no_main]

use core::arch::global_asm;
use kernel::driver::manager::AnyDriver;
use kernel::platform::{Platform, PlatformConfig};

global_asm!(include_str!("../startup.s"));

#[unsafe(no_mangle)]
pub extern "C" fn platform_init() -> ! {
    kernel::boot::<Stm32f103Bluepill, arch::arm::cortex_m::CortexM>();
}

#[unsafe(no_mangle)]
pub extern "C" fn usart1_handler() {
    kernel::irq::handle_irq(bsp::arm::stm32f103::usart1_irq() as usize);
}

struct Stm32f103Bluepill;

impl Platform for Stm32f103Bluepill {
    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn _ebss();
        }

        PlatformConfig {
            cpu_freq_hz: bsp::arm::stm32f103::CPU_FREQ_HZ,
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: kernel::generated_config::OPENION_EXTERNAL_IRQ_COUNT,
            memory_base: bsp::arm::stm32f103::MEMORY_BASE,
            memory_size: bsp::arm::stm32f103::MEMORY_SIZE,
            kernel_end: _ebss as *const () as usize,
        }
    }

    fn early_init() {
        arch::arm::cortex_m::irq::disable();
        bsp::arm::stm32f103::early_clock_init();
        kernel::kdebug!("STM32F103 early init complete");
    }

    fn init_console() {
        bsp::arm::stm32f103::early_clock_init();
        bsp::arm::stm32f103::init_console();
    }

    fn init_irqs() {
        bsp::arm::stm32f103::init_irqs();
    }

    fn init_timer() {
        bsp::arm::stm32f103::init_timer();
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        bsp::arm::stm32f103::drivers()
    }
}
