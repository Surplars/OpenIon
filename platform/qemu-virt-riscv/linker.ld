OUTPUT_ARCH(riscv)
ENTRY(_start)

BASE_ADDRESS = 0x80200000;

SECTIONS
{
    /* Load the kernel at this address: OpenSBI jumps here */
    . = BASE_ADDRESS;
    skernel = .;

    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    . = ALIGN(4K);
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    . = ALIGN(4K);
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    . = ALIGN(4K);
    .bss : {
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        ebss = .;
    }

    . = ALIGN(4K);
    /* Boot stack of 64KB */
    boot_stack_bottom = .;
    . += 0x10000;
    boot_stack_top = .;
    
    ekernel = .;
}
