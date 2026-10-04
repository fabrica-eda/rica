OUTPUT_ARCH(riscv)
ENTRY(_start)

/* RAM-resident PMP diagnostics: M code, one M test page and one U code page,
   each in its own 4 KiB page so PMP entries can cover them exactly. */
SECTIONS
{
    . = 0x80000000;
    .text : { *(.text.init) *(.text .text.*) }
    . = ALIGN(4096);
    .machine_page : { KEEP(*(.machine_page)) }
    . = ALIGN(4096);
    .user_page : { KEEP(*(.user_page)) }
    ASSERT(SIZEOF(.user_page) == 4096, "U page must fill exactly one page")
    . = ALIGN(4096);
    .data : { *(.data .data.*) }
    .tohost : { KEEP(*(.tohost)) }
    .bss : { *(.bss .bss.*) *(COMMON) }
}
