.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a3, 4
    li a2, 0
outer:
    la a0, values
    li a1, 256
loop:
    ld t2, 0(a0)
    add a2, a2, t2
    addi a0, a0, 8
    addi a1, a1, -1
    bnez a1, loop
    addi a3, a3, -1
    bnez a3, outer
    li t3, 131584
    bne a2, t3, fail
    PASS
fail:
    FAIL 10
.section .rodata, "a", @progbits
.balign 8
values:
.set value, 1
.rept 256
    .dword value
    .set value, value + 1
.endr
