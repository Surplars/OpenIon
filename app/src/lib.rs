#![no_std]

pub fn root_task() -> ! {
    kernel::process::root_task()
}
