OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
  . = 0;
  .text : { *(.text.init) *(.text .text.*) }
  . = 0x100;
  .tohost : { KEEP(*(.tohost)) }
}
