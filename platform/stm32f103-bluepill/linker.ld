MEMORY {
    ROM (rx)  : ORIGIN = 0x08000000, LENGTH = 128K
    RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 20K
}

ENTRY(reset_handler)

_stack_size = 0x0600;

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

    .bss (NOLOAD) :
    {
        . = ALIGN(8);
        _sbss = .;
        *(.bss*)
        *(COMMON)
        . = ALIGN(8);
        _ebss = .;
    } > RAM

    .stack (NOLOAD) :
    {
        . = ALIGN(8);
        _stack_end = .;
        . = . + _stack_size;
        . = ALIGN(8);
        _stack_top = .;
    } > RAM
}
