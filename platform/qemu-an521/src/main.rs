#![no_std]
#![no_main]

pub mod irq;

#[cfg(feature = "driver_cmsdk_uart")]
use cmsdk_uart::CmsdkUart;
use core::arch::global_asm;
use kernel::driver::manager::AnyDriver;
use kernel::platform::{Platform, PlatformConfig};
#[cfg(feature = "driver_lan9118")]
use lan9118::Lan9118;

// Chip startup assembly: sets up stack pointer and calls platform_init() to enter Rust
global_asm!(include_str!("../startup.s"));

static UART_CONSOLE: UartConsole = UartConsole;

#[cfg(feature = "driver_cmsdk_uart")]
static UART: CmsdkUart = CmsdkUart::new(UART_DATA, UART_IRQ);
#[cfg(feature = "driver_lan9118")]
static LAN9118_ETH: Lan9118 = Lan9118::new(kernel::driver::GenericDeviceConfig::new(
    LAN9118_BASE,
    LAN9118_IRQ,
));

#[cfg(all(feature = "driver_cmsdk_uart", feature = "driver_lan9118"))]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 2] = [&UART, &LAN9118_ETH];
#[cfg(all(feature = "driver_cmsdk_uart", not(feature = "driver_lan9118")))]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 1] = [&UART];
#[cfg(all(not(feature = "driver_cmsdk_uart"), feature = "driver_lan9118"))]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 1] = [&LAN9118_ETH];
#[cfg(all(not(feature = "driver_cmsdk_uart"), not(feature = "driver_lan9118")))]
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

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

const CPU_FREQ_HZ: u32 = 25_000_000;
const MEMORY_BASE: usize = 0x8000_0000;
const MEMORY_SIZE: usize = 16 * 1024 * 1024;
const UART_DATA: usize = 0x4020_0000;
const UART_IRQ: u32 = 0;
#[cfg(feature = "driver_lan9118")]
const LAN9118_BASE: usize = 0x4200_0000;
const LAN9118_IRQ: u32 = 48;

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

        uart_init();
        kernel::log::set_console(&UART_CONSOLE);
        #[cfg(feature = "driver_cmsdk_uart")]
        arch::arm::cortex_m::nvic::enable_irq(UART_IRQ as u16);
        let config = Self::config();

        arch::arm::cortex_m::systick::init(config.cpu_freq_hz, config.systick_hz);

        kernel::kdebug!("QEMU AN521 early init complete");
    }

    fn drivers() -> &'static [&'static dyn AnyDriver] {
        &PLATFORM_DRIVERS
    }

    fn net_device() -> Option<&'static kernel::driver::net::DynNetDevice> {
        #[cfg(feature = "driver_lan9118")]
        {
            Some(&LAN9118_ETH)
        }

        #[cfg(not(feature = "driver_lan9118"))]
        {
            None
        }
    }
}
