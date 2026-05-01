    .section .text.entry
    .globl _start

_start:
    /* OpenSBI 启动时，a0 = hartid, a1 = dtb 物理地址 */
    bnez a0, .L_sleep

    /* Save DTB address BEFORE setting up stack or clearing BSS */
    /* Use a location in .text.entry which won't be zeroed */
    la t0, _dtb_addr_saved
    sd a1, 0(t0)

    /* 设置启动栈指针 */
    la sp, boot_stack_top

    call rust_main

.L_sleep:
    wfi
    j .L_sleep

/* Place in .text.entry so clear_bss() won't overwrite it */
.section .text.entry
.align 8
_dtb_addr_saved:
    .quad 0
