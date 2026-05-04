#![no_std]

use kernel::driver::char::CharDevice;
use kernel::driver::manager::AnyDriver;
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

pub struct Ns16550a {
    base_addr: usize,
    irq_num: u32,
}

impl Ns16550a {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self { base_addr, irq_num }
    }

    fn reg(&self, offset: usize) -> *mut u8 {
        (self.base_addr + offset) as *mut u8
    }

    pub fn init_hw(&self) {
        unsafe {
            // Disable interrupts
            self.reg(IER).write_volatile(0x00);

            // Set DLAB to configure baud rate divisor
            self.reg(LCR).write_volatile(0x80);
            // Divisor = 3 → 38400 baud (QEMU default clock)
            self.reg(0).write_volatile(0x03);
            self.reg(1).write_volatile(0x00);

            // 8N1, DLAB=0
            self.reg(LCR).write_volatile(0x03);

            // Enable & clear FIFO, 14-byte trigger
            self.reg(FCR).write_volatile(0xC7);

            // Enable RX interrupt
            self.reg(IER).write_volatile(0x01);
        }
    }

    pub fn putc(&self, ch: u8) {
        unsafe {
            while self.reg(LSR).read_volatile() & LSR_THRE == 0 {}
            self.reg(THR).write_volatile(ch);
        }
    }

    pub fn getc(&self) -> Option<u8> {
        unsafe {
            if self.reg(LSR).read_volatile() & LSR_DR != 0 {
                Some(self.reg(RBR).read_volatile())
            } else {
                None
            }
        }
    }

    pub fn irq_pending(&self) -> bool {
        unsafe { self.reg(IIR).read_volatile() & IIR_NO_INT == 0 }
    }
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

/// FDT-compatible factory for NS16550A UART.
/// Matches compatible = "ns16550a" and creates a driver instance.
pub struct Ns16550aFactory;

const MAX_NS16550A: usize = 4;
static DRIVER_POOL: StaticDriverPool<Ns16550a, MAX_NS16550A> = StaticDriverPool::new();

impl DriverFactory for Ns16550aFactory {
    fn compatible(&self) -> &[&str] {
        &["ns16550a"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        DRIVER_POOL
            .alloc(Ns16550a::new(resource.base_addr, resource.irq))
            .map(|d| d as _)
    }
}
