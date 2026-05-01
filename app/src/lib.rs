#![no_std]

use kernel::sched::Scheduler;
use kernel::kinfo;

static mut SHELL_TASK_STACK: [usize; kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()] =
    [0; kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()];

pub fn root_task() -> ! {
    kinfo!("Root process started, spawning shell...");

    Scheduler::create_task(
        kernel::shell::shell_main,
        unsafe { &mut *core::ptr::addr_of_mut!(SHELL_TASK_STACK) },
        2,
        "SHELL",
    );

    kinfo!("Root process sleeping...");
    loop {
        Scheduler::delay(10000);
    }
}
