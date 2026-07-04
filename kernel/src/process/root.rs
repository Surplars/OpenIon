#[cfg(any(feature = "async_rt", feature = "builtin_shell"))]
use super::init::InitError;
use crate::sched::Scheduler;

#[cfg(any(feature = "async_rt", feature = "builtin_shell"))]
use super::init::{InitManager, InitResult, InitService};

#[cfg(feature = "builtin_shell")]
static mut SHELL_TASK_STACK: [usize;
    crate::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()] =
    [0; crate::shell::BUILTIN_SHELL_STACK_SIZE / core::mem::size_of::<usize>()];

#[cfg(feature = "async_rt")]
static mut ASYNC_TASK_STACK: [usize; 1024] = [0; 1024];

#[cfg(any(feature = "async_rt", feature = "builtin_shell"))]
pub fn root_task() -> ! {
    let mut init = InitManager::new();

    #[cfg(feature = "async_rt")]
    register_init_service(
        &mut init,
        InitService::new("async", false, start_async_runtime),
    );

    #[cfg(feature = "builtin_shell")]
    register_init_service(
        &mut init,
        InitService::new("shell", true, start_builtin_shell),
    );

    let stats = init.run();

    #[cfg(not(feature = "builtin_shell"))]
    if stats.started == 0 {
        crate::kerror!(
            "No init process: builtin_shell is disabled and no external init is configured."
        );
        crate::kerror!("Enable OPENION_BUILTIN_SHELL or provide an init program.");
        panic!("No init process available");
    }

    crate::kinfo!(
        "Root process initialized: registered={}, started={}, skipped={}, failed={}",
        stats.registered,
        stats.started,
        stats.skipped,
        stats.failed
    );

    loop {
        Scheduler::delay(10000);
    }
}

#[cfg(not(any(feature = "async_rt", feature = "builtin_shell")))]
pub fn root_task() -> ! {
    loop {
        Scheduler::delay(10000);
    }
}

#[cfg(any(feature = "async_rt", feature = "builtin_shell"))]
fn register_init_service(init: &mut InitManager, service: InitService) {
    if let Err(err) = init.register(service) {
        crate::kerror!("init: failed to register {}: {:?}", service.name, err);
    }
}

#[cfg(feature = "async_rt")]
fn start_async_runtime() -> InitResult {
    if !crate::generated_config::OPENION_ASYNC_RT {
        return Err(InitError::Disabled);
    }

    crate::kinfo!("Root process starting async runtime...");
    let _ = crate::sched::async_rt::spawn("heartbeat", crate::sched::async_rt::heartbeat_task());
    let _ = crate::sched::async_rt::spawn("demo-event", crate::sched::async_rt::demo_event_task());
    let _ = crate::sched::async_rt::spawn("rx-counter", crate::sched::async_rt::rx_counter_task());

    let id = Scheduler::create_task(
        crate::sched::async_rt::executor_main,
        unsafe { &mut *core::ptr::addr_of_mut!(ASYNC_TASK_STACK) },
        1,
        "ASYNC",
    );

    if id == u32::MAX {
        return Err(InitError::StartFailed);
    }

    Ok(())
}

#[cfg(feature = "builtin_shell")]
fn start_builtin_shell() -> InitResult {
    crate::kinfo!("Root process starting shell init...");

    let id = Scheduler::create_task(
        crate::shell::shell_main,
        unsafe { &mut *core::ptr::addr_of_mut!(SHELL_TASK_STACK) },
        1,
        "SHELL",
    );

    if id == u32::MAX {
        return Err(InitError::StartFailed);
    }

    Ok(())
}
