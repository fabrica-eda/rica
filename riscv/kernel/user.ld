OUTPUT_ARCH(riscv)
ENTRY(_start)
MEMORY { RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 0x20000
USER (rx) : ORIGIN = 0x80020000, LENGTH = 0x2000 }
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
    .user_text 0x80020000 : { KEEP(*(.user_text)) } > USER
    .user_tail 0x80021000 : { KEEP(*(.user_tail)) } > USER
    ASSERT(SIZEOF(.user_text) <= 4096, "user text exceeds PMP region")
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
