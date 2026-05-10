OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY {
    SRAM (rwx) : ORIGIN = 0x40100000, LENGTH = 0x00f00000
}
BASE_ADDRESS = 0x40100000;

SECTIONS
{
    /* Load the kernel at this address: OpenSBI jumps here */
    . = BASE_ADDRESS;
    skernel = .;

    .text : {
        *(.text.entry)
        *(.text .text.*)
    } > SRAM

    . = ALIGN(4K);
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } > SRAM

    . = ALIGN(4K);
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } > SRAM

    . = ALIGN(4K);
    .bss : {
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        ebss = .;
    } > SRAM

    . = ALIGN(4K);
    /* Boot stack of 64KB */
    boot_stack_bottom = .;
    . += 0x10000;
    boot_stack_top = .;
    
    ekernel = .;
}
