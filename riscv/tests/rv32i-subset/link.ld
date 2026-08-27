OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0;
    .text : { *(.text.init) *(.text .text.*) }
    . = ALIGN(4);
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .tohost : { KEEP(*(.tohost)) }
    .bss : { *(.bss .bss.*) *(COMMON) }
}
