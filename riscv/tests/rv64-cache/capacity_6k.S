.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a0, 0
    li a1, 4
.balign 16
1:
    .rept 1536
    addi a0, a0, 1
    .endr
    addi a1, a1, -1
    # The body exceeds the +/-4 KiB range of a conditional branch.
    beqz a1, 2f
    j 1b
2:
    li a2, 6144
    bne a0, a2, fail
    PASS
fail:
    FAIL 1
