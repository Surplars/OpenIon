.cpu cortex-m33
.thumb

.section .isr_vector, "a", %progbits
.balign 1024
.global _isr_vector
_isr_vector:
    .word _stack_top        	/* 初始 MSP */
    .word reset_handler     	/* Reset */
    .word nmi_handler       	/* NMI */
    .word hardfault_handler 	/* HardFault */
    .word memmanage_handler 	/* MemManage */
    .word busfault_handler  	/* BusFault */
    .word usagefault_handler 	/* UsageFault */
    .word 0                 	/* Reserved */
    .word 0                 	/* Reserved */
    .word 0                 	/* Reserved */
    .word 0                 	/* Reserved */
    .word svc_handler       	/* SVC */
    .word debugmon_handler      /* DebugMon */
    .word 0                 	/* Reserved */
    .word pendsv_handler        /* PendSV */
    .word systick_handler       /* SysTick */
    .word uart0_rx_handler      /* IRQ 0: UART0 RX */
.rept 47
    .word default_handler
.endr
	.word eth_handler           /* IRQ 48: Ethernet */
.rept 47
    .word default_handler
.endr

.section .text.reset, "ax", %progbits
.global reset_handler
.thumb_func
.align 4
reset_handler:
    /* 复制 .data 段 */
    ldr r0, =_sdata
    ldr r1, =_edata
    ldr r2, =_sidata

data_copy_loop:
    cmp r0, r1
    itt lo
    ldrlo r3, [r2], #4
    strlo r3, [r0], #4
    blo data_copy_loop
   /* 清零 .bss 段 */
    ldr   r0, =_sbss
    ldr   r1, =_ebss
    movs  r2, #0

bss_clear_loop:
    cmp   r0, r1
    it    lt
    strlt r2, [r0], #4
    blt   bss_clear_loop

start_rust:
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
