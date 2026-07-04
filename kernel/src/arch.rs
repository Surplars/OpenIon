pub trait Arch {
    fn enable_global_irq();
    fn disable_global_irq();

    fn init_task_stack(stack: &mut [usize], entry: usize) -> usize;

    fn yield_cpu();

    fn idle_hint();

    fn start_first_task() -> !;

    /// Return the current CPU's logical id (hartid on RISC-V, MPIDR on ARM).
    fn current_cpu_id() -> u32 {
        0 // Default: single-CPU
    }
}

use core::sync::atomic::{AtomicPtr, Ordering};

static DISABLE_IRQ_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static ENABLE_IRQ_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static INIT_TASK_STACK_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static YIELD_CPU_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static IDLE_HINT_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static EXTERNAL_IRQ_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static EXTERNAL_IRQ_ID_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static CURRENT_CPU_ID_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

fn store_fn_ptr(slot: &AtomicPtr<()>, f: usize) {
    slot.store(f as *mut (), Ordering::Release);
}

fn load_fn_ptr(slot: &AtomicPtr<()>) -> Option<usize> {
    let p = slot.load(Ordering::Acquire);
    if p.is_null() { None } else { Some(p as usize) }
}

pub fn init<A: Arch>() {
    store_fn_ptr(&DISABLE_IRQ_FN, A::disable_global_irq as *const () as usize);
    store_fn_ptr(&ENABLE_IRQ_FN, A::enable_global_irq as *const () as usize);
    store_fn_ptr(
        &INIT_TASK_STACK_FN,
        A::init_task_stack as *const () as usize,
    );
    store_fn_ptr(&YIELD_CPU_FN, A::yield_cpu as *const () as usize);
    store_fn_ptr(&IDLE_HINT_FN, A::idle_hint as *const () as usize);
    store_fn_ptr(&CURRENT_CPU_ID_FN, A::current_cpu_id as *const () as usize);
}

pub fn disable_irq() {
    let nest = critical_nest();
    if nest == 0 {
        if let Some(f) = load_fn_ptr(&DISABLE_IRQ_FN) {
            let f: fn() = unsafe { core::mem::transmute(f) };
            f();
        }
    }
    set_critical_nest(nest + 1);
}

pub fn enable_irq() {
    let nest = exit_critical();
    if nest == 0 && crate::sched::has_current_task() {
        if let Some(f) = load_fn_ptr(&ENABLE_IRQ_FN) {
            let f: fn() = unsafe { core::mem::transmute(f) };
            f();
        }
    }
}

#[inline]
pub fn enter_critical() -> usize {
    crate::sched::percpu::enter_critical()
}

#[inline]
pub fn exit_critical() -> usize {
    crate::sched::percpu::exit_critical()
}

#[inline]
pub fn critical_nest() -> usize {
    crate::sched::percpu::critical_nest()
}

#[inline]
pub fn in_critical_section() -> bool {
    critical_nest() != 0
}

#[inline]
fn set_critical_nest(nest: usize) {
    crate::sched::percpu::set_critical_nest(nest);
}

pub fn init_task_stack(stack: &mut [usize], entry: usize) -> usize {
    if let Some(f) = load_fn_ptr(&INIT_TASK_STACK_FN) {
        let f: fn(&mut [usize], usize) -> usize = unsafe { core::mem::transmute(f) };
        f(stack, entry)
    } else {
        panic!("Arch not initialized");
    }
}

pub fn yield_cpu() {
    if let Some(f) = load_fn_ptr(&YIELD_CPU_FN) {
        let f: fn() = unsafe { core::mem::transmute(f) };
        f();
    } else {
        panic!("Arch not initialized");
    }
}

pub fn set_external_irq_handler(handler: fn()) {
    store_fn_ptr(&EXTERNAL_IRQ_HANDLER, handler as usize);
}

pub fn set_external_irq_id_handler(handler: fn(u32)) {
    store_fn_ptr(&EXTERNAL_IRQ_ID_HANDLER, handler as usize);
}

pub fn external_irq_handler() -> Option<fn()> {
    load_fn_ptr(&EXTERNAL_IRQ_HANDLER).map(|f| unsafe { core::mem::transmute(f) })
}

pub fn external_irq_id_handler() -> Option<fn(u32)> {
    load_fn_ptr(&EXTERNAL_IRQ_ID_HANDLER).map(|f| unsafe { core::mem::transmute(f) })
}

pub fn idle_hint() {
    if let Some(f) = load_fn_ptr(&IDLE_HINT_FN) {
        let f: fn() = unsafe { core::mem::transmute(f) };
        f();
    } else {
        core::hint::spin_loop();
    }
}

/// Return the current CPU's logical id.
pub fn current_cpu_id() -> u32 {
    if let Some(f) = load_fn_ptr(&CURRENT_CPU_ID_FN) {
        let f: fn() -> u32 = unsafe { core::mem::transmute(f) };
        f()
    } else {
        0
    }
}
