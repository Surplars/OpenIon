#![no_std]
#![no_main]

pub mod irq;

use cmsdk_uart::CmsdkUart;
use core::arch::global_asm;
use kernel::driver::manager::AnyDriver;
use kernel::platform::{Platform, PlatformConfig};
use lan9118::Lan9118;

// Chip startup assembly: sets up stack pointer and calls platform_init() to enter Rust
global_asm!(include_str!("../startup.s"));

static UART_CONSOLE: UartConsole = UartConsole;

static UART: CmsdkUart = CmsdkUart::new(UART_DATA, UART_IRQ);
static LAN9118_ETH: Lan9118 = Lan9118::new(kernel::driver::GenericDeviceConfig::new(
    LAN9118_BASE,
    LAN9118_IRQ,
));

static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 2] = [&UART, &LAN9118_ETH];

#[unsafe(no_mangle)]

pub extern "C" fn platform_init() -> ! {
    kernel::boot::<QemuAn521, arch::arm::cortex_m::CortexM>(app::root_task);
}

#[unsafe(no_mangle)]

pub extern "C" fn uart0_rx_handler() {
    kernel::irq::handle_irq(UART_IRQ as usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn eth_handler() {
    kernel::irq::handle_irq(LAN9118_IRQ as usize);
}

struct UartConsole;

unsafe impl Sync for UartConsole {}

impl kernel::log::PlatformConsole for UartConsole {
    fn putc(&self, ch: u8) {
        uart_putc(ch);
    }
}

const UART_DATA: usize = kernel::generated_config::OPENION_QEMU_AN521_UART_BASE;
const UART_IRQ: u32 = kernel::generated_config::OPENION_QEMU_AN521_UART_IRQ;
const LAN9118_BASE: usize = kernel::generated_config::OPENION_QEMU_AN521_LAN9118_BASE;
const LAN9118_IRQ: u32 = kernel::generated_config::OPENION_QEMU_AN521_LAN9118_IRQ;

const UART_STATE: usize = 0x40200004;

const UART_STATE_TXBF: u32 = 1 << 0;

const UART_CTRL: usize = 0x40200008;

fn uart_init() {
    // 使能 TX / RX

    write_reg(UART_CTRL, 1 | (1 << 1)); // TXEN | RXEN
}

fn uart_putc(ch: u8) {
    // 等待 TX buffer not full

    while (read_reg(UART_STATE) & UART_STATE_TXBF) != 0 {}

    write_reg(UART_DATA, ch as u32);
}

#[inline(always)]

fn read_reg(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline(always)]

fn write_reg(addr: usize, val: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}

struct QemuAn521;

impl Platform for QemuAn521 {
    fn config() -> PlatformConfig {
        unsafe extern "C" {
            fn _ebss();
        }

        PlatformConfig {
            cpu_freq_hz: kernel::generated_config::OPENION_QEMU_AN521_CPU_HZ,
            systick_hz: kernel::generated_config::OPENION_SYSTICK_HZ,
            external_irq_count: kernel::generated_config::OPENION_EXTERNAL_IRQ_COUNT,
            memory_base: kernel::generated_config::OPENION_QEMU_AN521_MEMORY_BASE,
            memory_size: kernel::generated_config::OPENION_QEMU_AN521_MEMORY_SIZE,
            kernel_end: _ebss as *const () as usize,
        }
    }

    fn early_init() {
        arch::arm::cortex_m::irq::disable();

        uart_init();
        kernel::log::set_console(&UART_CONSOLE);
        arch::arm::cortex_m::nvic::enable_irq(UART_IRQ as u16);
        let config = Self::config();

        arch::arm::cortex_m::systick::init(config.cpu_freq_hz, config.systick_hz);

        kernel::kdebug!("QEMU AN521 early init complete");
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        &PLATFORM_DRIVERS
    }

    fn net_device() -> Option<&'static kernel::driver::net::DynNetDevice> {
        Some(&LAN9118_ETH)
    }
}
