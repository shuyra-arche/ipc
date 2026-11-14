OUTPUT_ARCH(riscv)
ENTRY(_start)
PHDRS { code PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS {
    . = 0x10000;
    __code_start = .;
    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
        *(.rodata .rodata.* .srodata .srodata.*)
        . = ALIGN(4096);
    } :code
    __data_start = .;
    .data : { *(.data .data.* .sdata .sdata.*) } :data
    .bss : { *(.bss .bss.* .sbss .sbss.*) *(COMMON) . = ALIGN(4096); } :data
    __image_end = .;
    /DISCARD/ : { *(.eh_frame*) }
}
