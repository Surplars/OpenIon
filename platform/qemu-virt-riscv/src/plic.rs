const PLIC_BASE: usize = 0x0c00_0000;

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

pub fn init() {
    arch::riscv::plic::Plic::new(PLIC_BASE).init_context(context_id());
}

pub fn enable_irq(irq: u32, priority: u32) {
    arch::riscv::plic::Plic::new(PLIC_BASE).enable_irq(context_id(), irq, priority);
}

pub fn claim() -> u32 {
    arch::riscv::plic::Plic::new(PLIC_BASE).claim(context_id())
}

pub fn complete(irq: u32) {
    arch::riscv::plic::Plic::new(PLIC_BASE).complete(context_id(), irq);
}
