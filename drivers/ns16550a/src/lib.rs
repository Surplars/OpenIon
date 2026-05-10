#![no_std]

use core::sync::atomic::{AtomicPtr, Ordering};
use kernel::driver::char::{CharDevice, DynCharDevice};
use kernel::driver::manager::AnyDriver;
use kernel::driver::terminal::{DynTerminalDevice, TerminalDevice};
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};

// NS16550A register offsets (byte-accessible)
const RBR: usize = 0; // Receive Buffer (read, DLAB=0)
const THR: usize = 0; // Transmitter Holding (write, DLAB=0)
const IER: usize = 1; // Interrupt Enable (DLAB=0)
const IIR: usize = 2; // Interrupt Identification (read)
const FCR: usize = 2; // FIFO Control (write)
const LCR: usize = 3; // Line Control
const LSR: usize = 5; // Line Status

const LSR_DR: u8 = 1 << 0; // Data Ready
const LSR_THRE: u8 = 1 << 5; // TX Holding Register Empty
const IIR_NO_INT: u8 = 1 << 0;

static ACTIVE_UART: AtomicPtr<Ns16550a> = AtomicPtr::new(core::ptr::null_mut());
static NS16550A_CONSOLE: Ns16550aConsole = Ns16550aConsole;

pub struct Ns16550a {
    base_addr: usize,
    irq_num: u32,
    reg_shift: u8,
    reg_io_width: u8,
}

impl Ns16550a {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self::with_layout(base_addr, irq_num, 0, 1)
    }

    pub const fn with_layout(
        base_addr: usize,
        irq_num: u32,
        reg_shift: u8,
        reg_io_width: u8,
    ) -> Self {
        Self {
            base_addr,
            irq_num,
            reg_shift,
            reg_io_width,
        }
    }

    fn reg_addr(&self, offset: usize) -> usize {
        self.base_addr + (offset << self.reg_shift)
    }

    fn read_reg(&self, offset: usize) -> u8 {
        let addr = self.reg_addr(offset);
        unsafe {
            match self.reg_io_width {
                4 => (addr as *const u32).read_volatile() as u8,
                _ => (addr as *const u8).read_volatile(),
            }
        }
    }

    fn write_reg(&self, offset: usize, value: u8) {
        let addr = self.reg_addr(offset);
        unsafe {
            match self.reg_io_width {
                4 => (addr as *mut u32).write_volatile(value as u32),
                _ => (addr as *mut u8).write_volatile(value),
            }
        }
    }

    pub fn init_hw(&self) {
        // Disable interrupts
        self.write_reg(IER, 0x00);

        // Set DLAB to configure baud rate divisor
        self.write_reg(LCR, 0x80);
        // Divisor = 3 -> 38400 baud (QEMU default clock)
        self.write_reg(0, 0x03);
        self.write_reg(1, 0x00);

        // 8N1, DLAB=0
        self.write_reg(LCR, 0x03);

        // Enable & clear FIFO, 14-byte trigger
        self.write_reg(FCR, 0xC7);

        // Enable RX interrupt
        self.write_reg(IER, 0x01);
    }

    pub fn putc(&self, ch: u8) {
        while self.read_reg(LSR) & LSR_THRE == 0 {}
        self.write_reg(THR, ch);
    }

    pub fn getc(&self) -> Option<u8> {
        if self.read_reg(LSR) & LSR_DR != 0 {
            Some(self.read_reg(RBR))
        } else {
            None
        }
    }

    pub fn irq_pending(&self) -> bool {
        self.read_reg(IIR) & IIR_NO_INT == 0
    }

    fn set_active(&self) {
        ACTIVE_UART.store(self as *const Self as *mut Self, Ordering::Release);
        kernel::log::set_console(&NS16550A_CONSOLE);
        kernel::driver::char::set_rx_poll_fn(active_uart_getc);
    }
}

struct Ns16550aConsole;

unsafe impl Sync for Ns16550aConsole {}

impl kernel::log::PlatformConsole for Ns16550aConsole {
    fn putc(&self, ch: u8) {
        if let Some(uart) = active_uart() {
            uart.putc(ch);
        }
    }
}

fn active_uart() -> Option<&'static Ns16550a> {
    let ptr = ACTIVE_UART.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn active_uart_getc() -> Option<u8> {
    active_uart().and_then(Ns16550a::getc)
}

impl Driver for Ns16550a {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "NS16550A UART"
    }

    fn init(&self) -> DriverResult<()> {
        self.init_hw();
        self.set_active();
        kernel::kinfo!("NS16550A UART initialized with RX interrupt enabled");
        Ok(())
    }

    fn handle_irq(&self, irq_id: u32) -> bool {
        if irq_id != self.irq_num {
            return false;
        }
        while let Some(byte) = self.getc() {
            kernel::driver::char::push_to_rx_buf(byte);
        }
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

impl CharDevice for Ns16550a {
    fn write_byte(&self, byte: u8) -> DriverResult<()> {
        self.putc(byte);
        Ok(())
    }

    fn read_byte(&self) -> DriverResult<u8> {
        self.getc().ok_or(DriverErr::Busy)
    }
}

impl TerminalDevice for Ns16550a {}

/// FDT-compatible factory for NS16550A UART.
/// Matches compatible = "ns16550a" and creates a driver instance.
pub struct Ns16550aFactory;

const MAX_NS16550A: usize = 4;
static DRIVER_POOL: StaticDriverPool<Ns16550a, MAX_NS16550A> = StaticDriverPool::new();

impl DriverFactory for Ns16550aFactory {
    fn compatible(&self) -> &[&str] {
        &["ns16550a", "ns16550", "ns16550a-uart", "snps,dw-apb-uart"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        DRIVER_POOL
            .alloc(Ns16550a::new(resource.base_addr, resource.irq))
            .map(|d| d as _)
    }

    fn probe_fdt(
        &self,
        resource: DeviceResource,
        node: kernel::fdt::FdtNode<'static>,
    ) -> Option<&'static dyn AnyDriver> {
        let reg_shift = node.prop_u32("reg-shift").unwrap_or(0).min(u8::MAX as u32) as u8;
        let reg_io_width = match node.prop_u32("reg-io-width").unwrap_or(1) {
            4 => 4,
            _ => 1,
        };

        DRIVER_POOL
            .alloc(Ns16550a::with_layout(
                resource.base_addr,
                resource.irq,
                reg_shift,
                reg_io_width,
            ))
            .map(|d| d as _)
    }
}
