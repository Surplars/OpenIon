pub fn enable_timer_interrupts() {
    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mie::set_mtimer();

        #[cfg(feature = "s-mode")]
        riscv::register::sie::set_stimer();
    }
}

#[cfg(feature = "s-mode")]
pub fn set_sbi_timer(deadline: u64) {
    sbi_rt::set_timer(deadline);
}
