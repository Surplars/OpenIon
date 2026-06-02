#![no_std]

use core::sync::atomic::{AtomicPtr, Ordering};
use kernel::driver::char::{CharDevice, DynCharDevice};
use kernel::driver::manager::AnyDriver;
use kernel::driver::terminal::{DynTerminalDevice, TerminalDevice};
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};

const UART_FIFO: usize = 0x00;
const UART_STATUS: usize = 0x1c;
const UART_CLKDIV: usize = 0x14;
const UART_CONF0: usize = 0x20;

const UART_TXFIFO_CNT_SHIFT: u32 = 16;
const UART_TXFIFO_CNT_MASK: u32 = 0xff << UART_TXFIFO_CNT_SHIFT;
const UART_RXFIFO_CNT_MASK: u32 = 0xff;
const UART_TXFIFO_DEPTH: u32 = 128;
const UART_RXFIFO_RST: u32 = 1 << 23;
const UART_TXFIFO_RST: u32 = 1 << 22;

static ACTIVE_UART: AtomicPtr<Esp32s31Uart> = AtomicPtr::new(core::ptr::null_mut());
static ESP32S31_UART_CONSOLE: Esp32s31UartConsole = Esp32s31UartConsole;

pub struct Esp32s31Uart {
    base_addr: usize,
    irq_num: u32,
    input_clock_hz: u32,
    baudrate: u32,
}

impl Esp32s31Uart {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            input_clock_hz: 80_000_000,
            baudrate: 115_200,
        }
    }

    pub const fn with_clock(
        base_addr: usize,
        irq_num: u32,
        input_clock_hz: u32,
        baudrate: u32,
    ) -> Self {
        Self {
            base_addr,
            irq_num,
            input_clock_hz,
            baudrate,
        }
    }

    fn reg(&self, offset: usize) -> *mut u32 {
        (self.base_addr + offset) as *mut u32
    }

    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { self.reg(offset).read_volatile() }
    }

    fn write_reg(&self, offset: usize, value: u32) {
        unsafe { self.reg(offset).write_volatile(value) }
    }

    fn update_reg(&self, offset: usize, f: impl FnOnce(u32) -> u32) {
        let old = self.read_reg(offset);
        self.write_reg(offset, f(old));
    }

    pub fn init_hw(&self) {
        let baudrate = self.baudrate.max(1);
        let clkdiv = self.input_clock_hz / baudrate;
        if clkdiv != 0 {
            self.write_reg(UART_CLKDIV, clkdiv & 0x0fff);
        }

        self.update_reg(UART_CONF0, |v| v | UART_RXFIFO_RST | UART_TXFIFO_RST);
        self.update_reg(UART_CONF0, |v| v & !(UART_RXFIFO_RST | UART_TXFIFO_RST));
    }

    pub fn putc(&self, ch: u8) {
        while self.txfifo_count() >= UART_TXFIFO_DEPTH {}
        self.write_reg(UART_FIFO, ch as u32);
    }

    pub fn getc(&self) -> Option<u8> {
        if self.rxfifo_count() == 0 {
            None
        } else {
            Some((self.read_reg(UART_FIFO) & 0xff) as u8)
        }
    }

    fn txfifo_count(&self) -> u32 {
        (self.read_reg(UART_STATUS) & UART_TXFIFO_CNT_MASK) >> UART_TXFIFO_CNT_SHIFT
    }

    fn rxfifo_count(&self) -> u32 {
        self.read_reg(UART_STATUS) & UART_RXFIFO_CNT_MASK
    }

    fn drain_rx(&self) {
        while let Some(byte) = self.getc() {
            kernel::driver::char::push_to_rx_buf(byte);
        }
    }

    fn set_active(&self) {
        ACTIVE_UART.store(self as *const Self as *mut Self, Ordering::Release);
        kernel::log::set_console(&ESP32S31_UART_CONSOLE);
        kernel::driver::char::set_rx_poll_fn(active_uart_getc);
    }
}

struct Esp32s31UartConsole;

unsafe impl Sync for Esp32s31UartConsole {}

impl kernel::log::PlatformConsole for Esp32s31UartConsole {
    fn putc(&self, ch: u8) {
        if let Some(uart) = active_uart() {
            uart.putc(ch);
        }
    }
}

fn active_uart() -> Option<&'static Esp32s31Uart> {
    let ptr = ACTIVE_UART.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn active_uart_getc() -> Option<u8> {
    active_uart().and_then(Esp32s31Uart::getc)
}

impl Driver for Esp32s31Uart {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::with_mmio(self.base_addr, 0xa0, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "ESP32-S31 UART"
    }

    fn init(&self) -> DriverResult<()> {
        self.init_hw();
        self.set_active();
        kernel::kinfo!("ESP32-S31 UART initialized");
        Ok(())
    }

    fn handle_irq(&self, irq_id: u32) -> bool {
        if irq_id != self.irq_num {
            return false;
        }
        self.drain_rx();
        true
    }

    fn as_char_device(&self) -> Option<&'static DynCharDevice> {
        let dev: &DynCharDevice = self;
        Some(unsafe { core::mem::transmute::<&DynCharDevice, &'static DynCharDevice>(dev) })
    }

    fn as_terminal_device(&self) -> Option<&'static DynTerminalDevice> {
        let dev: &DynTerminalDevice = self;
        Some(unsafe { core::mem::transmute::<&DynTerminalDevice, &'static DynTerminalDevice>(dev) })
    }
}

impl CharDevice for Esp32s31Uart {
    fn write_byte(&self, byte: u8) -> DriverResult<()> {
        self.putc(byte);
        Ok(())
    }

    fn read_byte(&self) -> DriverResult<u8> {
        self.getc().ok_or(DriverErr::Busy)
    }
}

impl TerminalDevice for Esp32s31Uart {}

pub struct Esp32s31UartFactory;

const MAX_ESP32S31_UARTS: usize = 3;
static DRIVER_POOL: StaticDriverPool<Esp32s31Uart, MAX_ESP32S31_UARTS> = StaticDriverPool::new();

impl DriverFactory for Esp32s31UartFactory {
    fn compatible(&self) -> &[&str] {
        &["esp,esp32s31-uart", "espressif,esp32s31-uart"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        DRIVER_POOL
            .alloc(Esp32s31Uart::new(resource.base_addr, resource.irq))
            .map(|d| d as _)
    }

    fn probe_fdt(
        &self,
        resource: DeviceResource,
        node: kernel::fdt::FdtNode<'static>,
    ) -> Option<&'static dyn AnyDriver> {
        let input_clock_hz = node.prop_u32("clock-frequency").unwrap_or(80_000_000);
        let baudrate = node.prop_u32("current-speed").unwrap_or(115_200);

        DRIVER_POOL
            .alloc(Esp32s31Uart::with_clock(
                resource.base_addr,
                resource.irq,
                input_clock_hz,
                baudrate,
            ))
            .map(|d| d as _)
    }
}
