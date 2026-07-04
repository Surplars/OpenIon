use super::{
    DeviceConfig, DeviceResource, DeviceState, Driver, DriverErr, DriverFactory, DriverResult,
};
use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

pub trait AnyDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn identity(&self) -> DriverIdentity {
        DriverIdentity::new(self.name(), self.resource())
    }
    fn auto_init(&self) -> DriverResult<()>;
    fn handle_irq(&self, irq_id: u32) -> bool;
    fn check_health(&self) -> DriverResult<()>;
    fn power_on(&self) -> DriverResult<()>;
    fn power_off(&self) -> DriverResult<()>;
    fn state(&self) -> DeviceState;
    fn resource(&self) -> DeviceResource;
    fn as_block_device(&self) -> Option<&'static super::block::DynBlockDevice>;
    fn as_char_device(&self) -> Option<&'static super::char::DynCharDevice>;
    fn as_terminal_device(&self) -> Option<&'static super::terminal::DynTerminalDevice>;
    fn as_gpio_controller(&self) -> Option<&'static super::gpio::DynGpioController>;
    fn as_framebuffer_device(&self) -> Option<&'static super::framebuffer::DynFramebufferDevice>;
    fn as_net_device(&self) -> Option<&'static super::net::DynNetDevice>;
    fn as_rng_device(&self) -> Option<&'static super::rng::DynRngDevice>;
    fn device_class(&self) -> super::DeviceClass {
        let mut class = super::DeviceClass::empty();
        if self.as_block_device().is_some() {
            class |= super::DeviceClass::BLOCK;
        }
        if self.as_char_device().is_some() {
            class |= super::DeviceClass::CHAR;
        }
        if self.as_terminal_device().is_some() {
            class |= super::DeviceClass::TERMINAL;
        }
        if self.as_gpio_controller().is_some() {
            class |= super::DeviceClass::GPIO;
        }
        if self.as_framebuffer_device().is_some() {
            class |= super::DeviceClass::FRAMEBUFFER;
        }
        if self.as_net_device().is_some() {
            class |= super::DeviceClass::NET;
        }
        if self.as_rng_device().is_some() {
            class |= super::DeviceClass::RNG;
        }
        class
    }

    fn device_class_name(&self) -> &'static str {
        let class = self.device_class();
        if class.contains(super::DeviceClass::BLOCK) {
            "block"
        } else if class.contains(super::DeviceClass::CHAR) {
            "char"
        } else if class.contains(super::DeviceClass::TERMINAL) {
            "terminal"
        } else if class.contains(super::DeviceClass::GPIO) {
            "gpio"
        } else if class.contains(super::DeviceClass::FRAMEBUFFER) {
            "framebuffer"
        } else if class.contains(super::DeviceClass::NET) {
            "net"
        } else if class.contains(super::DeviceClass::RNG) {
            "rng"
        } else {
            "other"
        }
    }
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
    fn resource(&self) -> DeviceResource {
        self.get_config().resource()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverIdentity {
    pub class: &'static str,
    pub base_addr: usize,
    pub irq: u32,
}

impl DriverIdentity {
    pub const fn new(class: &'static str, resource: DeviceResource) -> Self {
        Self {
            class,
            base_addr: resource.base_addr,
            irq: resource.irq,
        }
    }
}

const MAX_DRIVERS: usize = 32;
const MAX_FACTORIES: usize = 16;
pub const DRIVER_SNAPSHOT_CAP: usize = MAX_DRIVERS;

#[derive(Clone, Copy)]
pub struct DriverRegistryStats {
    pub registered_drivers: usize,
    pub registered_factories: usize,
    pub registered_block_devices: usize,
    pub registered_char_devices: usize,
    pub registered_terminal_devices: usize,
    pub registered_gpio_controllers: usize,
    pub registered_framebuffer_devices: usize,
    pub registered_net_devices: usize,
    pub registered_rng_devices: usize,
    pub probe_matches: u32,
    pub probe_successes: u32,
    pub probe_failures: u32,
    pub irq_dispatches: u32,
    pub irq_driver_hits: u32,
    pub irq_driver_misses: u32,
    pub irq_fast_slots: usize,
    pub irq_fast_hits: u32,
    pub irq_fast_fallbacks: u32,
    pub irq_fast_conflicts: u32,
}

/// Registered drivers (manual + auto-probed).
static DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);

static BLOCK_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static CHAR_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static TERMINAL_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static GPIO_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static FRAMEBUFFER_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static NET_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);
static RNG_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]> =
    Mutex::new([None; MAX_DRIVERS]);

/// Registered driver factories for FDT auto-probing.
static FACTORIES: Mutex<[Option<&'static dyn DriverFactory>; MAX_FACTORIES]> =
    Mutex::new([None; MAX_FACTORIES]);

static IRQ_DRIVERS: Mutex<[Option<&'static dyn AnyDriver>; crate::irq::MAX_EXTERNAL_IRQS]> =
    Mutex::new([None; crate::irq::MAX_EXTERNAL_IRQS]);

static PROBE_MATCHES: AtomicU32 = AtomicU32::new(0);
static PROBE_SUCCESSES: AtomicU32 = AtomicU32::new(0);
static PROBE_FAILURES: AtomicU32 = AtomicU32::new(0);
static IRQ_DISPATCHES: AtomicU32 = AtomicU32::new(0);
static IRQ_DRIVER_HITS: AtomicU32 = AtomicU32::new(0);
static IRQ_DRIVER_MISSES: AtomicU32 = AtomicU32::new(0);
static IRQ_FAST_HITS: AtomicU32 = AtomicU32::new(0);
static IRQ_FAST_FALLBACKS: AtomicU32 = AtomicU32::new(0);
static IRQ_FAST_CONFLICTS: AtomicU32 = AtomicU32::new(0);
static REGISTERED_DRIVERS: AtomicU32 = AtomicU32::new(0);
static REGISTERED_FACTORIES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_BLOCK_DEVICES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_CHAR_DEVICES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_TERMINAL_DEVICES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_GPIO_CONTROLLERS: AtomicU32 = AtomicU32::new(0);
static REGISTERED_FRAMEBUFFER_DEVICES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_NET_DEVICES: AtomicU32 = AtomicU32::new(0);
static REGISTERED_RNG_DEVICES: AtomicU32 = AtomicU32::new(0);

pub struct DriverManager;

fn is_valid_driver_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn snapshot_bucket(
    bucket: &Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]>,
) -> [Option<&'static dyn AnyDriver>; MAX_DRIVERS] {
    let table = bucket.lock();
    let mut snapshot = [None; MAX_DRIVERS];
    for (dst, src) in snapshot.iter_mut().zip(table.iter()) {
        *dst = *src;
    }
    snapshot
}

fn bucket_has_space(bucket: &Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]>) -> bool {
    let table = bucket.lock();
    table.iter().any(|slot| slot.is_none())
}

fn push_bucket_slot(
    bucket: &Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]>,
    driver: &'static dyn AnyDriver,
) {
    let mut table = bucket.lock();
    for slot in table.iter_mut() {
        if slot.is_none() {
            *slot = Some(driver);
            return;
        }
    }
}

fn bump_registered_count(counter: &AtomicU32, register: bool) {
    if register {
        counter.fetch_add(1, Ordering::AcqRel);
    } else {
        counter.fetch_sub(1, Ordering::AcqRel);
    }
}

fn bump_registered_class_counts(class: super::DeviceClass, register: bool) {
    if class.contains(super::DeviceClass::BLOCK) {
        bump_registered_count(&REGISTERED_BLOCK_DEVICES, register);
    }
    if class.contains(super::DeviceClass::CHAR) {
        bump_registered_count(&REGISTERED_CHAR_DEVICES, register);
    }
    if class.contains(super::DeviceClass::TERMINAL) {
        bump_registered_count(&REGISTERED_TERMINAL_DEVICES, register);
    }
    if class.contains(super::DeviceClass::GPIO) {
        bump_registered_count(&REGISTERED_GPIO_CONTROLLERS, register);
    }
    if class.contains(super::DeviceClass::FRAMEBUFFER) {
        bump_registered_count(&REGISTERED_FRAMEBUFFER_DEVICES, register);
    }
    if class.contains(super::DeviceClass::NET) {
        bump_registered_count(&REGISTERED_NET_DEVICES, register);
    }
    if class.contains(super::DeviceClass::RNG) {
        bump_registered_count(&REGISTERED_RNG_DEVICES, register);
    }
}

fn remove_bucket_slot(
    bucket: &Mutex<[Option<&'static dyn AnyDriver>; MAX_DRIVERS]>,
    driver: &'static dyn AnyDriver,
) {
    let mut table = bucket.lock();
    for slot in table.iter_mut() {
        if let Some(existing) = slot {
            if core::ptr::addr_eq(*existing, driver) {
                *slot = None;
                return;
            }
        }
    }
}

impl DriverManager {
    /// Register a driver manually (used by Platform::drivers() and MCU platforms).
    pub fn register_driver(driver: &'static dyn AnyDriver) -> DriverResult<()> {
        if !is_valid_driver_name(driver.name()) {
            return Err(DriverErr::InvalidConfig);
        }

        let identity = driver.identity();
        let mut table = DRIVERS.lock();
        let mut first_empty = None;
        for (i, slot) in table.iter().enumerate() {
            if let Some(existing) = slot {
                if existing.identity() == identity {
                    return Err(DriverErr::InvalidConfig);
                }
            } else if first_empty.is_none() {
                first_empty = Some(i);
            }
        }
        let Some(idx) = first_empty else {
            return Err(DriverErr::RegistryFull);
        };

        let class = driver.device_class();
        let need_block = class.contains(super::DeviceClass::BLOCK);
        let need_char = class.contains(super::DeviceClass::CHAR);
        let need_term = class.contains(super::DeviceClass::TERMINAL);
        let need_gpio = class.contains(super::DeviceClass::GPIO);
        let need_framebuffer = class.contains(super::DeviceClass::FRAMEBUFFER);
        let need_net = class.contains(super::DeviceClass::NET);
        let need_rng = class.contains(super::DeviceClass::RNG);

        if (need_block && !bucket_has_space(&BLOCK_DRIVERS))
            || (need_char && !bucket_has_space(&CHAR_DRIVERS))
            || (need_term && !bucket_has_space(&TERMINAL_DRIVERS))
            || (need_gpio && !bucket_has_space(&GPIO_DRIVERS))
            || (need_framebuffer && !bucket_has_space(&FRAMEBUFFER_DRIVERS))
            || (need_net && !bucket_has_space(&NET_DRIVERS))
            || (need_rng && !bucket_has_space(&RNG_DRIVERS))
        {
            return Err(DriverErr::RegistryFull);
        }

        table[idx] = Some(driver);
        drop(table);

        if need_block {
            push_bucket_slot(&BLOCK_DRIVERS, driver);
        }
        if need_char {
            push_bucket_slot(&CHAR_DRIVERS, driver);
        }
        if need_term {
            push_bucket_slot(&TERMINAL_DRIVERS, driver);
        }
        if need_gpio {
            push_bucket_slot(&GPIO_DRIVERS, driver);
        }
        if need_framebuffer {
            push_bucket_slot(&FRAMEBUFFER_DRIVERS, driver);
        }
        if need_net {
            push_bucket_slot(&NET_DRIVERS, driver);
        }
        if need_rng {
            push_bucket_slot(&RNG_DRIVERS, driver);
        }

        REGISTERED_DRIVERS.fetch_add(1, Ordering::AcqRel);
        bump_registered_class_counts(class, true);
        Self::register_irq_fast_path(driver);
        Ok(())
    }

    pub fn unregister_driver(name: &str) -> DriverResult<()> {
        let mut table = DRIVERS.lock();
        for slot in table.iter_mut() {
            if let Some(existing) = slot {
                if existing.name() == name {
                    let driver = *existing;
                    let class = driver.device_class();
                    *slot = None;
                    drop(table);
                    Self::unregister_irq_fast_path(driver);
                    remove_bucket_slot(&BLOCK_DRIVERS, driver);
                    remove_bucket_slot(&CHAR_DRIVERS, driver);
                    remove_bucket_slot(&TERMINAL_DRIVERS, driver);
                    remove_bucket_slot(&GPIO_DRIVERS, driver);
                    remove_bucket_slot(&FRAMEBUFFER_DRIVERS, driver);
                    remove_bucket_slot(&NET_DRIVERS, driver);
                    remove_bucket_slot(&RNG_DRIVERS, driver);
                    REGISTERED_DRIVERS.fetch_sub(1, Ordering::AcqRel);
                    bump_registered_class_counts(class, false);
                    return Ok(());
                }
            }
        }
        Err(DriverErr::NotFound)
    }

    pub fn unregister_driver_by_identity(identity: DriverIdentity) -> DriverResult<()> {
        let mut table = DRIVERS.lock();
        for slot in table.iter_mut() {
            if let Some(existing) = slot {
                if existing.identity() == identity {
                    let driver = *existing;
                    let class = driver.device_class();
                    *slot = None;
                    drop(table);
                    Self::unregister_irq_fast_path(driver);
                    remove_bucket_slot(&BLOCK_DRIVERS, driver);
                    remove_bucket_slot(&CHAR_DRIVERS, driver);
                    remove_bucket_slot(&TERMINAL_DRIVERS, driver);
                    remove_bucket_slot(&GPIO_DRIVERS, driver);
                    remove_bucket_slot(&FRAMEBUFFER_DRIVERS, driver);
                    remove_bucket_slot(&NET_DRIVERS, driver);
                    remove_bucket_slot(&RNG_DRIVERS, driver);
                    REGISTERED_DRIVERS.fetch_sub(1, Ordering::AcqRel);
                    bump_registered_class_counts(class, false);
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

    pub fn get_driver_by_identity(identity: DriverIdentity) -> Option<&'static dyn AnyDriver> {
        let table = DRIVERS.lock();
        for slot in table.iter() {
            if let Some(driver) = slot {
                if driver.identity() == identity {
                    return Some(*driver);
                }
            }
        }
        None
    }

    pub fn dispatch_irq(irq_id: u32) -> bool {
        IRQ_DISPATCHES.fetch_add(1, Ordering::AcqRel);

        if let Some(driver) = Self::irq_fast_driver(irq_id) {
            if driver.handle_irq(irq_id) {
                IRQ_FAST_HITS.fetch_add(1, Ordering::AcqRel);
                IRQ_DRIVER_HITS.fetch_add(1, Ordering::AcqRel);
                return true;
            }
            IRQ_FAST_FALLBACKS.fetch_add(1, Ordering::AcqRel);
        }

        let snapshot = Self::drivers_snapshot();
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if driver.handle_irq(irq_id) {
                    IRQ_DRIVER_HITS.fetch_add(1, Ordering::AcqRel);
                    return true;
                }
            }
        }
        IRQ_DRIVER_MISSES.fetch_add(1, Ordering::AcqRel);
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

    pub fn for_each_block_device(mut f: impl FnMut(&'static super::block::DynBlockDevice)) {
        let snapshot = snapshot_bucket(&BLOCK_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_block_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_char_device(mut f: impl FnMut(&'static super::char::DynCharDevice)) {
        let snapshot = snapshot_bucket(&CHAR_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_char_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_terminal_device(
        mut f: impl FnMut(&'static super::terminal::DynTerminalDevice),
    ) {
        let snapshot = snapshot_bucket(&TERMINAL_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_terminal_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_gpio_controller(mut f: impl FnMut(&'static super::gpio::DynGpioController)) {
        let snapshot = snapshot_bucket(&GPIO_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_gpio_controller() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_framebuffer_device(
        mut f: impl FnMut(&'static super::framebuffer::DynFramebufferDevice),
    ) {
        let snapshot = snapshot_bucket(&FRAMEBUFFER_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_framebuffer_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_net_device(mut f: impl FnMut(&'static super::net::DynNetDevice)) {
        let snapshot = snapshot_bucket(&NET_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_net_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn for_each_rng_device(mut f: impl FnMut(&'static super::rng::DynRngDevice)) {
        let snapshot = snapshot_bucket(&RNG_DRIVERS);
        for slot in snapshot.iter() {
            if let Some(driver) = *slot {
                if let Some(dev) = driver.as_rng_device() {
                    f(dev);
                }
            }
        }
    }

    pub fn drivers_snapshot() -> [Option<&'static dyn AnyDriver>; MAX_DRIVERS] {
        snapshot_bucket(&DRIVERS)
    }

    fn register_irq_fast_path(driver: &'static dyn AnyDriver) {
        let irq = driver.resource().irq as usize;
        if irq == 0 || irq >= crate::irq::MAX_EXTERNAL_IRQS {
            return;
        }

        let mut table = IRQ_DRIVERS.lock();
        match table[irq] {
            None => table[irq] = Some(driver),
            Some(existing) if core::ptr::addr_eq(existing, driver) => {}
            Some(_) => {
                IRQ_FAST_CONFLICTS.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    fn unregister_irq_fast_path(driver: &'static dyn AnyDriver) {
        let irq = driver.resource().irq as usize;
        if irq == 0 || irq >= crate::irq::MAX_EXTERNAL_IRQS {
            return;
        }

        let mut table = IRQ_DRIVERS.lock();
        if let Some(existing) = table[irq] {
            if core::ptr::addr_eq(existing, driver) {
                table[irq] = None;
            }
        }
    }

    fn irq_fast_driver(irq_id: u32) -> Option<&'static dyn AnyDriver> {
        let irq = irq_id as usize;
        if irq >= crate::irq::MAX_EXTERNAL_IRQS {
            return None;
        }
        let table = IRQ_DRIVERS.lock();
        table[irq]
    }

    fn irq_fast_slot_count() -> usize {
        let table = IRQ_DRIVERS.lock();
        table.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn registry_stats() -> DriverRegistryStats {
        DriverRegistryStats {
            registered_drivers: REGISTERED_DRIVERS.load(Ordering::Acquire) as usize,
            registered_factories: REGISTERED_FACTORIES.load(Ordering::Acquire) as usize,
            registered_block_devices: REGISTERED_BLOCK_DEVICES.load(Ordering::Acquire) as usize,
            registered_char_devices: REGISTERED_CHAR_DEVICES.load(Ordering::Acquire) as usize,
            registered_terminal_devices: REGISTERED_TERMINAL_DEVICES.load(Ordering::Acquire)
                as usize,
            registered_gpio_controllers: REGISTERED_GPIO_CONTROLLERS.load(Ordering::Acquire)
                as usize,
            registered_framebuffer_devices: REGISTERED_FRAMEBUFFER_DEVICES.load(Ordering::Acquire)
                as usize,
            registered_net_devices: REGISTERED_NET_DEVICES.load(Ordering::Acquire) as usize,
            registered_rng_devices: REGISTERED_RNG_DEVICES.load(Ordering::Acquire) as usize,
            probe_matches: PROBE_MATCHES.load(Ordering::Acquire),
            probe_successes: PROBE_SUCCESSES.load(Ordering::Acquire),
            probe_failures: PROBE_FAILURES.load(Ordering::Acquire),
            irq_dispatches: IRQ_DISPATCHES.load(Ordering::Acquire),
            irq_driver_hits: IRQ_DRIVER_HITS.load(Ordering::Acquire),
            irq_driver_misses: IRQ_DRIVER_MISSES.load(Ordering::Acquire),
            irq_fast_slots: Self::irq_fast_slot_count(),
            irq_fast_hits: IRQ_FAST_HITS.load(Ordering::Acquire),
            irq_fast_fallbacks: IRQ_FAST_FALLBACKS.load(Ordering::Acquire),
            irq_fast_conflicts: IRQ_FAST_CONFLICTS.load(Ordering::Acquire),
        }
    }

    // ---- Factory registration & FDT auto-probe ----

    /// Register a driver factory for FDT-compatible auto-probing.
    pub fn register_factory(factory: &'static dyn DriverFactory) -> DriverResult<()> {
        let mut table = FACTORIES.lock();
        let mut first_empty = None;
        for (idx, slot) in table.iter().enumerate() {
            if let Some(existing) = slot {
                if core::ptr::eq(*existing, factory) {
                    return Err(DriverErr::InvalidConfig);
                }
            } else if first_empty.is_none() {
                first_empty = Some(idx);
            }
        }
        if let Some(idx) = first_empty {
            table[idx] = Some(factory);
            Ok(())
        } else {
            Err(DriverErr::RegistryFull)
        }
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
                if !node.is_available() {
                    return;
                }
                let Some(reg) = node.first_reg() else {
                    return;
                };
                let resource = DeviceResource::new(reg.base, reg.size, node.interrupt_or_zero());

                let mut probed = false;
                for factory in factories.iter() {
                    if let Some(f) = *factory {
                        for &c in f.compatible() {
                            if node.compatible_matches(c) {
                                PROBE_MATCHES.fetch_add(1, Ordering::AcqRel);
                                if let Some(driver) = f.probe_fdt(resource, node) {
                                    match Self::register_driver(driver) {
                                        Ok(()) => match driver.auto_init() {
                                            Ok(()) => {
                                                PROBE_SUCCESSES.fetch_add(1, Ordering::AcqRel);
                                                crate::kdebug!(
                                                    "{}: auto-probed base={:#x} size={:#x} irq={}",
                                                    driver.name(),
                                                    resource.base_addr,
                                                    resource.size,
                                                    resource.irq
                                                );
                                                count += 1;
                                                probed = true;
                                            }
                                            Err(err) => {
                                                PROBE_FAILURES.fetch_add(1, Ordering::AcqRel);
                                                crate::kerror!(
                                                    "{}: init failed: {:?}",
                                                    driver.name(),
                                                    err
                                                );
                                            }
                                        },
                                        Err(err) => {
                                            PROBE_FAILURES.fetch_add(1, Ordering::AcqRel);
                                            crate::kwarn!(
                                                "{}: register failed: {:?}",
                                                driver.name(),
                                                err
                                            );
                                        }
                                    }
                                } else {
                                    PROBE_FAILURES.fetch_add(1, Ordering::AcqRel);
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
