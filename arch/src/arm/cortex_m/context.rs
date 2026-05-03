use core::arch::asm;
use core::arch::global_asm;

pub fn init_task_stack(stack: &mut [usize], entry: usize) -> usize {
    let stack_top = stack.as_mut_ptr() as usize + stack.len() * core::mem::size_of::<usize>();
    let mut sp = stack_top;

    sp &= !0x7;
    sp -= 16 * 4;

    let frame = sp as *mut u32;

    unsafe {
        frame.add(15).write(0x01000000); // xPSR
        frame.add(14).write((entry as u32) | 1); // PC with Thumb bit set
        frame.add(13).write(0xFFFFFFFD); // LR

        frame.add(12).write(0); // r12
        frame.add(11).write(0); // r3
        frame.add(10).write(0); // r2
        frame.add(9).write(0); // r1
        frame.add(8).write(0); // r0

        for i in 0..8 {
            frame.add(i).write(0); // r4-r11
        }
    }

    sp
}

pub fn yield_cpu() {
    unsafe {
        core::ptr::write_volatile(0xE000_ED04 as *mut u32, 1 << 28);
    }
}

pub fn start_first_task() -> ! {
    unsafe {
        asm!(
            "
            // Get NEXT_TCB
            ldr r1, [r3]
            
            // CURRENT_TCB = NEXT_TCB
            str r1, [r2]
            
            cmp r1, #0
            beq 2f

            ldr r0, [r1] // r0 = sp
            
            // Pop r4-r11 (SW saved state)
            ldmia r0!, {{r4-r11}}
            
            // r0 now points to HW frame [R0, R1, R2, R3, R12, LR, PC, xPSR] (32 bytes)
            // Advance PSP to the end of the hardware frame
            adds r1, r0, #32
            msr psp, r1
            
            // Switch to using PSP
            movs r1, #2
            msr control, r1
            isb
            
            // Load registers manually from the HW frame
            ldr r1, [r0, #4]
            ldr r2, [r0, #8]
            ldr r3, [r0, #12]
            ldr r4, [r0, #16]
            mov r12, r4
            ldr r4, [r0, #20]
            mov lr, r4
            ldr r5, [r0, #24] // PC
            
            // Load R0 last since we are using it as our base pointer
            ldr r0, [r0, #0]
            
            // Enable interrupts
            cpsie i
            
            // Jump to the task entry point
            bx r5

            2:
            b 2b
            ",
            in("r2") core::ptr::addr_of!(kernel::sched::CURRENT_TCB),
            in("r3") core::ptr::addr_of!(kernel::sched::NEXT_TCB),
            options(noreturn)
        )
    }
}

global_asm!(
    "
    .syntax unified
    .global pendsv_handler
    .type pendsv_handler, %function
    .thumb_func
pendsv_handler:
    mrs r0, psp

    ldr r3, =NEXT_TCB
    ldr r1, [r3]
    cmp r1, #0
    beq 2f

    stmdb r0!, {{r4-r11}}

    ldr r2, =CURRENT_TCB
    ldr r1, [r2]

    cmp r1, #0
    beq 1f

    // Save sp into CURRENT_TCB
    str r0, [r1]

1:
    ldr r3, =NEXT_TCB
    ldr r1, [r3]

    // CURRENT_TCB = NEXT_TCB
    str r1, [r2]

    ldr r0, [r1]

    ldmia r0!, {{r4-r11}}

    msr psp, r0

    bx lr

2:
    bx lr

    .pool
    "
);
