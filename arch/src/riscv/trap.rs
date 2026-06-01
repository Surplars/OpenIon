use core::arch::global_asm;

#[cfg(target_pointer_width = "64")]
global_asm!(
    r#"
.equ REG_BYTES, 8
.macro REG_S reg, slot, base
    sd \reg, \slot*REG_BYTES(\base)
.endm
.macro REG_L reg, slot, base
    ld \reg, \slot*REG_BYTES(\base)
.endm
"#
);

#[cfg(target_pointer_width = "32")]
global_asm!(
    r#"
.equ REG_BYTES, 4
.macro REG_S reg, slot, base
    sw \reg, \slot*REG_BYTES(\base)
.endm
.macro REG_L reg, slot, base
    lw \reg, \slot*REG_BYTES(\base)
.endm
"#
);

#[cfg(feature = "m-mode")]
global_asm!(
    r#"
.macro SAVE_STATUS_EPC
    csrr t0, mstatus
    REG_S t0, 32, sp
    csrr t1, mepc
    REG_S t1, 33, sp
.endm

.macro LOAD_STATUS_EPC
    REG_L t0, 32, sp
    csrw mstatus, t0
    REG_L t1, 33, sp
    csrw mepc, t1
.endm

.macro RET
    mret
.endm
"#
);

#[cfg(feature = "s-mode")]
global_asm!(
    r#"
.macro SAVE_STATUS_EPC
    csrr t0, sstatus
    REG_S t0, 32, sp
    csrr t1, sepc
    REG_S t1, 33, sp
.endm

.macro LOAD_STATUS_EPC
    REG_L t0, 32, sp
    csrw sstatus, t0
    REG_L t1, 33, sp
    csrw sepc, t1
.endm

.macro RET
    sret
.endm
"#
);

global_asm!(include_str!("trap.S"));

const TRAP_VECTOR_ALIGN: usize = 64;

#[repr(C)]
pub struct TrapFrame {
    pub x: [usize; 32],
    pub status: usize,
    pub epc: usize,
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_trap_handler(tf: &mut TrapFrame) {
    // Hardware disabled irqs. Ensure kernel tracking logic knows we are in a critical section.
    unsafe {
        kernel::arch::ARCH_CRIT_NEST += 1;
    }

    #[cfg(feature = "m-mode")]
    let raw_cause = read_mcause();
    #[cfg(feature = "m-mode")]
    let interrupt_id = raw_cause & !(1usize << (usize::BITS as usize - 1));
    #[cfg(feature = "m-mode")]
    let cause = riscv::register::mcause::read().cause();
    #[cfg(feature = "m-mode")]
    let is_timer = cause
        == riscv::register::mcause::Trap::Interrupt(
            riscv::register::mcause::Interrupt::MachineTimer,
        );
    #[cfg(feature = "m-mode")]
    let is_interrupt = raw_cause & (1usize << (usize::BITS as usize - 1)) != 0;
    #[cfg(feature = "m-mode")]
    let is_external = is_interrupt
        && interrupt_id != 3
        && (cause
            == riscv::register::mcause::Trap::Interrupt(
                riscv::register::mcause::Interrupt::MachineExternal,
            )
            || !is_timer);
    #[cfg(feature = "m-mode")]
    let is_yield = cause
        == riscv::register::mcause::Trap::Exception(riscv::register::mcause::Exception::Breakpoint)
        || is_ebreak_instruction(tf.epc);
    #[cfg(feature = "m-mode")]
    let is_syscall = matches!(
        cause,
        riscv::register::mcause::Trap::Exception(
            riscv::register::mcause::Exception::UserEnvCall
                | riscv::register::mcause::Exception::SupervisorEnvCall
                | riscv::register::mcause::Exception::MachineEnvCall
        )
    );

    #[cfg(feature = "s-mode")]
    let raw_cause = read_scause();
    #[cfg(feature = "s-mode")]
    let interrupt_id = raw_cause & !(1usize << (usize::BITS as usize - 1));
    #[cfg(feature = "s-mode")]
    let is_interrupt = raw_cause & (1usize << (usize::BITS as usize - 1)) != 0;
    #[cfg(feature = "s-mode")]
    let cause = riscv::register::scause::read().cause();
    #[cfg(feature = "s-mode")]
    let is_timer = cause
        == riscv::register::scause::Trap::Interrupt(
            riscv::register::scause::Interrupt::SupervisorTimer,
        );
    #[cfg(feature = "s-mode")]
    let is_external = {
        let is_standard_external = cause
            == riscv::register::scause::Trap::Interrupt(
                riscv::register::scause::Interrupt::SupervisorExternal,
            );
        let has_id_handler = unsafe { kernel::arch::EXTERNAL_IRQ_ID_HANDLER }.is_some();
        let is_clic_interrupt = has_id_handler && is_interrupt && interrupt_id != 1 && !is_timer;
        is_standard_external || is_clic_interrupt
    };
    #[cfg(feature = "s-mode")]
    let is_yield = cause
        == riscv::register::scause::Trap::Exception(riscv::register::scause::Exception::Breakpoint)
        || is_ebreak_instruction(tf.epc);
    #[cfg(feature = "s-mode")]
    let is_syscall = matches!(
        cause,
        riscv::register::scause::Trap::Exception(
            riscv::register::scause::Exception::UserEnvCall
                | riscv::register::scause::Exception::SupervisorEnvCall
        )
    );

    if is_timer {
        kernel::timer::tick();
        kernel::platform::schedule_next_timer_tick();
        kernel::sched::Scheduler::tick_update();
        #[cfg(feature = "async_rt")]
        kernel::sched::async_rt::tick_update();
        kernel::sched::Scheduler::schedule();

        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    if is_yield {
        tf.epc += instruction_len(tf.epc);
        kernel::sched::Scheduler::schedule();

        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    if is_syscall {
        let args = kernel::syscall::SyscallArgs::new(
            tf.x[17],
            [tf.x[10], tf.x[11], tf.x[12], tf.x[13], tf.x[14], tf.x[15]],
        );
        let ret = kernel::syscall::dispatch(args);
        tf.x[10] = ret.value as usize;
        tf.epc += 4;
        if !ret.schedule {
            kernel::sched::Scheduler::schedule_if_preempt_pending();
        }

        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    if is_external {
        if let Some(handler) = unsafe { kernel::arch::EXTERNAL_IRQ_ID_HANDLER } {
            handler(interrupt_id as u32);
        } else if let Some(handler) = unsafe { kernel::arch::EXTERNAL_IRQ_HANDLER } {
            handler();
        }
        kernel::sched::Scheduler::schedule_if_preempt_pending();
        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    #[cfg(feature = "m-mode")]
    let trap_value = riscv::register::mtval::read();
    #[cfg(feature = "s-mode")]
    let trap_value = riscv::register::stval::read();

    panic!(
        "Kernel Trapped: {:?}, epc: {:#x}, tval: {:#x}",
        cause, tf.epc, trap_value
    );
}

#[cfg(feature = "m-mode")]
fn read_mcause() -> usize {
    let value: usize;
    unsafe {
        core::arch::asm!("csrr {}, mcause", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(feature = "s-mode")]
fn read_scause() -> usize {
    let value: usize;
    unsafe {
        core::arch::asm!("csrr {}, scause", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

fn instruction_len(epc: usize) -> usize {
    let halfword = unsafe { (epc as *const u16).read_unaligned() };
    if halfword & 0b11 == 0b11 { 4 } else { 2 }
}

fn is_ebreak_instruction(epc: usize) -> bool {
    let halfword = unsafe { (epc as *const u16).read_unaligned() };
    if halfword == 0x9002 {
        return true;
    }

    if halfword & 0b11 == 0b11 {
        let word = unsafe { (epc as *const u32).read_unaligned() };
        word == 0x0010_0073
    } else {
        false
    }
}

pub fn init() {
    unsafe extern "C" {
        fn trap_vector();
    }

    set_trap_vector(trap_vector as *const () as usize);
}

pub fn set_trap_vector(vector: usize) {
    debug_assert_eq!(vector & (TRAP_VECTOR_ALIGN - 1), 0);

    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mtvec::write(vector, riscv::register::mtvec::TrapMode::Direct);

        #[cfg(feature = "s-mode")]
        riscv::register::stvec::write(vector, riscv::register::stvec::TrapMode::Direct);
    }
}

pub fn set_trap_vector_clic() {
    unsafe extern "C" {
        fn trap_vector();
    }

    set_trap_vector_raw(trap_vector as *const () as usize, 3);
}

fn set_trap_vector_raw(vector: usize, mode: usize) {
    debug_assert_eq!(vector & (TRAP_VECTOR_ALIGN - 1), 0);
    let value = vector | mode;

    unsafe {
        #[cfg(feature = "m-mode")]
        core::arch::asm!("csrw mtvec, {}", in(reg) value, options(nomem, nostack, preserves_flags));

        #[cfg(feature = "s-mode")]
        core::arch::asm!("csrw stvec, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}
