pub mod char;
pub mod block;
pub mod manager;
pub mod net;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverErr {
        InitFailed = 0,
        Timeout = 1,
        InvalidConfig = 2,
        HardwareFault = 3,
        Busy = 4,
        NotSupported = 5,
        RegistryFull = 6,
        NotFound = 7,
        Custom = 255,
}

pub type DriverResult<T> = Result<T, DriverErr>;

pub trait DeviceConfig {
    fn base_address(&self) -> usize;
    fn irq_number(&self) -> u32;
}

#[derive(Debug, Clone, Copy)]
pub struct GenericDeviceConfig {
    pub base_addr: usize,
    pub irq_num: u32,
}

impl GenericDeviceConfig {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
        }
    }
}

impl DeviceConfig for GenericDeviceConfig {
    fn base_address(&self) -> usize {
        self.base_addr
    }

    fn irq_number(&self) -> u32 {
        self.irq_num
    }
}

pub enum DeviceState {
    Uninitialized = 0,
    Ready = 1,
    Busy = 2,
    Error = 3,
    Suspended = 4,
}

impl DeviceState {
    pub const fn is_ready(&self) -> bool {
        matches!(self, DeviceState::Ready)
    }

    pub const fn is_error(&self) -> bool {
        matches!(self, DeviceState::Error)
    }
}

pub trait Driver: Send + Sync {
    type Config: DeviceConfig;
    type Error: Copy;

    fn get_config(&self) -> Self::Config;
    fn name(&self) -> &'static str;
    fn init(&self) -> DriverResult<()>;
    fn handle_irq(&self, irq_id: u32) -> bool;

    fn check_health(&self) -> DriverResult<()> { Ok(()) }
    fn power_on(&self) -> DriverResult<()> { Ok(()) }
    fn power_off(&self) -> DriverResult<()> { Ok(()) }
    fn state(&self) -> DeviceState { DeviceState::Ready }

    /// If this driver is a block device, return a 'static reference.
    /// VFS uses this to discover mountable storage. Drivers are stored in
    /// static pools, so the reference is valid for the kernel lifetime.
    fn as_block_device(&self) -> Option<&'static block::DynBlockDevice> { None }
}

/// Factory for auto-probing drivers from FDT compatible strings.
///
/// Drivers implement this to declare which FDT `compatible` values they handle.
/// During boot, the kernel scans FDT and calls `probe()` for each matching node.
///
/// MCU platforms without FDT ignore factories and use `Platform::drivers()` directly.
pub trait DriverFactory: Send + Sync {
    /// FDT compatible strings this driver handles (e.g. `["ns16550a"]`, `["virtio,mmio"]`).
    fn compatible(&self) -> &[&str];

    /// Try to create a driver instance for the given device.
    /// `base_addr` and `irq` come from the FDT `reg` and `interrupts` properties.
    /// Returns `Some(driver)` if this factory can handle the device.
    fn probe(&self, base_addr: usize, irq: u32) -> Option<&'static dyn manager::AnyDriver>;
}


