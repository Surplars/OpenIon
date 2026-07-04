/// CLIC platform configuration layer.
///
/// This module handles platform-specific CLIC initialization and IRQ dispatch.
/// The low-level register access is provided by `arch::riscv::clic::Clic`.
use arch::riscv::clic::Clic;
use core::sync::atomic::{AtomicUsize, Ordering};

static CLIC_BASE: AtomicUsize = AtomicUsize::new(0);
static IRQ_COUNT: AtomicUsize = AtomicUsize::new(0);

const CLIC_BASE_ALIGN: usize = 4;

pub fn configure(base: usize, irq_count: usize) {
    if base != 0 && base % CLIC_BASE_ALIGN != 0 {
        kernel::kwarn!(
            "CLIC: ignoring unaligned base {:#x}, expected {} byte alignment",
            base,
            CLIC_BASE_ALIGN
        );
        CLIC_BASE.store(0, Ordering::Release);
        IRQ_COUNT.store(0, Ordering::Release);
        return;
    }

    CLIC_BASE.store(base, Ordering::Release);
    IRQ_COUNT.store(irq_count, Ordering::Release);
}

fn clic() -> Option<Clic> {
    let base = CLIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        None
    } else {
        Some(Clic::new(base))
    }
}

pub fn is_configured() -> bool {
    CLIC_BASE.load(Ordering::Acquire) != 0
}

pub fn init() {
    if clic().is_none() {
        kernel::kwarn!("CLIC: no interrupt controller found in DTB");
        return;
    }

    #[cfg(feature = "s-mode")]
    {
        kernel::kinfo!("CLIC: using firmware-configured S-mode interrupt controller");
        return;
    }

    #[cfg(feature = "m-mode")]
    {
        let clic = clic().unwrap();
        // nlbits=0 keeps all enabled sources at one privilege level and direct trap entry.
        clic.set_config(0);

        let irq_count = IRQ_COUNT.load(Ordering::Acquire).min(u32::MAX as usize);
        for irq in 1..irq_count {
            clic.init_irq(irq as u32);
        }
    }
}

pub fn handle_irq(irq: u32) {
    if irq == 0 {
        return;
    }

    kernel::irq::handle_irq(irq as usize);

    #[cfg(feature = "s-mode")]
    {
        return;
    }

    #[cfg(feature = "m-mode")]
    {
        if let Some(clic) = clic() {
            clic.clear_pending(irq);
        }
    }
}
