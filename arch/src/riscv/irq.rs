pub fn init() {
    // RISC-V specific IRQ initializations via PLIC
}

pub fn enable_external_interrupts() {
    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mie::set_mext();

        #[cfg(feature = "s-mode")]
        riscv::register::sie::set_sext();
    }
}
