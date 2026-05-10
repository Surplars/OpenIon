.cpu cortex-m4
.thumb

.section .isr_vector, "a", %progbits
.balign 512
.global _isr_vector
_isr_vector:
    .word _stack_top
    .word reset_handler
    .word nmi_handler
    .word hardfault_handler
    .word memmanage_handler
    .word busfault_handler
    .word default_handler
    .word 0
    .word 0
    .word 0
    .word 0
    .word default_handler
    .word default_handler
    .word 0
    .word pendsv_handler
    .word systick_handler
.rept 37
    .word default_handler
.endr
    .word usart1_handler
.rept 26
    .word default_handler
.endr

.section .text.reset, "ax", %progbits
.global reset_handler
.thumb_func
.align 4
reset_handler:
    ldr r0, =_sdata
    ldr r1, =_edata
    ldr r2, =_sidata

data_copy_loop:
    cmp r0, r1
    itt lo
    ldrlo r3, [r2], #4
    strlo r3, [r0], #4
    blo data_copy_loop

    ldr r0, =_sbss
    ldr r1, =_ebss
    movs r2, #0

bss_clear_loop:
    cmp r0, r1
    it lt
    strlt r2, [r0], #4
    blt bss_clear_loop

    bl platform_init
    b .

.section .text.handlers, "ax", %progbits
.thumb_func
default_handler:
    b .
