use core::sync::atomic::{AtomicUsize, Ordering};

static PLIC_BASE: AtomicUsize = AtomicUsize::new(0);
static IRQ_COUNT: AtomicUsize = AtomicUsize::new(0);

const PLIC_BASE_ALIGN: usize = 0x1000;

pub fn configure(base: usize, irq_count: usize) {
    if base != 0 && base % PLIC_BASE_ALIGN != 0 {
        kernel::kwarn!(
            "PLIC: ignoring unaligned base {:#x}, expected {} byte alignment",
            base,
            PLIC_BASE_ALIGN
        );
        PLIC_BASE.store(0, Ordering::Release);
        IRQ_COUNT.store(0, Ordering::Release);
        return;
    }

    PLIC_BASE.store(base, Ordering::Release);
    IRQ_COUNT.store(irq_count, Ordering::Release);
}

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

fn controller() -> Option<arch::riscv::plic::Plic> {
    let base = PLIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        None
    } else {
        Some(arch::riscv::plic::Plic::new(base))
    }
}

pub fn is_configured() -> bool {
    PLIC_BASE.load(Ordering::Acquire) != 0
}

pub fn init() {
    let Some(plic) = controller() else {
        kernel::kwarn!("PLIC: no interrupt controller found in DTB");
        return;
    };

    plic.init_context(context_id());

    // Enable IRQs for all sources discovered from DTB.
    // Drivers should call enable_irq() for their specific IRQs instead.
    let irq_count = IRQ_COUNT.load(Ordering::Acquire);
    for irq in 1..=irq_count.min(u32::MAX as usize) {
        plic.enable_irq(context_id(), irq as u32, 1);
    }
}

pub fn enable_irq(irq: u32, priority: u32) {
    if let Some(plic) = controller() {
        plic.enable_irq(context_id(), irq, priority);
    }
}

pub fn disable_irq(irq: u32) {
    if let Some(plic) = controller() {
        plic.enable_irq(context_id(), irq, 0);
    }
}

pub fn claim() -> u32 {
    controller()
        .map(|plic| plic.claim(context_id()))
        .unwrap_or(0)
}

pub fn complete(irq: u32) {
    if let Some(plic) = controller() {
        plic.complete(context_id(), irq);
    }
}
