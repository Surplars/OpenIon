use core::sync::atomic::{AtomicUsize, Ordering};

static CLINT_BASE: AtomicUsize = AtomicUsize::new(0);

fn base() -> usize {
    CLINT_BASE.load(Ordering::Relaxed)
}

fn controller() -> arch::riscv::clint::Clint {
    arch::riscv::clint::Clint::new(base())
}

pub fn set_base(base_addr: usize) {
    CLINT_BASE.store(base_addr, Ordering::Relaxed);
}

pub fn is_ready() -> bool {
    base() != 0
}

pub fn init_timer() {
    if !is_ready() {
        return;
    }

    kernel::platform::set_next_timer_tick(set_next_tick);
    set_next_tick();
    arch::riscv::timer::enable_timer_interrupts();
}

pub fn set_next_tick() {
    let clint = controller();
    if !clint.is_valid() {
        return;
    }

    let cfg = kernel::platform::get_config();
    let increment = (cfg.cpu_freq_hz / cfg.systick_hz) as u64;
    let deadline = clint.mtime() + increment;

    #[cfg(feature = "m-mode")]
    {
        clint.set_mtimecmp(0, deadline);
    }

    #[cfg(feature = "s-mode")]
    arch::riscv::timer::set_sbi_timer(deadline);
}
