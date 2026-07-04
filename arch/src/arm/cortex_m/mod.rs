pub mod context;
pub mod irq;
pub mod nvic;
pub mod systick;

pub struct CortexM;

impl kernel::arch::Arch for CortexM {
    fn enable_global_irq() {
        unsafe {
            cortex_m::interrupt::enable();
        }
    }

    fn disable_global_irq() {
        cortex_m::interrupt::disable();
    }

    fn init_task_stack(stack: &mut [usize], entry: usize) -> usize {
        context::init_task_stack(stack, entry)
    }

    fn yield_cpu() {
        context::yield_cpu();
    }

    fn idle_hint() {
        cortex_m::asm::wfi();
    }

    fn start_first_task() -> ! {
        context::start_first_task();
    }

    fn current_cpu_id() -> u32 {
        // Cortex-M single-core, always return 0
        0
    }
}
