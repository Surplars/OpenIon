    .section .text.entry
    .globl _start
    .globl secondary_entry

_start:
    /* OpenSBI starts with a0 = hartid, a1 = DTB physical address. */
    bnez a0, .L_sleep

    /* Save DTB address before setting up the stack or clearing BSS. */
    la t0, _dtb_addr_saved
    sw a1, 0(t0)

    mv tp, a0
    la sp, boot_stack_top

    call rust_main

secondary_entry:
    mv tp, a0
    la sp, secondary_boot_stack_top
    andi t0, a0, 15
    slli t0, t0, 12
    sub sp, sp, t0
    call rust_secondary_main

.L_sleep:
    wfi
    j .L_sleep

    .section .text.entry
    .align 4
_dtb_addr_saved:
    .word 0

