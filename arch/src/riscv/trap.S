.section .text.trap
.balign 64
.global trap_vector

trap_vector:
    addi sp, sp, -34*REG_BYTES

    REG_S x1, 1, sp
    # x2 is sp.
    REG_S x3, 3, sp
    REG_S x4, 4, sp
    REG_S x5, 5, sp
    REG_S x6, 6, sp
    REG_S x7, 7, sp
    REG_S x8, 8, sp
    REG_S x9, 9, sp
    REG_S x10, 10, sp
    REG_S x11, 11, sp
    REG_S x12, 12, sp
    REG_S x13, 13, sp
    REG_S x14, 14, sp
    REG_S x15, 15, sp
    REG_S x16, 16, sp
    REG_S x17, 17, sp
    REG_S x18, 18, sp
    REG_S x19, 19, sp
    REG_S x20, 20, sp
    REG_S x21, 21, sp
    REG_S x22, 22, sp
    REG_S x23, 23, sp
    REG_S x24, 24, sp
    REG_S x25, 25, sp
    REG_S x26, 26, sp
    REG_S x27, 27, sp
    REG_S x28, 28, sp
    REG_S x29, 29, sp
    REG_S x30, 30, sp
    REG_S x31, 31, sp

    SAVE_STATUS_EPC

    mv a0, sp
    call rust_trap_handler

    la t0, CURRENT_TCB
    REG_L t1, 0, t0
    la t2, NEXT_TCB
    REG_L t3, 0, t2

    beq t1, t3, 1f

    REG_S sp, 0, t1
    REG_S t3, 0, t0
    REG_L sp, 0, t3

1:
    .global trap_exit
trap_exit:
    LOAD_STATUS_EPC

    REG_L x1, 1, sp
    # x2 is sp.
    REG_L x3, 3, sp
    REG_L x4, 4, sp
    REG_L x5, 5, sp
    REG_L x6, 6, sp
    REG_L x7, 7, sp
    REG_L x8, 8, sp
    REG_L x9, 9, sp
    REG_L x10, 10, sp
    REG_L x11, 11, sp
    REG_L x12, 12, sp
    REG_L x13, 13, sp
    REG_L x14, 14, sp
    REG_L x15, 15, sp
    REG_L x16, 16, sp
    REG_L x17, 17, sp
    REG_L x18, 18, sp
    REG_L x19, 19, sp
    REG_L x20, 20, sp
    REG_L x21, 21, sp
    REG_L x22, 22, sp
    REG_L x23, 23, sp
    REG_L x24, 24, sp
    REG_L x25, 25, sp
    REG_L x26, 26, sp
    REG_L x27, 27, sp
    REG_L x28, 28, sp
    REG_L x29, 29, sp
    REG_L x30, 30, sp
    REG_L x31, 31, sp

    addi sp, sp, 34*REG_BYTES
    RET
