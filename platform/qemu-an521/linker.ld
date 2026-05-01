MEMORY {
    ROM (rx)  : ORIGIN = 0x10000000, LENGTH = 1M
    RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 16M
}

ENTRY(reset_handler)

_stack_size = 0x4000;  /* 16KB */

SECTIONS {
    .text :
    {
        . = ALIGN(4);
        KEEP(*(.isr_vector))
        . = ALIGN(4);
        *(.text*)
    } > ROM

    .rodata :
    {
        . = ALIGN(4);
        *(.rodata*)
    } > ROM

    .data :
    {
        . = ALIGN(8);
        _sdata = .;
        *(.data*)
        . = ALIGN(8);
        _edata = .;
    } > RAM AT> ROM
    _sidata = LOADADDR(.data);

    .stack (NOLOAD) :
    {
        . = ALIGN(4);
        _stack_end = .;
        . = . + _stack_size;
        . = ALIGN(4);
        _stack_top = .;
    } > RAM

    .bss (NOLOAD) :
    {
        . = ALIGN(8);
        _sbss = .;
        *(.bss*)
        *(COMMON)
        . = ALIGN(8);
        _ebss = .;
    } > RAM
}