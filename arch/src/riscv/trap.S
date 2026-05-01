.section .text.trap
.balign 4
.global trap_vector

trap_vector:
    # 栈指针减去 34 个 usize (272 字节)
    addi sp, sp, -34*8
    
    # 保存所有的通用寄存器 x1~x31，x0 恒为 0，跳过
    sd x1, 1*8(sp)
    # 不保存 x2 (sp)，因为它是栈指针
    sd x3, 3*8(sp)
    sd x4, 4*8(sp)
    sd x5, 5*8(sp)
    sd x6, 6*8(sp)
    sd x7, 7*8(sp)
    sd x8, 8*8(sp)
    sd x9, 9*8(sp)
    sd x10, 10*8(sp)
    sd x11, 11*8(sp)
    sd x12, 12*8(sp)
    sd x13, 13*8(sp)
    sd x14, 14*8(sp)
    sd x15, 15*8(sp)
    sd x16, 16*8(sp)
    sd x17, 17*8(sp)
    sd x18, 18*8(sp)
    sd x19, 19*8(sp)
    sd x20, 20*8(sp)
    sd x21, 21*8(sp)
    sd x22, 22*8(sp)
    sd x23, 23*8(sp)
    sd x24, 24*8(sp)
    sd x25, 25*8(sp)
    sd x26, 26*8(sp)
    sd x27, 27*8(sp)
    sd x28, 28*8(sp)
    sd x29, 29*8(sp)
    sd x30, 30*8(sp)
    sd x31, 31*8(sp)

    # 调用宏（来自于 rust 的 global_asm!），保存状态和断点地址 (mstatus/sstatus 和 mepc/sepc)
    SAVE_STATUS_EPC

    # 把 TrapFrame 的指针传给 rust_trap_handler 的第一个参数 tf (a0 = x10)
    mv a0, sp
    call rust_trap_handler

    # Context Switch 逻辑开始
    # a0 中存放的还是我们在 trap 进入时设置的 trap frame 指针
    la t0, CURRENT_TCB
    ld t1, 0(t0)        # t1 = 当前 TCB 的地址 (&CURRENT_TCB)
    la t2, NEXT_TCB
    ld t3, 0(t2)        # t3 = 下一个要调用的 TCB 的地址 (&NEXT_TCB)

    # 比较 t1 和 t3，若相同，说明没有发生上下文切换
    beq t1, t3, 1f

    # 如果不同，说明发生了上下文切换
    sd sp, 0(t1)        # CURRENT_TCB->sp = sp 
    sd t3, 0(t0)        # CURRENT_TCB = NEXT_TCB
    ld sp, 0(t3)        # sp = NEXT_TCB->sp

1:
    .global trap_exit
trap_exit:
    # 加载状态和断点地址返回 (mstatus/sstatus 和 mepc/sepc)
    LOAD_STATUS_EPC

    # 恢复所有的通用寄存器
    ld x1, 1*8(sp)
    # 不恢复 x2 (sp)，它是栈指针
    ld x3, 3*8(sp)
    ld x4, 4*8(sp)
    ld x5, 5*8(sp)
    ld x6, 6*8(sp)
    ld x7, 7*8(sp)
    ld x8, 8*8(sp)
    ld x9, 9*8(sp)
    ld x10, 10*8(sp)
    ld x11, 11*8(sp)
    ld x12, 12*8(sp)
    ld x13, 13*8(sp)
    ld x14, 14*8(sp)
    ld x15, 15*8(sp)
    ld x16, 16*8(sp)
    ld x17, 17*8(sp)
    ld x18, 18*8(sp)
    ld x19, 19*8(sp)
    ld x20, 20*8(sp)
    ld x21, 21*8(sp)
    ld x22, 22*8(sp)
    ld x23, 23*8(sp)
    ld x24, 24*8(sp)
    ld x25, 25*8(sp)
    ld x26, 26*8(sp)
    ld x27, 27*8(sp)
    ld x28, 28*8(sp)
    ld x29, 29*8(sp)
    ld x30, 30*8(sp)
    ld x31, 31*8(sp)

    addi sp, sp, 34*8
    
    # 根据 feature 特性 mret 或者 sret
    RET
