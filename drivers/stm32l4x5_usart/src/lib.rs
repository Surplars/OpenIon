#![no_std]

use kernel::driver::char::{CharDevice, DynCharDevice};
use kernel::driver::manager::AnyDriver;
use kernel::driver::terminal::{DynTerminalDevice, TerminalDevice};
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};

const CR1: usize = 0x00;
const BRR: usize = 0x0c;
const ISR: usize = 0x1c;
const RDR: usize = 0x24;
const TDR: usize = 0x28;

const CR1_UE: u32 = 1 << 0;
const CR1_RE: u32 = 1 << 2;
const CR1_TE: u32 = 1 << 3;
const CR1_RXNEIE: u32 = 1 << 5;

const ISR_RXNE: u32 = 1 << 5;
const ISR_TXE: u32 = 1 << 7;

pub struct Stm32l4x5Usart {
    base_addr: usize,
    irq_num: u32,
    pclk_hz: u32,
    baud: u32,
}

impl Stm32l4x5Usart {
    pub const fn new(base_addr: usize, irq_num: u32, pclk_hz: u32, baud: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            pclk_hz,
            baud,
        }
    }

    #[inline(always)]
    fn reg(&self, offset: usize) -> *mut u32 {
        (self.base_addr + offset) as *mut u32
    }

    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { self.reg(offset).read_volatile() }
    }

    fn write_reg(&self, offset: usize, val: u32) {
        unsafe { self.reg(offset).write_volatile(val) }
    }

    pub fn init_hw(&self) {
        self.write_reg(CR1, 0);
        let div = if self.baud == 0 {
            0
        } else {
            self.pclk_hz / self.baud
        };
        if div != 0 {
            self.write_reg(BRR, div);
        }
        self.write_reg(CR1, CR1_UE | CR1_TE | CR1_RE | CR1_RXNEIE);
    }

    pub fn putc(&self, ch: u8) {
        while self.read_reg(ISR) & ISR_TXE == 0 {}
        self.write_reg(TDR, ch as u32);
    }

    pub fn getc(&self) -> Option<u8> {
        if self.read_reg(ISR) & ISR_RXNE != 0 {
            Some(self.read_reg(RDR) as u8)
        } else {
            None
        }
    }
}

impl Driver for Stm32l4x5Usart {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "STM32L4x5 USART"
    }

    fn init(&self) -> DriverResult<()> {
        self.init_hw();
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

impl CharDevice for Stm32l4x5Usart {
    fn read_byte(&self) -> DriverResult<u8> {
        self.getc().ok_or(DriverErr::Busy)
    }

    fn write_byte(&self, byte: u8) -> DriverResult<()> {
        self.putc(byte);
        Ok(())
    }
}

impl TerminalDevice for Stm32l4x5Usart {}

pub struct Stm32l4x5UsartFactory {
    pub pclk_hz: u32,
    pub baud: u32,
}

const MAX_USART: usize = 4;
static USART_POOL: StaticDriverPool<Stm32l4x5Usart, MAX_USART> = StaticDriverPool::new();

impl DriverFactory for Stm32l4x5UsartFactory {
    fn compatible(&self) -> &[&str] {
        &["st,stm32l4-usart", "st,stm32-usart"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        USART_POOL
            .alloc(Stm32l4x5Usart::new(
                resource.base_addr,
                resource.irq,
                self.pclk_hz,
                self.baud,
            ))
            .map(|d| d as _)
    }
}
