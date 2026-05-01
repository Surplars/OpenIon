pub trait Arch {
    fn enable_global_irq();
    fn disable_global_irq();

    fn init_task_stack(stack: &mut [usize], entry: usize) -> usize;

    fn yield_cpu();

    fn start_first_task() -> !;
}

pub static mut DISABLE_IRQ_FN: Option<fn()> = None;
pub static mut ENABLE_IRQ_FN: Option<fn()> = None;
pub static mut INIT_TASK_STACK_FN: Option<fn(&mut [usize], usize) -> usize> = None;
pub static mut YIELD_CPU_FN: Option<fn()> = None;
pub static mut EXTERNAL_IRQ_HANDLER: Option<fn()> = None;

pub fn init<A: Arch>() {
    unsafe {
        DISABLE_IRQ_FN = Some(A::disable_global_irq);
        ENABLE_IRQ_FN = Some(A::enable_global_irq);
        INIT_TASK_STACK_FN = Some(A::init_task_stack);
        YIELD_CPU_FN = Some(A::yield_cpu);
    }
}

pub static mut ARCH_CRIT_NEST: usize = 0;

pub fn disable_irq() {
    unsafe {
        if ARCH_CRIT_NEST == 0 {
            if let Some(f) = DISABLE_IRQ_FN {
                f()
            }
        }
        ARCH_CRIT_NEST += 1;
    }
}

pub fn enable_irq() {
    unsafe {
        if ARCH_CRIT_NEST > 0 {
            ARCH_CRIT_NEST -= 1;
        }
        if ARCH_CRIT_NEST == 0 && !crate::sched::CURRENT_TCB.is_null() {
            if let Some(f) = ENABLE_IRQ_FN {
                f()
            }
        }
    }
}
