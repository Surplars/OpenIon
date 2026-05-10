#![no_std]
#![no_main]

use core::arch::global_asm;
use kernel::driver::manager::AnyDriver;
use kernel::platform::{Platform, PlatformConfig};
#[cfg(feature = "driver_stm32l4x5_usart")]
use stm32l4x5_usart::Stm32l4x5Usart;

global_asm!(include_str!("../startup.s"));

const CPU_FREQ_HZ: u32 = 4_000_000;
const MEMORY_BASE: usize = 0x2000_0000;
const MEMORY_SIZE: usize = 96 * 1024;

const RCC_BASE: usize = 0x4002_1000;
const RCC_AHB2ENR: usize = RCC_BASE + 0x4c;
const RCC_APB2ENR: usize = RCC_BASE + 0x60;
const RCC_AHB2ENR_GPIOAEN: u32 = 1 << 0;
const RCC_APB2ENR_USART1EN: u32 = 1 << 14;

const GPIOA_BASE: usize = 0x4800_0000;
const GPIO_MODER: usize = 0x00;
const GPIO_OSPEEDR: usize = 0x08;
const GPIO_AFRH: usize = 0x24;

const USART1_BASE: usize = 0x4001_3800;
const USART1_IRQ: u32 = 37;
const USART_BAUD: u32 = 115_200;

#[cfg(feature = "driver_stm32l4x5_usart")]
static USART1: Stm32l4x5Usart =
    Stm32l4x5Usart::new(USART1_BASE, USART1_IRQ, CPU_FREQ_HZ, USART_BAUD);

#[cfg(feature = "driver_stm32l4x5_usart")]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 1] = [&USART1];
#[cfg(not(feature = "driver_stm32l4x5_usart"))]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

static UART_CONSOLE: UartConsole = UartConsole;

#[unsafe(no_mangle)]
pub extern "C" fn platform_init() -> ! {
    kernel::boot::<QemuStm32l475, arch::arm::cortex_m::CortexM>(app::root_task);
}

#[unsafe(no_mangle)]
pub extern "C" fn usart1_handler() {
    kernel::irq::handle_irq(USART1_IRQ as usize);
}

struct UartConsole;

unsafe impl Sync for UartConsole {}

impl kernel::log::PlatformConsole for UartConsole {
    fn putc(&self, ch: u8) {
        #[cfg(feature = "driver_stm32l4x5_usart")]
        USART1.putc(ch);

        #[cfg(not(feature = "driver_stm32l4x5_usart"))]
        let _ = ch;
    }
}

struct QemuStm32l475;

impl Platform for QemuStm32l475 {
    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn _ebss();
        }

        PlatformConfig {
            cpu_freq_hz: CPU_FREQ_HZ,
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: kernel::generated_config::OPENION_EXTERNAL_IRQ_COUNT,
            memory_base: MEMORY_BASE,
            memory_size: MEMORY_SIZE,
            kernel_end: _ebss as *const () as usize,
        }
    }

    fn early_init() {
        arch::arm::cortex_m::irq::disable();

        init_clocks_and_pins();
        #[cfg(feature = "driver_stm32l4x5_usart")]
        USART1.init_hw();
        kernel::log::set_console(&UART_CONSOLE);
        #[cfg(feature = "driver_stm32l4x5_usart")]
        {
            kernel::driver::char::set_rx_poll_fn(poll_usart1_rx);
            arch::arm::cortex_m::nvic::enable_irq(USART1_IRQ as u16);
        }

        let config = Self::config();
        arch::arm::cortex_m::systick::init(config.cpu_freq_hz, config.systick_hz);

        kernel::kdebug!("QEMU STM32L475 early init complete");
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        &PLATFORM_DRIVERS
    }
}

#[cfg(feature = "driver_stm32l4x5_usart")]
fn poll_usart1_rx() -> Option<u8> {
    USART1.getc()
}

fn init_clocks_and_pins() {
    set_bits(RCC_AHB2ENR, RCC_AHB2ENR_GPIOAEN);
    set_bits(RCC_APB2ENR, RCC_APB2ENR_USART1EN);

    // USART1 on STM32L475 commonly uses PA9/PA10 with alternate function 7.
    let moder = read_reg(GPIOA_BASE + GPIO_MODER);
    let moder = (moder & !((0b11 << 18) | (0b11 << 20))) | (0b10 << 18) | (0b10 << 20);
    write_reg(GPIOA_BASE + GPIO_MODER, moder);

    let ospeedr = read_reg(GPIOA_BASE + GPIO_OSPEEDR);
    write_reg(
        GPIOA_BASE + GPIO_OSPEEDR,
        ospeedr | (0b11 << 18) | (0b11 << 20),
    );

    let afrh = read_reg(GPIOA_BASE + GPIO_AFRH);
    let afrh = (afrh & !((0xf << 4) | (0xf << 8))) | (7 << 4) | (7 << 8);
    write_reg(GPIOA_BASE + GPIO_AFRH, afrh);
}

fn set_bits(addr: usize, bits: u32) {
    write_reg(addr, read_reg(addr) | bits);
}

fn read_reg(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

fn write_reg(addr: usize, val: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}
