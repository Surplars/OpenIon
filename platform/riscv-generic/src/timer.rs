use core::sync::atomic::{AtomicUsize, Ordering};

static CLINT_BASE: AtomicUsize = AtomicUsize::new(0);

pub fn configure_clint(base: usize) {
    CLINT_BASE.store(base, Ordering::Release);
}

pub fn init_timer(clic_configured: bool) {
    kernel::platform::set_next_timer_tick(set_next_tick);
    set_next_tick();

    #[cfg(feature = "s-mode")]
    if clic_configured {
        kernel::kwarn!("CLIC: skipping standard S-mode timer interrupt enable");
        return;
    }

    let _ = clic_configured;
    arch::riscv::timer::enable_timer_interrupts();
}

pub fn set_next_tick() {
    let cfg = kernel::platform::get_config();
    let increment = (cfg.cpu_freq_hz / cfg.systick_hz) as u64;
    let deadline = arch::riscv::timer::read_time().wrapping_add(increment);

    #[cfg(feature = "m-mode")]
    {
        let clint_base = CLINT_BASE.load(Ordering::Acquire);
        if clint_base != 0 {
            let clint = arch::riscv::clint::Clint::new(clint_base);
            clint.set_mtimecmp(0, deadline);
        }
    }

    #[cfg(feature = "s-mode")]
    arch::riscv::timer::set_sbi_timer(deadline);
}
