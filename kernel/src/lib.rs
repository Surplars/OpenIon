#![no_std]

pub mod arch;
pub mod driver;
pub mod fdt;
pub mod fs;
pub mod irq;
pub mod log;
pub mod mm;
pub mod net;
pub mod platform;
pub mod process;
pub mod sched;
pub mod shell;
pub mod sync;
pub mod timer;
pub mod version;

use arch::Arch;
use platform::{Platform, PlatformConfig};

pub fn boot<P: Platform, A: Arch>(root_task_entry: fn() -> !) -> ! {
    arch::init::<A>();
    P::early_init();

    version::banner();

    let config: PlatformConfig = P::config();
    platform::set_config(config);
    kinfo!("config written");
    core_init();
    kinfo!("kernel core init done");

    // FDT auto-probing: discover and init drivers from device tree
    let fdt_count = driver::manager::DriverManager::auto_probe_fdt();
    if fdt_count > 0 {
        kinfo!("FDT auto-probed {} driver(s)", fdt_count);
    }

    auto_drivers_init::<P>();

    // Initialize VFS
    fs::init();

    // Register device files in /dev
    register_dev_files();

    // Initialize network stack if available
    net::init::<P>();

    kinfo!("Setting up root process...");
    sched::Scheduler::init_system_tasks(root_task_entry);

    kinfo!("Starting scheduler...");
    // Pick the first task
    sched::Scheduler::schedule();

    // Interrupts will be enabled inside start_first_task
    A::start_first_task();
}

fn core_init() {
    timer::init(platform::get_config().systick_hz);
    irq::init(platform::get_config().external_irq_count);
    sched::Scheduler::init();
}

fn auto_drivers_init<P: Platform>() {
    let drivers = P::drivers();
    for i in 0..drivers.len() {
        let drv = drivers[i];
        if let Err(_e) = driver::manager::DriverManager::register_driver(drv) {
            kerror!("Failed to register driver: {}", drv.name());
        } else {
            if let Err(_e) = drv.auto_init() {
                kerror!("Failed to init driver: {}", drv.name());
            } else {
                kdebug!("Driver registered & initialized: {}", drv.name());
            }
        }
    }
}

/// Auto-create device files in /dev for all registered drivers.
fn register_dev_files() {
    let dev_dir = fs::resolve_path("/dev");
    let dev = match dev_dir {
        Some(d) => d,
        None => return,
    };

    let mut blk_idx: u32 = 0;
    let mut char_idx: u32 = 0;

    driver::manager::DriverManager::for_each_driver(|drv| {
        let dev_name = if drv.as_block_device().is_some() {
            blk_idx += 1;
            let idx = blk_idx - 1;
            // format "blkN"
            let mut buf = [0u8; 16];
            let s = if idx == 0 { "blk0" } else if idx == 1 { "blk1" } else { "blk2" };
            let b = s.as_bytes();
            let len = b.len().min(15);
            buf[..len].copy_from_slice(&b[..len]);
            (buf, len)
        } else {
            char_idx += 1;
            let idx = char_idx - 1;
            let mut buf = [0u8; 16];
            let s = if idx == 0 { "ttyS0" } else if idx == 1 { "ttyS1" } else { "ttyS2" };
            let b = s.as_bytes();
            let len = b.len().min(15);
            buf[..len].copy_from_slice(&b[..len]);
            (buf, len)
        };
        let name_str = core::str::from_utf8(&dev_name.0[..dev_name.1]).unwrap_or("");
        match fs::lookup(dev, name_str) {
            Some(_) => {}
            None => {
                fs::create_file(dev, name_str);
                kdebug!("VFS: created /dev/{}", name_str);
            }
        }
    });
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kerror!("KERNEL_PANIC: {}", info.message());
    loop {}
}
