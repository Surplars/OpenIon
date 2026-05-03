use core::arch::global_asm;

pub fn init_task_stack(stack: &mut [usize], entry: usize) -> usize {
    let stack_top = stack.as_mut_ptr() as usize + stack.len() * core::mem::size_of::<usize>();
    let mut sp = stack_top;

    // 16-byte alignment as per RISC-V ABI
    sp &= !0xF;

    // Allocate space for TrapFrame: 34 words
    sp -= 34 * core::mem::size_of::<usize>();

    let frame = sp as *mut usize;

    unsafe {
        // Just fill with 0s for all registers
        for i in 0..34 {
            frame.add(i).write(0);
        }

        // frame[0..31] are x0-x31
        // Default mstatus/sstatus: enable external interrupt tracking, and set SPP/MPP appropriately?
        // Wait, for bare-metal M/S-mode, MPIE/SPIE might be set so that RET enables interrupts!

        #[cfg(feature = "m-mode")]
        let init_status = 0x1880; // MPP=3 (M-mode), MPIE=1

        #[cfg(feature = "s-mode")]
        let init_status = 0x120; // SPP=1 (S-mode), SPIE=1

        // status at offset 32
        frame.add(32).write(init_status);
        // epc at offset 33 (Entry Point!)
        frame.add(33).write(entry);
    }

    sp
}

pub fn yield_cpu() {
    // Cause a software interrupt or ECALL to yield.
    unsafe {
        core::arch::asm!("ebreak");
    }
}

pub fn start_first_task() -> ! {
    unsafe extern "C" {
        fn load_first_task_and_start() -> !;
    }
    unsafe {
        load_first_task_and_start();
    }
}

global_asm!(
    r#"
.section .text
.global load_first_task_and_start
load_first_task_and_start:
    la t0, NEXT_TCB
    ld t1, 0(t0)        # t1 = &NEXT_TCB
    
    la t2, CURRENT_TCB
    sd t1, 0(t2)        # CURRENT_TCB = NEXT_TCB
    
    ld sp, 0(t1)        # sp = NEXT_TCB->sp

    # Goto trap_exit to restore context and start task
    j trap_exit
    "#
);
