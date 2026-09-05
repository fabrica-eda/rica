.include "test_macros.inc"
.option rvc
.section .text.init, "ax", @progbits
.globl _start
_start:
    la sp, stack_end
    li s0, 256
    li a0, 0
    la s1, outer
loop:
    # The same return instruction alternates between two callers; the
    # compressed call also checks the architectural PC+2 return address.
    c.jalr s1
    jal ra, outer
    c.addi s0, -1
    bnez s0, loop
    li a1, 512
    bne a0, a1, fail
    PASS
outer:
    addi sp, sp, -16
    sd ra, 8(sp)
    jal ra, leaf
    ld ra, 8(sp)
    addi sp, sp, 16
    ret
leaf:
    c.addi a0, 1
    c.jr ra
fail:
    FAIL 1
.section .bss, "aw", @nobits
.balign 16
stack:
    .space 256
stack_end:
