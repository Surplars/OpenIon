use core::sync::atomic::{AtomicUsize, Ordering};

static PLIC_BASE: AtomicUsize = AtomicUsize::new(0);

fn context_id() -> usize {
    #[cfg(feature = "m-mode")]
    {
        0
    }

    #[cfg(feature = "s-mode")]
    {
        1
    }
}

fn base() -> usize {
    PLIC_BASE.load(Ordering::Relaxed)
}

fn controller() -> arch::riscv::plic::Plic {
    arch::riscv::plic::Plic::new(base())
}

pub fn set_base(base_addr: usize) {
    PLIC_BASE.store(base_addr, Ordering::Relaxed);
}

pub fn is_ready() -> bool {
    base() != 0
}

pub fn init() {
    controller().init_context(context_id());
}

pub fn enable_irq(irq: u32, priority: u32) {
    controller().enable_irq(context_id(), irq, priority);
}

pub fn claim() -> u32 {
    controller().claim(context_id())
}

pub fn complete(irq: u32) {
    controller().complete(context_id(), irq);
}
