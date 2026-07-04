OUTPUT_ARCH(riscv)
ENTRY(_start)

PROVIDE(BASE_ADDRESS = 0x80200000);

SECTIONS
{
    /* Load the kernel at this address: OpenSBI jumps here */
    . = BASE_ADDRESS;
    skernel = .;

    stext = .;
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }
    etext = .;

    . = ALIGN(4K);
    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }
    erodata = .;

    . = ALIGN(4K);
    sdata = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    edata = .;

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

    . = ALIGN(4K);
    /* Early parking stacks for secondary harts during SMP bring-up. */
    secondary_boot_stack_bottom = .;
    . += 0x10000;
    secondary_boot_stack_top = .;

    ekernel = .;
}
