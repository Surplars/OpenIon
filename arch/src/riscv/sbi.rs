/// Host SBI calls made by OpenIon when running in RISC-V S-mode.
///
/// `sbi_rt` is suitable for host-to-firmware calls such as timer setup. Guest
/// SBI calls are different: the hypervisor must emulate or forward them from
/// `arch::riscv::hypervisor::sbi` instead of exposing host firmware directly.

const SBI_EXT_DBCN: usize = 0x4442_434E;
const SBI_DBCN_CONSOLE_WRITE_BYTE: usize = 0x2;

pub fn debug_console_putchar(ch: u8) {
    console_putchar(ch);
}

pub fn console_putchar(ch: u8) {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") ch as usize,
            in("a6") SBI_DBCN_CONSOLE_WRITE_BYTE,
            in("a7") SBI_EXT_DBCN,
        );
    }
}
