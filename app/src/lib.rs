#![no_std]

use kernel::kinfo;
use kernel::sched::Scheduler;

#[cfg(feature = "builtin_shell")]
static mut SHELL_TASK_STACK: [usize;
    kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()] =
    [0; kernel::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()];

#[cfg(feature = "async_rt")]
static mut ASYNC_TASK_STACK: [usize; 1024] = [0; 1024];

pub fn root_task() -> ! {
    #[cfg(feature = "async_rt")]
    if kernel::generated_config::OPENION_ASYNC_RT {
        kinfo!("Root process starting async runtime...");
        let _ =
            kernel::sched::async_rt::spawn("heartbeat", kernel::sched::async_rt::heartbeat_task());
        let _ = kernel::sched::async_rt::spawn(
            "demo-event",
            kernel::sched::async_rt::demo_event_task(),
        );
        let _ = kernel::sched::async_rt::spawn(
            "rx-counter",
            kernel::sched::async_rt::rx_counter_task(),
        );
        Scheduler::create_task(
            kernel::sched::async_rt::executor_main,
            unsafe { &mut *core::ptr::addr_of_mut!(ASYNC_TASK_STACK) },
            1,
            "ASYNC",
        );
    }

    #[cfg(feature = "builtin_shell")]
    {
        kinfo!("Root process started, spawning shell...");

        Scheduler::create_task(
            kernel::shell::shell_main,
            unsafe { &mut *core::ptr::addr_of_mut!(SHELL_TASK_STACK) },
            1,
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
