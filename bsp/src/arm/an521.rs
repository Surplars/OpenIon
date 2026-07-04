use kernel::driver::manager::AnyDriver;
use kernel::log::FunctionConsole;

#[cfg(feature = "driver_cmsdk_uart")]
use cmsdk_uart::CmsdkUart;
#[cfg(feature = "driver_lan9118")]
use lan9118::Lan9118;

pub const CPU_FREQ_HZ: u32 = 25_000_000;
pub const MEMORY_BASE: usize = 0x8000_0000;
pub const MEMORY_SIZE: usize = 16 * 1024 * 1024;

const UART_DATA: usize = 0x4020_0000;
pub const UART_IRQ: u32 = 0;
const UART_STATE: usize = 0x4020_0004;
const UART_STATE_TXBF: u32 = 1 << 0;
const UART_CTRL: usize = 0x4020_0008;

#[cfg(feature = "driver_lan9118")]
const LAN9118_BASE: usize = 0x4200_0000;
pub const LAN9118_IRQ: u32 = 48;

static UART_CONSOLE: FunctionConsole = FunctionConsole { putc_fn: uart_putc };

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

pub fn init_console() {
    uart_init();
    crate::install_console(&UART_CONSOLE);
}

pub fn drivers() -> &'static [&'static dyn AnyDriver] {
    &PLATFORM_DRIVERS
}

#[cfg(target_arch = "arm")]
pub fn init_irqs() {
    #[cfg(feature = "driver_cmsdk_uart")]
    {
        kernel::irq::add_irq_handler(UART_IRQ as usize, uart_irq_handler);
        arch::arm::cortex_m::nvic::enable_irq(UART_IRQ as u16);
    }

    #[cfg(feature = "driver_lan9118")]
    {
        kernel::irq::add_irq_handler(LAN9118_IRQ as usize, lan9118_irq_handler);
        arch::arm::cortex_m::nvic::enable_irq(LAN9118_IRQ as u16);
    }
}

#[cfg(not(target_arch = "arm"))]
pub fn init_irqs() {}

#[cfg(target_arch = "arm")]
pub fn init_timer() {
    let config = kernel::platform::get_config();
    arch::arm::cortex_m::systick::init(config.cpu_freq_hz, config.systick_hz);
}

#[cfg(not(target_arch = "arm"))]
pub fn init_timer() {}

pub fn net_device() -> Option<&'static kernel::driver::net::DynNetDevice> {
    #[cfg(feature = "driver_lan9118")]
    {
        Some(&LAN9118_ETH)
    }

    #[cfg(not(feature = "driver_lan9118"))]
    {
        None
    }
}

pub fn uart_irq() -> u32 {
    UART_IRQ
}

pub fn lan9118_irq() -> u32 {
    LAN9118_IRQ
}

#[cfg(all(target_arch = "arm", feature = "driver_cmsdk_uart"))]
fn uart_irq_handler() {
    kernel::driver::manager::DriverManager::dispatch_irq(UART_IRQ);
}

#[cfg(all(target_arch = "arm", feature = "driver_lan9118"))]
fn lan9118_irq_handler() {
    kernel::driver::manager::DriverManager::dispatch_irq(LAN9118_IRQ);
}

fn uart_init() {
    write_reg(UART_CTRL, 1 | (1 << 1));
}

fn uart_putc(ch: u8) {
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
