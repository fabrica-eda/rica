.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a0, 0
    li a1, 512
    li a2, 0
1:
    # A repeating N,N,T,T pattern needs history; a per-PC counter cannot
    # identify the next direction from its own saturated state alone.
    andi a3, a2, 2
    bnez a3, 2f
    addi a0, a0, 1
2:
    addi a2, a2, 1
    addi a1, a1, -1
    bnez a1, 1b
    li a4, 256
    bne a0, a4, fail
    PASS
fail:
    FAIL 2
