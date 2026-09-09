OUTPUT_ARCH(riscv)
ENTRY(_start)
MEMORY { RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 0x3e000 }
SECTIONS
{
    .text : { KEEP(*(.text.init)) *(.text .text.*) } > RAM
    .rodata : ALIGN(16) { *(.rodata .rodata.* .srodata .srodata.*) } > RAM
    .data : ALIGN(16) {
        __global_pointer$ = . + 0x800;
        *(.data .data.* .sdata .sdata.*)
    } > RAM
    .tohost : ALIGN(8) { KEEP(*(.tohost)) } > RAM
    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.* .sbss .sbss.*) *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    } > RAM
    .boot_stack (NOLOAD) : ALIGN(16) {
        . += 8192;
        __boot_stack_top = .;
    } > RAM
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
