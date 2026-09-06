.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a3, 4
outer:
    la a0, source
    la a1, destination
    li a2, 256
loop:
    ld t2, 0(a0)
    sd t2, 0(a1)
    addi a0, a0, 8
    addi a1, a1, 8
    addi a2, a2, -1
    bnez a2, loop
    addi a3, a3, -1
    bnez a3, outer
    la a0, destination
    li a1, 1
    li a2, 257
verify:
    ld t2, 0(a0)
    bne t2, a1, fail
    addi a0, a0, 8
    addi a1, a1, 1
    bne a1, a2, verify
    PASS
fail:
    FAIL 11
.section .rodata, "a", @progbits
.balign 8
source:
.set value, 1
.rept 256
    .dword value
    .set value, value + 1
.endr
.section .bss, "aw", @nobits
.balign 8
destination:
    .zero 2048
