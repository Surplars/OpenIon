#![no_std]

use kernel::log::{FunctionConsole, set_console};

pub mod arm;
pub mod esp32;

pub fn install_console(console: &'static FunctionConsole) {
    set_console(console);
}
