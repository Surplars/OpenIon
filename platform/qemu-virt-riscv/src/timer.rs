use crate::QemuVirtRiscv;
use kernel::platform::Platform;

const CLINT_BASE: usize = 0x0200_0000;

pub fn init_timer() {
    kernel::platform::set_next_timer_tick(set_next_tick);
    set_next_tick();
    arch::riscv::timer::enable_timer_interrupts();
}

pub fn set_next_tick() {
    let cfg = QemuVirtRiscv::config();
    let increment = (cfg.cpu_freq_hz / cfg.systick_hz) as u64;
    let clint = arch::riscv::clint::Clint::new(CLINT_BASE);
    let deadline = clint.mtime() + increment;

    #[cfg(feature = "m-mode")]
    {
        clint.set_mtimecmp(0, deadline);
    }

    #[cfg(feature = "s-mode")]
    arch::riscv::timer::set_sbi_timer(deadline);
}
