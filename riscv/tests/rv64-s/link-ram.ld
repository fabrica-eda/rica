OUTPUT_ARCH(riscv)
ENTRY(_start)

/* RAM-resident diagnostics. Page tables and test pages use fixed physical
   addresses above 0x80020000, so the image must stay below them. */
SECTIONS
{
    . = 0x80000000;
    .text : { *(.text.init) *(.text .text.*) }
    . = ALIGN(8);
    .data : { *(.data .data.*) }
    .tohost : { KEEP(*(.tohost)) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    ASSERT(. <= 0x80020000, "image overlaps the page tables")
}
