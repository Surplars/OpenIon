use super::{DeviceResource, DeviceState, Driver, DriverErr, DriverFactory, DriverResult};
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
    fn as_char_device(&self) -> Option<&'static super::char::DynCharDevice>;
    fn as_terminal_device(&self) -> Option<&'static super::terminal::DynTerminalDevice>;
    fn as_gpio_controller(&self) -> Option<&'static super::gpio::DynGpioController>;
    fn as_framebuffer_device(&self) -> Option<&'static super::framebuffer::DynFramebufferDevice>;
    fn as_net_device(&self) -> Option<&'static super::net::DynNetDevice>;
    fn as_rng_device(&self) -> Option<&'static super::rng::DynRngDevice>;
}

impl<T: Driver> AnyDriver for T {
    fn name(&self) -> &'static str {
        self.name()
    }
    fn auto_init(&self) -> DriverResult<()> {
        self.init()
    }
    fn handle_irq(&self, irq_id: u32) -> bool {
        self.handle_irq(irq_id)
    }
    fn check_health(&self) -> DriverResult<()> {
        self.check_health()
    }
    fn power_on(&self) -> DriverResult<()> {
        self.power_on()
    }
    fn power_off(&self) -> DriverResult<()> {
        self.power_off()
    }
    fn state(&self) -> DeviceState {
        self.state()
    }
    fn as_block_device(&self) -> Option<&'static super::block::DynBlockDevice> {
        self.as_block_device()
    }
    fn as_char_device(&self) -> Option<&'static super::char::DynCharDevice> {
        self.as_char_device()
    }
    fn as_terminal_device(&self) -> Option<&'static super::terminal::DynTerminalDevice> {
        self.as_terminal_device()
    }
    fn as_gpio_controller(&self) -> Option<&'static super::gpio::DynGpioController> {
        self.as_gpio_controller()
    }
    fn as_framebuffer_device(&self) -> Option<&'static super::framebuffer::DynFramebufferDevice> {
        self.as_framebuffer_device()
    }
    fn as_net_device(&self) -> Option<&'static super::net::DynNetDevice> {
        self.as_net_device()
    }
    fn as_rng_device(&self) -> Option<&'static super::rng::DynRngDevice> {
        self.as_rng_device()
    }
}

const MAX_DRIVERS: usize = 32;
const MAX_FACTORIES: usize = 16;
pub const DRIVER_SNAPSHOT_CAP: usize = MAX_DRIVERS;

/// Registered drivers (manual + auto-probed).
static DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);

/// Registered driver factories for FDT auto-probing.
static FACTORIES: Mutex<[Option<&'static dyn DriverFactory>; MAX_FACTORIES]> =
    Mutex::new([None; MAX_FACTORIES]);

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
        let snapshot = Self::drivers_snapshot();
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if driver.handle_irq(irq_id) {
                    return true;
                }
            }
        }
        false
    }

    /// Iterate over all registered drivers. Used by VFS for block device discovery.
    pub fn for_each_driver(mut f: impl FnMut(&dyn AnyDriver)) {
        let snapshot = Self::drivers_snapshot();

        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                f(driver);
            }
        }
    }

    pub fn drivers_snapshot() -> [Option<&'static dyn AnyDriver>; MAX_DRIVERS] {
        let table = DRIVERS.lock();
        let mut snapshot = [None; MAX_DRIVERS];
        for (dst, src) in snapshot.iter_mut().zip(table.iter()) {
            *dst = *src;
        }
        snapshot
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

        let factories = {
            let table = FACTORIES.lock();
            let mut snapshot = [None; MAX_FACTORIES];
            for (dst, src) in snapshot.iter_mut().zip(table.iter()) {
                *dst = *src;
            }
            snapshot
        };
        let mut count = 0usize;

        unsafe {
            crate::fdt::walk_nodes(dtb, |node| {
                let Some(reg) = node.first_reg() else {
                    return;
                };
                let resource = DeviceResource::new(reg.base, reg.size, node.interrupt_or_zero());

                let mut probed = false;
                for factory in factories.iter() {
                    if let Some(f) = *factory {
                        for &c in f.compatible() {
                            if node.compatible_matches(c) {
                                if let Some(driver) = f.probe_fdt(resource, node) {
                                    if Self::register_driver(driver).is_ok() {
                                        if driver.auto_init().is_ok() {
                                            crate::kdebug!(
                                                "FDT auto: {} @{:#x}+{:#x} irq={}",
                                                driver.name(),
                                                resource.base_addr,
                                                resource.size,
                                                resource.irq
                                            );
                                        }
                                        count += 1;
                                        probed = true;
                                    }
                                }
                                if probed {
                                    break;
                                }
                            }
                        }
                        if probed {
                            break;
                        }
                    }
                }
            });
        }

        count
    }
}
