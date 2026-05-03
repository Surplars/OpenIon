use core::fmt::{self, Write};
use spin::{Mutex, Once};

pub trait PlatformConsole {
    fn putc(&self, ch: u8);
}

pub trait CpuIdProvider {
    fn cpu_id(&self) -> u32;
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

static CONSOLE: Once<&'static (dyn PlatformConsole + Sync)> = Once::new();
static CPU_PROVIDER: Once<&'static (dyn CpuIdProvider + Sync)> = Once::new();

pub fn set_console(console: &'static (dyn PlatformConsole + Sync)) {
    CONSOLE.call_once(|| console);
}

pub fn set_cpu_id_provider(provider: &'static (dyn CpuIdProvider + Sync)) {
    CPU_PROVIDER.call_once(|| provider);
}

pub fn console() -> Option<&'static (dyn PlatformConsole + Sync)> {
    CONSOLE.get().copied()
}

struct ConsoleWriter;

static WRITER: Mutex<ConsoleWriter> = Mutex::new(ConsoleWriter);

impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(c) = console() {
            for &b in s.as_bytes() {
                if b == b'\n' {
                    c.putc(b'\r');
                }
                c.putc(b);
            }
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    crate::arch::disable_irq();
    if let Some(mut writer) = WRITER.try_lock() {
        let _ = writer.write_fmt(args);
    }
    crate::arch::enable_irq();
}

pub fn log(_level: LogLevel, args: fmt::Arguments) {
    let ticks = crate::timer::ticks();
    let secs = ticks / 1000;
    let ms = ticks % 1000;

    crate::arch::disable_irq();
    if let Some(mut w) = WRITER.try_lock() {
        // Timestamp: [  0.000]
        let _ = write!(&mut *w, "[{:>4}.{:03}]", secs, ms);

        let _ = w.write_str(" ");

        let _ = w.write_fmt(args);
        let _ = w.write_str("\n");
    }
    crate::arch::enable_irq();
}

#[macro_export]
macro_rules! kp {
    ($($arg:tt)*) => {
        $crate::log::_print(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kpln {
    () => {
        $crate::kp!("\n")
    };
    ($fmt:expr) => {
        $crate::kp!(concat!($fmt, "\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::kp!(concat!($fmt, "\n"), $($arg)*)
    };
}

#[macro_export]
macro_rules! klog {
    ($lvl:expr, $($arg:tt)*) => {
        $crate::log::log($lvl, core::format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {
        $crate::klog!($crate::log::LogLevel::Debug, $($arg)*);
    };
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {
        $crate::klog!($crate::log::LogLevel::Info, $($arg)*);
    };
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {
        $crate::klog!($crate::log::LogLevel::Warn, $($arg)*);
    };
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {
        $crate::klog!($crate::log::LogLevel::Error, $($arg)*);
    };
}
