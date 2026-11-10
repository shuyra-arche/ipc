OUTPUT_ARCH(riscv)
ENTRY(_start)
PHDRS { code PT_LOAD FLAGS(5); }
SECTIONS {
    . = 0x10000;
    .text : {
        __code_start = .;
        KEEP(*(.text.entry))
        *(.text .text.*)
        *(.rodata .rodata.* .srodata .srodata.*)
        __code_end = .;
    } :code
    /DISCARD/ : { *(.eh_frame*) }
}
