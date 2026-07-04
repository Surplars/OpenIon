use spin::Once;

#[derive(Clone, Copy)]
pub struct SmpStatus {
    pub enabled: bool,
    pub possible_cpus: usize,
    pub online_cpus: usize,
    pub active_cpus: usize,
    pub parked_cpus: usize,
    pub boot_cpu: u32,
    pub current_cpu: u32,
    pub online_mask: usize,
    pub active_mask: usize,
    pub parked_mask: usize,
    pub start_attempts: usize,
    pub start_failures: usize,
}

impl SmpStatus {
    pub const fn single_cpu() -> Self {
        Self {
            enabled: false,
            possible_cpus: 1,
            online_cpus: 1,
            active_cpus: 1,
            parked_cpus: 0,
            boot_cpu: 0,
            current_cpu: 0,
            online_mask: 1,
            active_mask: 1,
            parked_mask: 0,
            start_attempts: 0,
            start_failures: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PlatformConfig {
    pub cpu_freq_hz: u32,
    pub systick_hz: u32,
    pub external_irq_count: usize,
    pub memory_base: usize,
    pub memory_size: usize,
    pub kernel_end: usize,
}

pub trait Platform {
    fn config() -> PlatformConfig;
    fn early_init();

    fn smp_status() -> SmpStatus {
        SmpStatus::single_cpu()
    }

    /// Install the earliest console and CPU identity providers.
    fn init_console() {}

    /// Register FDT driver factories before generic auto-probing runs.
    fn register_driver_factories() {}

    /// Initialize platform interrupt controllers after the kernel IRQ table exists.
    fn init_irqs() {}

    /// Enable architecture memory translation/protection after the generic
    /// heap and frame allocator are ready.
    fn init_memory() {}

    /// Initialize the platform timer after the kernel timer core exists.
    fn init_timer() {}

    fn drivers() -> &'static [&'static dyn crate::driver::manager::AnyDriver] {
        &[]
    }

    fn net_device() -> Option<&'static crate::driver::net::DynNetDevice> {
        None
    }
}

static CONFIG: Once<PlatformConfig> = Once::new();
static NEXT_TIMER_TICK: Once<fn()> = Once::new();
static SMP_STATUS: Once<fn() -> SmpStatus> = Once::new();

/// DTB (Device Tree Blob) address, set by platform before boot().
static DTB_ADDR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn set_config(cfg: PlatformConfig) {
    CONFIG.call_once(|| cfg);
}

pub fn get_config() -> &'static PlatformConfig {
    CONFIG.get().expect("config not initialized")
}

pub fn set_smp_status_provider(provider: fn() -> SmpStatus) {
    SMP_STATUS.call_once(|| provider);
}

pub fn smp_status() -> SmpStatus {
    if let Some(provider) = SMP_STATUS.get() {
        provider()
    } else {
        SmpStatus::single_cpu()
    }
}

pub fn set_next_timer_tick(handler: fn()) {
    NEXT_TIMER_TICK.call_once(|| handler);
}

pub fn schedule_next_timer_tick() {
    if let Some(handler) = NEXT_TIMER_TICK.get() {
        handler();
    }
}

/// Set the DTB address (called from platform rust_main before boot).
pub fn set_dtb_addr(addr: usize) {
    DTB_ADDR.store(addr, core::sync::atomic::Ordering::Relaxed);
}

/// Get the DTB address.
pub fn dtb_addr() -> usize {
    DTB_ADDR.load(core::sync::atomic::Ordering::Relaxed)
}
