.cpu cortex-m3
.thumb

.section .isr_vector, "a", %progbits
.balign 128
.global _isr_vector
_isr_vector:
    .word _stack_top
    .word reset_handler
    .word nmi_handler
    .word hardfault_handler
    .word memmanage_handler
    .word busfault_handler
    .word usagefault_handler
    .word 0
    .word 0
    .word 0
    .word 0
    .word svc_handler
    .word debugmon_handler
    .word 0
    .word pendsv_handler
    .word systick_handler
.rept 37
    .word default_handler
.endr
    .word usart1_handler
.rept 22
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

1:
    cmp r0, r1
    itt lo
    ldrlo r3, [r2], #4
    strlo r3, [r0], #4
    blo 1b

    ldr r0, =_sbss
    ldr r1, =_ebss
    movs r2, #0

2:
    cmp r0, r1
    itt lo
    strlo r2, [r0], #4
    blo 2b

    bl platform_init
    b .

.section .text.handlers, "ax", %progbits
.thumb_func
default_handler:
    b .
.thumb_func
usagefault_handler:
    b .
.thumb_func
svc_handler:
    b .
.thumb_func
debugmon_handler:
    b .