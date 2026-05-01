use spin::Once;

#[derive(Clone, Copy)]
pub struct PlatformConfig {
    pub cpu_freq_hz: u32,
    pub systick_hz: u32,
    pub external_irq_count: usize,
}

pub trait Platform {
    fn config() -> PlatformConfig;
    fn early_init();

    fn drivers() -> &'static [&'static dyn crate::driver::manager::AnyDriver] {
        &[]
    }

    fn net_device() -> Option<&'static crate::driver::net::DynNetDevice> {
        None
    }
}

static CONFIG: Once<PlatformConfig> = Once::new();

/// DTB (Device Tree Blob) address, set by platform before boot().
static DTB_ADDR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn set_config(cfg: PlatformConfig) {
    CONFIG.call_once(|| cfg);
}

pub fn get_config() -> &'static PlatformConfig {
    CONFIG.get().expect("config not initialized")
}

/// Set the DTB address (called from platform rust_main before boot).
pub fn set_dtb_addr(addr: usize) {
    DTB_ADDR.store(addr, core::sync::atomic::Ordering::Relaxed);
}

/// Get the DTB address.
pub fn dtb_addr() -> usize {
    DTB_ADDR.load(core::sync::atomic::Ordering::Relaxed)
}
