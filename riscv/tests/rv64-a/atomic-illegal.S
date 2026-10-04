.include "test_macros.inc"
.macro ILLEGAL word
    li s0, \word
    la s1, inst\@
    la s2, resume\@
inst\@:
    .word \word
    j fail
resume\@:
.endm
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, trap
    csrw mtvec, t0
    li s3, 0
    ILLEGAL 0x101130af # LR.D with rs2 != zero
    ILLEGAL 0x000100af # unsupported byte width
    ILLEGAL 0x000110af # unsupported halfword width
    ILLEGAL 0x000140af # reserved width
    ILLEGAL 0x280130af # reserved funct5
    li t0, 5
    bne s3, t0, fail
    PASS
.balign 4
trap:
    csrr t0, mcause
    li t1, 2
    bne t0, t1, fail
    csrr t0, mtval
    bne t0, s0, fail
    csrr t0, mepc
    bne t0, s1, fail
    csrw mepc, s2
    addi s3, s3, 1
    mret
.balign 4
fail:
    FAIL 95
