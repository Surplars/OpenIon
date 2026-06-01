use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "m-mode")]
const CLICCFG_OFFSET: usize = 0x0000;
#[cfg(feature = "m-mode")]
const CLICINT_BASE: usize = 0x1000;
#[cfg(feature = "m-mode")]
const CLICINT_STRIDE: usize = 4;
#[cfg(feature = "m-mode")]
const CLICINTIP: usize = 0;
#[cfg(feature = "m-mode")]
const CLICINTIE: usize = 1;
#[cfg(feature = "m-mode")]
const CLICINTATTR: usize = 2;
#[cfg(feature = "m-mode")]
const CLICINTCTL: usize = 3;

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

pub fn is_configured() -> bool {
    CLIC_BASE.load(Ordering::Acquire) != 0
}

pub fn init() {
    let base = CLIC_BASE.load(Ordering::Acquire);
    if base == 0 {
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
        // nlbits=0 keeps all enabled sources at one privilege level and direct trap entry.
        write_u8(base + CLICCFG_OFFSET, 0);

        let irq_count = IRQ_COUNT.load(Ordering::Acquire).min(u32::MAX as usize);
        for irq in 1..irq_count {
            let reg = base + CLICINT_BASE + irq * CLICINT_STRIDE;
            write_u8(reg + CLICINTATTR, 0);
            write_u8(reg + CLICINTCTL, 0xff);
            write_u8(reg + CLICINTIE, 1);
        }
    }
}

pub fn handle_irq(irq: u32) {
    if irq == 0 {
        return;
    }

    let _ = kernel::driver::manager::DriverManager::dispatch_irq(irq);

    #[cfg(feature = "s-mode")]
    {
        return;
    }

    #[cfg(feature = "m-mode")]
    {
        let base = CLIC_BASE.load(Ordering::Acquire);
        if base != 0 {
            let pending = base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTIP;
            write_u8(pending, 0);
        }
    }
}

#[cfg(feature = "m-mode")]
fn write_u8(addr: usize, value: u8) {
    unsafe {
        (addr as *mut u8).write_volatile(value);
    }
}
