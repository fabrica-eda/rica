.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, fail
    csrw mtvec, t0
    la s0, cell
    addi s1, s0, 4
    li t0, 0x8000000012345678
    sd t0, 0(s0)
    li t1, 1
    amoadd.w.aqrl t2, t1, (s1)
    li t3, -2147483648
    bne t2, t3, fail
    ld t0, 0(s0)
    li t3, 0x8000000112345678
    bne t0, t3, fail
    lr.w t2, (s1)
    li t3, -2147483647
    bne t2, t3, fail
    sc.w t2, zero, (s1)
    bnez t2, fail
    ld t0, 0(s0)
    li t3, 0x12345678
    bne t0, t3, fail
    PASS
.balign 4
fail:
    FAIL 94
.section .data, "aw", @progbits
.balign 8
cell: .dword 0
