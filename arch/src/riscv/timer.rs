pub fn enable_timer_interrupts() {
    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mie::set_mtimer();

        #[cfg(feature = "s-mode")]
        riscv::register::sie::set_stimer();
    }
}

pub fn read_time() -> u64 {
    #[cfg(target_pointer_width = "64")]
    {
        let value: usize;
        unsafe {
            core::arch::asm!("rdtime {}", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        value as u64
    }

    #[cfg(target_pointer_width = "32")]
    {
        loop {
            let hi0: u32;
            let lo: u32;
            let hi1: u32;
            unsafe {
                core::arch::asm!("rdtimeh {}", out(reg) hi0, options(nomem, nostack, preserves_flags));
                core::arch::asm!("rdtime {}", out(reg) lo, options(nomem, nostack, preserves_flags));
                core::arch::asm!("rdtimeh {}", out(reg) hi1, options(nomem, nostack, preserves_flags));
            }
            if hi0 == hi1 {
                return ((hi0 as u64) << 32) | lo as u64;
            }
        }
    }
}

#[cfg(feature = "s-mode")]
pub fn set_sbi_timer(deadline: u64) {
    sbi_rt::set_timer(deadline);
}
