OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x80000000;
    .text : { *(.text.init) *(.text .text.*) }
    . = ALIGN(8);
    .rodata : { *(.rodata .rodata.*) }
    . = ALIGN(8);
    .data : { *(.data .data.*) }
    .tohost : { KEEP(*(.tohost)) }
    .bss : { *(.bss .bss.*) *(COMMON) }
}
