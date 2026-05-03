pub mod block;
pub mod char;
pub mod manager;
pub mod net;
pub mod pool;

pub use pool::StaticDriverPool;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceResource {
    pub base_addr: usize,
    pub size: usize,
    pub irq: u32,
}

impl DeviceResource {
    pub const fn new(base_addr: usize, size: usize, irq: u32) -> Self {
        Self {
            base_addr,
            size,
            irq,
        }
    }

    pub const fn without_size(base_addr: usize, irq: u32) -> Self {
        Self::new(base_addr, 0, irq)
    }
}

pub trait DeviceConfig {
    fn resource(&self) -> DeviceResource;

    fn base_address(&self) -> usize;
    fn mmio_size(&self) -> usize {
        self.resource().size
    }
    fn irq_number(&self) -> u32 {
        self.resource().irq
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GenericDeviceConfig {
    pub base_addr: usize,
    pub size: usize,
    pub irq_num: u32,
}

impl GenericDeviceConfig {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self::from_resource(DeviceResource::without_size(base_addr, irq_num))
    }

    pub const fn with_mmio(base_addr: usize, size: usize, irq_num: u32) -> Self {
        Self::from_resource(DeviceResource::new(base_addr, size, irq_num))
    }

    pub const fn from_resource(resource: DeviceResource) -> Self {
        Self {
            base_addr: resource.base_addr,
            size: resource.size,
            irq_num: resource.irq,
        }
    }
}

impl DeviceConfig for GenericDeviceConfig {
    fn resource(&self) -> DeviceResource {
        DeviceResource::new(self.base_addr, self.size, self.irq_num)
    }

    fn base_address(&self) -> usize {
        self.base_addr
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

    fn check_health(&self) -> DriverResult<()> {
        Ok(())
    }
    fn power_on(&self) -> DriverResult<()> {
        Ok(())
    }
    fn power_off(&self) -> DriverResult<()> {
        Ok(())
    }
    fn state(&self) -> DeviceState {
        DeviceState::Ready
    }

    /// If this driver is a block device, return a 'static reference.
    /// VFS uses this to discover mountable storage. Drivers are stored in
    /// static pools, so the reference is valid for the kernel lifetime.
    fn as_block_device(&self) -> Option<&'static block::DynBlockDevice> {
        None
    }
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
    /// The resource normally comes from FDT `reg` and `interrupts`, but the same
    /// shape is also used by platforms that register MMIO devices manually.
    /// Returns `Some(driver)` if this factory can handle the device.
    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn manager::AnyDriver>;
}
