.include "test_macros.inc"
.option rvc
.section .text.init, "ax", @progbits
.globl _start
_start:
    li s0, 256
    li a0, 0
    la s1, leaf
1:
    # c.jalr writes a PC+2 return address to x1.
    c.jalr s1
    c.addi s0, -1
    bnez s0, 1b
    li a1, 768
    bne a0, a1, fail
    PASS
leaf:
    c.addi a0, 1
    c.addi a0, 2
    c.jr ra
fail:
    FAIL 3
