    .section .text.entry
    .globl _start

_start:
    /* OpenSBI starts with a0 = hartid, a1 = DTB physical address. */
    bnez a0, .L_sleep

    /* Save DTB address before setting up the stack or clearing BSS. */
    la t0, _dtb_addr_saved
    sw a1, 0(t0)

    la sp, boot_stack_top

    call rust_main

.L_sleep:
    wfi
    j .L_sleep

    .section .text.entry
    .align 4
_dtb_addr_saved:
    .word 0
