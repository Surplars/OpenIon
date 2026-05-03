pub mod context;
pub mod hypervisor;
pub mod irq;
pub mod pmp;
pub mod sbi;
pub mod sv39;
pub mod timer;
pub mod trap;

pub struct RiscvArch;

impl kernel::arch::Arch for RiscvArch {
    fn enable_global_irq() {
        unsafe {
            #[cfg(feature = "m-mode")]
            riscv::register::mstatus::set_mie();

            #[cfg(feature = "s-mode")]
            riscv::register::sstatus::set_sie();
        }
    }

    fn disable_global_irq() {
        unsafe {
            #[cfg(feature = "m-mode")]
            riscv::register::mstatus::clear_mie();

            #[cfg(feature = "s-mode")]
            riscv::register::sstatus::clear_sie();
        }
    }

    fn init_task_stack(stack: &mut [usize], entry: usize) -> usize {
        context::init_task_stack(stack, entry)
    }

    fn yield_cpu() {
        context::yield_cpu();
    }

    fn start_first_task() -> ! {
        trap::init(); // Initialize trap handler vector before starting first task
        context::start_first_task();
    }
}
