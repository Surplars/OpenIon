use super::{Driver, DriverErr, DriverResult, GenericDeviceConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinMode {
    Input,
    InputPullUp,
    InputPullDown,
    OutputPushPull,
    OutputOpenDrain,
    Alternate(u8),
    Analog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinLevel {
    Low,
    High,
}

impl PinLevel {
    pub const fn is_high(self) -> bool {
        matches!(self, PinLevel::High)
    }

    pub const fn from_bool(value: bool) -> Self {
        if value { PinLevel::High } else { PinLevel::Low }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioIrqTrigger {
    RisingEdge,
    FallingEdge,
    BothEdges,
    LowLevel,
    HighLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioPin {
    pub bank: u8,
    pub index: u8,
}

impl GpioPin {
    pub const fn new(bank: u8, index: u8) -> Self {
        Self { bank, index }
    }
}

pub trait GpioController: Driver {
    fn pin_count(&self) -> usize;

    fn configure_pin(&self, pin: u8, mode: PinMode) -> DriverResult<()>;

    fn read_pin(&self, pin: u8) -> DriverResult<PinLevel>;

    fn write_pin(&self, pin: u8, level: PinLevel) -> DriverResult<()>;

    fn toggle_pin(&self, pin: u8) -> DriverResult<PinLevel> {
        let next = match self.read_pin(pin)? {
            PinLevel::Low => PinLevel::High,
            PinLevel::High => PinLevel::Low,
        };
        self.write_pin(pin, next)?;
        Ok(next)
    }

    fn configure_irq(&self, _pin: u8, _trigger: GpioIrqTrigger) -> DriverResult<()> {
        Err(DriverErr::NotSupported)
    }

    fn clear_irq(&self, _pin: u8) -> DriverResult<()> {
        Err(DriverErr::NotSupported)
    }
}

pub type DynGpioController = dyn GpioController<Config = GenericDeviceConfig, Error = DriverErr>;

pub fn validate_pin(controller: &DynGpioController, pin: u8) -> DriverResult<()> {
    if (pin as usize) < controller.pin_count() {
        Ok(())
    } else {
        Err(DriverErr::InvalidConfig)
    }
}
