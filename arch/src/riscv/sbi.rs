pub fn debug_console_putchar(ch: u8) {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") ch as usize,
            in("a6") 0x2usize,
            in("a7") 0x4442434Eusize,
        );
    }
}
