    .section .text.entry
    .globl _start
    .globl secondary_entry

_start:
    /* OpenSBI starts with a0 = hartid, a1 = DTB physical address. */
    bnez a0, .L_sleep

    /* Save DTB address BEFORE setting up stack or clearing BSS. */
    /* Use a location in .text.entry which won't be zeroed. */
    la t0, _dtb_addr_saved
    sd a1, 0(t0)

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

/* Place in .text.entry so clear_bss() won't overwrite it. */
.section .text.entry
.align 8
_dtb_addr_saved:
    .quad 0

