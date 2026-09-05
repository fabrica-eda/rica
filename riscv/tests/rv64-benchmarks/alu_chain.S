.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a0, 0
    li a1, 256
1:
    .rept 16
    addi a0, a0, 1
    .endr
    addi a1, a1, -1
    bnez a1, 1b
    li a2, 4096
    bne a0, a2, fail
    PASS
fail:
    FAIL 1
