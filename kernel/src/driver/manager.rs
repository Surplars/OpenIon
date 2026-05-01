use super::{DeviceState, Driver, DriverErr, DriverResult, DriverFactory};
use crate::sync::Mutex;

pub trait AnyDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn auto_init(&self) -> DriverResult<()>;
    fn handle_irq(&self, irq_id: u32) -> bool;
    fn check_health(&self) -> DriverResult<()>;
    fn power_on(&self) -> DriverResult<()>;
    fn power_off(&self) -> DriverResult<()>;
    fn state(&self) -> DeviceState;
    fn as_block_device(&self) -> Option<&'static super::block::DynBlockDevice>;
}

impl<T: Driver> AnyDriver for T {
    fn name(&self) -> &'static str { self.name() }
    fn auto_init(&self) -> DriverResult<()> { self.init() }
    fn handle_irq(&self, irq_id: u32) -> bool { self.handle_irq(irq_id) }
    fn check_health(&self) -> DriverResult<()> { self.check_health() }
    fn power_on(&self) -> DriverResult<()> { self.power_on() }
    fn power_off(&self) -> DriverResult<()> { self.power_off() }
    fn state(&self) -> DeviceState { self.state() }
    fn as_block_device(&self) -> Option<&'static super::block::DynBlockDevice> { self.as_block_device() }
}

const MAX_DRIVERS: usize = 32;
const MAX_FACTORIES: usize = 16;

/// Registered drivers (manual + auto-probed).
static DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> = Mutex::new([None; MAX_DRIVERS]);

/// Registered driver factories for FDT auto-probing.
static FACTORIES: Mutex<[Option<&'static dyn DriverFactory>; MAX_FACTORIES]> = Mutex::new([None; MAX_FACTORIES]);

pub struct DriverManager;

impl DriverManager {
    /// Register a driver manually (used by Platform::drivers() and MCU platforms).
    pub fn register_driver(driver: &'static dyn AnyDriver) -> DriverResult<()> {
        let mut table = DRIVERS.lock();
        for slot in table.iter() {
            if let Some(existing) = slot {
                if existing.name() == driver.name() {
                    return Err(DriverErr::InvalidConfig);
                }
            }
        }
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(driver);
                return Ok(());
            }
        }
        Err(DriverErr::RegistryFull)
    }

    pub fn unregister_driver(name: &str) -> DriverResult<()> {
        let mut table = DRIVERS.lock();
        for slot in table.iter_mut() {
            if let Some(existing) = slot {
                if existing.name() == name {
                    *slot = None;
                    return Ok(());
                }
            }
        }
        Err(DriverErr::NotFound)
    }

    pub fn get_driver(name: &str) -> Option<&'static dyn AnyDriver> {
        let table = DRIVERS.lock();
        for slot in table.iter() {
            if let Some(driver) = slot {
                if driver.name() == name {
                    return Some(*driver);
                }
            }
        }
        None
    }

    pub fn dispatch_irq(irq_id: u32) -> bool {
        let table = DRIVERS.lock();
        for slot in table.iter() {
            if let Some(driver) = slot {
                if driver.handle_irq(irq_id) {
                    return true;
                }
            }
        }
        false
    }

    /// Iterate over all registered drivers. Used by VFS for block device discovery.
    pub fn for_each_driver(mut f: impl FnMut(&dyn AnyDriver)) {
        let table = DRIVERS.lock();
        for slot in table.iter() {
            if let Some(driver) = slot {
                f(*driver);
            }
        }
    }

    // ---- Factory registration & FDT auto-probe ----

    /// Register a driver factory for FDT-compatible auto-probing.
    pub fn register_factory(factory: &'static dyn DriverFactory) -> DriverResult<()> {
        let mut table = FACTORIES.lock();
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(factory);
                return Ok(());
            }
        }
        Err(DriverErr::RegistryFull)
    }

    /// Probe FDT for devices and auto-register matching drivers.
    /// Call this after registering factories and setting DTB address.
    /// Returns the number of drivers auto-probed.
    pub fn auto_probe_fdt() -> usize {
        let dtb = crate::platform::dtb_addr();
        if dtb == 0 {
            return 0;
        }

        let factories = FACTORIES.lock();
        let mut count = 0usize;

        unsafe {
            crate::fdt::parse_with(dtb, |_name, compat, reg, interrupt| {
                if reg.len() < 16 {
                    return;
                }
                let base_addr = u64::from_be_bytes([
                    reg[0], reg[1], reg[2], reg[3],
                    reg[4], reg[5], reg[6], reg[7],
                ]) as usize;
                let _size = u64::from_be_bytes([
                    reg[8], reg[9], reg[10], reg[11],
                    reg[12], reg[13], reg[14], reg[15],
                ]);

                for factory in factories.iter() {
                    if let Some(f) = factory {
                        for &c in f.compatible() {
                            if compat == c {
                                if let Some(driver) = f.probe(base_addr, interrupt) {
                                    if Self::register_driver(driver).is_ok() {
                                        if driver.auto_init().is_ok() {
                                            crate::kdebug!("FDT auto: {} @{:#x} irq={}", driver.name(), base_addr, interrupt);
                                        }
                                        count += 1;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            });
        }

        count
    }
}
