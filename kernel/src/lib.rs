#![no_std]

extern crate alloc;

pub mod arch;
pub mod driver;
pub mod fdt;
pub mod fs;
pub mod generated_config;
pub mod hv;
pub mod irq;
pub mod log;
pub mod mm;
pub mod net;
pub mod platform;
pub mod process;
pub mod sched;
#[cfg(feature = "builtin_shell")]
pub mod shell;
pub mod sync;
pub mod syscall;
pub mod timer;
pub mod version;

use arch::Arch;
use platform::{Platform, PlatformConfig};

pub fn boot<P: Platform, A: Arch>() -> ! {
    arch::init::<A>();
    P::init_console();
    platform::set_smp_status_provider(P::smp_status);
    P::register_driver_factories();
    P::early_init();

    version::banner();

    let config: PlatformConfig = P::config();
    platform::set_config(config);
    kinfo!("config written");
    mm::init(&config);
    P::init_memory();
    core_init();
    hv::init(cfg!(feature = "hypervisor"), false);
    kinfo!("kernel core init done");
    P::init_irqs();
    P::init_timer();

    if generated_config::OPENION_FDT && generated_config::OPENION_FDT_AUTO_PROBE {
        // FDT auto-probing: discover and init drivers from device tree.
        let fdt_count = driver::manager::DriverManager::auto_probe_fdt();
        if fdt_count > 0 {
            kinfo!("driver_manager: auto-probed {} driver(s)", fdt_count);
        }
    }

    auto_drivers_init::<P>();

    // Initialize VFS
    fs::init();

    // Register device files in /dev
    register_dev_files();

    // Initialize network stack if available
    net::init::<P>();

    kinfo!("Setting up root process...");
    sched::Scheduler::init_system_tasks(process::root_task);

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
            kerror!("{}: register failed", drv.name());
        } else {
            if let Err(_e) = drv.auto_init() {
                kerror!("{}: init failed", drv.name());
            } else {
                kdebug!("{}: registered and initialized", drv.name());
            }
        }
    }
}

/// Auto-create device files in /dev for all registered drivers.
fn register_dev_files() {
    let dev = match fs::resolve_path("/dev") {
        Ok(d) => d,
        Err(_) => return,
    };

    let mut blk_idx: u32 = 0;
    let mut char_idx: u32 = 0;
    let mut gpio_idx: u32 = 0;
    let mut net_idx: u32 = 0;

    driver::manager::DriverManager::for_each_block_device(|_| {
        let idx = blk_idx;
        blk_idx += 1;
        let dev_name = format_dev_name("blk", idx);
        let name_str = core::str::from_utf8(&dev_name.0[..dev_name.1]).unwrap_or("");
        match fs::lookup(dev, name_str) {
            Ok(_) => {}
            Err(_) => {
                let _ = fs::create_file(dev, name_str);
                kdebug!("VFS: created /dev/{}", name_str);
            }
        }
    });

    driver::manager::DriverManager::for_each_char_device(|_| {
        let idx = char_idx;
        char_idx += 1;
        let dev_name = format_dev_name("ttyS", idx);
        let name_str = core::str::from_utf8(&dev_name.0[..dev_name.1]).unwrap_or("");
        match fs::lookup(dev, name_str) {
            Ok(_) => {}
            Err(_) => {
                let _ = fs::create_file(dev, name_str);
                kdebug!("VFS: created /dev/{}", name_str);
            }
        }
    });

    driver::manager::DriverManager::for_each_gpio_controller(|_| {
        let idx = gpio_idx;
        gpio_idx += 1;
        let dev_name = format_dev_name("gpio", idx);
        let name_str = core::str::from_utf8(&dev_name.0[..dev_name.1]).unwrap_or("");
        match fs::lookup(dev, name_str) {
            Ok(_) => {}
            Err(_) => {
                let _ = fs::create_file(dev, name_str);
                kdebug!("VFS: created /dev/{}", name_str);
            }
        }
    });

    driver::manager::DriverManager::for_each_net_device(|_| {
        let idx = net_idx;
        net_idx += 1;
        let dev_name = format_dev_name("eth", idx);
        let name_str = core::str::from_utf8(&dev_name.0[..dev_name.1]).unwrap_or("");
        match fs::lookup(dev, name_str) {
            Ok(_) => {}
            Err(_) => {
                let _ = fs::create_file(dev, name_str);
                kdebug!("VFS: created /dev/{}", name_str);
            }
        }
    });
}

fn format_dev_name(prefix: &str, idx: u32) -> ([u8; 16], usize) {
    let mut buf = [0u8; 16];
    let mut pos = 0;
    for &b in prefix.as_bytes() {
        if pos >= 15 {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    // Write index digits
    let mut tmp = [0u8; 10];
    let mut len = 0;
    let mut n = idx;
    loop {
        tmp[len] = b'0' + (n % 10) as u8;
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    // Reverse digits
    for i in (0..len).rev() {
        if pos >= 15 {
            break;
        }
        buf[pos] = tmp[i];
        pos += 1;
    }
    (buf, pos)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kerror!("KERNEL_PANIC: {}", info.message());
    loop {}
}
