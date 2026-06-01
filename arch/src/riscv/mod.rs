pub mod clint;
pub mod context;
#[cfg(feature = "hypervisor")]
pub mod hypervisor;
pub mod irq;
pub mod plic;
pub mod pmp;
pub mod sbi;
#[cfg(target_arch = "riscv32")]
pub mod sv32;
#[cfg(target_arch = "riscv64")]
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

    fn idle_hint() {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
    }

    fn start_first_task() -> ! {
        trap::init(); // Initialize trap handler vector before starting first task
        context::start_first_task();
    }
}
