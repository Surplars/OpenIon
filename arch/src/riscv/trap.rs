use core::arch::global_asm;

// M-mode 宏定义
#[cfg(feature = "m-mode")]
global_asm!(
    r#"
.macro SAVE_STATUS_EPC
    csrr t0, mstatus
    sd t0, 32*8(sp)
    csrr t1, mepc
    sd t1, 33*8(sp)
.endm

.macro LOAD_STATUS_EPC
    ld t0, 32*8(sp)
    csrw mstatus, t0
    ld t1, 33*8(sp)
    csrw mepc, t1
.endm

.macro RET
    mret
.endm
"#
);

// S-mode 宏定义
#[cfg(feature = "s-mode")]
global_asm!(
    r#"
.macro SAVE_STATUS_EPC
    csrr t0, sstatus
    sd t0, 32*8(sp)
    csrr t1, sepc
    sd t1, 33*8(sp)
.endm

.macro LOAD_STATUS_EPC
    ld t0, 32*8(sp)
    csrw sstatus, t0
    ld t1, 33*8(sp)
    csrw sepc, t1
.endm

.macro RET
    sret
.endm
"#
);

// 核心的陷阱进入与退出机制汇编
global_asm!(include_str!("trap.S"));

#[repr(C)]
pub struct TrapFrame {
    pub x: [usize; 32], // x[0] 为 x0 (始终为 0), x[1] 为 ra, ... x[31] 为 t6
    pub status: usize,  // mstatus / sstatus
    pub epc: usize,     // mepc / sepc
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_trap_handler(tf: &mut TrapFrame) {
    // Hardware disabled irqs. Ensure kernel tracking logic knows we are in a critical section.
    unsafe {
        kernel::arch::ARCH_CRIT_NEST += 1;
    }

    #[cfg(feature = "m-mode")]
    let cause = riscv::register::mcause::read().cause();
    #[cfg(feature = "m-mode")]
    let is_timer = cause
        == riscv::register::mcause::Trap::Interrupt(
            riscv::register::mcause::Interrupt::MachineTimer,
        );
    #[cfg(feature = "m-mode")]
    let is_external = cause
        == riscv::register::mcause::Trap::Interrupt(
            riscv::register::mcause::Interrupt::MachineExternal,
        );
    #[cfg(feature = "m-mode")]
    let is_yield = cause
        == riscv::register::mcause::Trap::Exception(riscv::register::mcause::Exception::Breakpoint);

    #[cfg(feature = "s-mode")]
    let cause = riscv::register::scause::read().cause();
    #[cfg(feature = "s-mode")]
    let is_timer = cause
        == riscv::register::scause::Trap::Interrupt(
            riscv::register::scause::Interrupt::SupervisorTimer,
        );
    #[cfg(feature = "s-mode")]
    let is_external = cause
        == riscv::register::scause::Trap::Interrupt(
            riscv::register::scause::Interrupt::SupervisorExternal,
        );
    #[cfg(feature = "s-mode")]
    let is_yield = cause
        == riscv::register::scause::Trap::Exception(riscv::register::scause::Exception::Breakpoint);

    if is_timer {
        kernel::timer::tick();
        kernel::platform::schedule_next_timer_tick();
        kernel::sched::Scheduler::tick_update();
        kernel::sched::Scheduler::schedule();

        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    if is_yield {
        tf.epc += 4;
        kernel::sched::Scheduler::schedule();

        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    if is_external {
        if let Some(handler) = unsafe { kernel::arch::EXTERNAL_IRQ_HANDLER } {
            handler();
        }
        kernel::sched::Scheduler::schedule_if_preempt_pending();
        unsafe {
            kernel::arch::ARCH_CRIT_NEST -= 1;
        }
        return;
    }

    let stval = riscv::register::stval::read();
    panic!(
        "Kernel Trapped: {:?}, sepc: {:#x}, stval: {:#x}",
        cause, tf.epc, stval
    );
}

pub fn init() {
    unsafe extern "C" {
        fn trap_vector();
    }

    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mtvec::write(
            trap_vector as *const () as usize,
            riscv::register::mtvec::TrapMode::Direct,
        );

        #[cfg(feature = "s-mode")]
        riscv::register::stvec::write(
            trap_vector as *const () as usize,
            riscv::register::stvec::TrapMode::Direct,
        );
    }
}

pub fn set_trap_vector(vector: usize) {
    unsafe {
        #[cfg(feature = "m-mode")]
        riscv::register::mtvec::write(vector, riscv::register::mtvec::TrapMode::Direct);

        #[cfg(feature = "s-mode")]
        riscv::register::stvec::write(vector, riscv::register::stvec::TrapMode::Direct);
    }
}
