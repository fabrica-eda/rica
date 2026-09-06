.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la a0, nodes
    mv a3, a0
    li a1, 1024
    li a2, 0
loop:
    ld a0, 0(a0)
    add a2, a2, a0
    addi a1, a1, -1
    bnez a1, loop
    bne a0, a3, fail
    slli a3, a3, 10
    li a4, 1044480
    add a3, a3, a4
    bne a2, a3, fail
    PASS
fail:
    FAIL 12
.section .rodata, "a", @progbits
.balign 8
nodes:
.set index, 0
.rept 256
    .dword nodes + 8 * ((index + 73) & 255)
    .set index, index + 1
.endr
