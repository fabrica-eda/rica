OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
 . = 0x80000000;
 .text : { *(.text.entry) *(.text .text.*) }
 .rodata : { *(.rodata .rodata.*) }
 .data : { *(.data .data.*) *(.sdata .sdata.*) }
 PROVIDE(__global_pointer$ = . + 0x800);
 .bss : { *(.bss .bss.*) *(.sbss .sbss.*) }
 /DISCARD/ : { *(.eh_frame*) *(.comment) }
}
