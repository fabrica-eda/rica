.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la a0, value
    li a1, 256
1:
    ld a2, 0(a0)
    addi a2, a2, 1
    sd a2, 0(a0)
    addi a1, a1, -1
    bnez a1, 1b
    ld a2, 0(a0)
    li a3, 256
    bne a2, a3, fail
    PASS
fail:
    FAIL 4
.section .data, "aw", @progbits
.balign 8
value:
    .dword 0
