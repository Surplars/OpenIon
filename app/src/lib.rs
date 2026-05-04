#![no_std]

use kernel::kinfo;
use kernel::sched::Scheduler;

#[cfg(feature = "builtin_shell")]
static mut SHELL_TASK_STACK: [usize;
    kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()] =
    [0; kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()];

pub fn root_task() -> ! {
    #[cfg(feature = "builtin_shell")]
    {
        kinfo!("Root process started, spawning shell...");

        Scheduler::create_task(
            kernel::shell::shell_main,
            unsafe { &mut *core::ptr::addr_of_mut!(SHELL_TASK_STACK) },
            2,
            "SHELL",
        );
    }

    #[cfg(not(feature = "builtin_shell"))]
    kinfo!("Root process started with built-in shell disabled.");

    kinfo!("Root process sleeping...");
    loop {
        Scheduler::delay(10000);
    }
}
