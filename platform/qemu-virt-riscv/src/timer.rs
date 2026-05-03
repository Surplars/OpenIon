use crate::QemuVirtRiscv;
use kernel::platform::Platform;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME: usize = CLINT_BASE + 0xBFF8;

#[cfg(feature = "m-mode")]
const CLINT_MTIMECMP: usize = CLINT_BASE + 0x4000;

pub fn init_timer() {
    kernel::platform::set_next_timer_tick(set_next_tick);
    set_next_tick();
    arch::riscv::timer::enable_timer_interrupts();
}

pub fn set_next_tick() {
    let cfg = QemuVirtRiscv::config();
    let increment = (cfg.cpu_freq_hz / cfg.systick_hz) as u64;
    let deadline = unsafe { (CLINT_MTIME as *const u64).read_volatile() } + increment;

    #[cfg(feature = "m-mode")]
    unsafe {
        (CLINT_MTIMECMP as *mut u64).write_volatile(deadline);
    }

    #[cfg(feature = "s-mode")]
    arch::riscv::timer::set_sbi_timer(deadline);
}
