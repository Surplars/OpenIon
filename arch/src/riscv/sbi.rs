/// Host SBI calls made by OpenIon when running in RISC-V S-mode.
///
/// `sbi_rt` is suitable for host-to-firmware calls such as timer setup. Guest
/// SBI calls are different: the hypervisor must emulate or forward them from
/// `arch::riscv::hypervisor::sbi` instead of exposing host firmware directly.

const SBI_EXT_DBCN: usize = 0x4442_434E;
const SBI_DBCN_CONSOLE_WRITE_BYTE: usize = 0x2;
const SBI_EXT_HSM: usize = 0x4853_4D;
const SBI_HSM_HART_START: usize = 0x0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbiRet {
    pub error: isize,
    pub value: usize,
}

#[inline]
fn sbi_call(extension: usize, function: usize, args: [usize; 6]) -> SbiRet {
    let error: isize;
    let value: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") args[0] => error,
            inlateout("a1") args[1] => value,
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
            in("a5") args[5],
            in("a6") function,
            in("a7") extension,
        );
    }
    SbiRet { error, value }
}

pub fn debug_console_putchar(ch: u8) {
    console_putchar(ch);
}

pub fn console_putchar(ch: u8) {
    let _ = sbi_call(
        SBI_EXT_DBCN,
        SBI_DBCN_CONSOLE_WRITE_BYTE,
        [ch as usize, 0, 0, 0, 0, 0],
    );
}

/// Start a stopped hart through SBI HSM.
pub fn hart_start(hartid: usize, start_addr: usize, opaque: usize) -> SbiRet {
    sbi_call(
        SBI_EXT_HSM,
        SBI_HSM_HART_START,
        [hartid, start_addr, opaque, 0, 0, 0],
    )
}
