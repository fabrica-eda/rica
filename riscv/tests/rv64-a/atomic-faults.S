.include "test_macros.inc"
.macro PROBE op, cause, address
    li s4, \cause
    li s5, \address
    la s6, resume\@
    la s7, fault\@
fault\@:
    \op t2, t3, (s5)
    j fail
resume\@:
.endm
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, trap
    csrw mtvec, t0
    li s3, 0
    li t3, 1
    # SC must check addresses even without a reservation.
    PROBE sc.d, 7, 0x02004000
    PROBE sc.w, 7, 0x180000000
    PROBE sc.d, 6, 0x80000101
    PROBE amoadd.d, 6, 0x80000104
    PROBE amoswap.w, 6, 0x80000102
    PROBE amoadd.d, 7, 0x8003f000
    PROBE amoxor.w, 7, 0x180000000
    PROBE amoor.d, 7, 0xffffffff80000000
    PROBE amoand.w, 7, 0x02000000
    PROBE amomin.d, 7, 0x00000000
    PROBE amomax.w, 7, 0x02010000
    li s4, 4
    li s5, 0x80000101
    la s6, lr_resume
    la s7, lr_fault
lr_fault:
    lr.d t2, (s5)
    j fail
lr_resume:
    li s4, 5
    li s5, 0x0200bff8
    la s6, lr_mmio_resume
    la s7, lr_mmio_fault
lr_mmio_fault:
    lr.w t2, (s5)
    j fail
lr_mmio_resume:
    li t0, 13
    bne s3, t0, fail
    # Wrong-path AMO must never issue a write, including predictor warmup.
    la s0, cell
    sd zero, 0(s0)
    li s1, 64
branch_loop:
    beq zero, zero, skip
    amoadd.d zero, t3, (s0)
skip:
    addi s1, s1, -1
    bnez s1, branch_loop
    ld t0, 0(s0)
    bnez t0, fail
    PASS
.balign 4
trap:
    csrr t0, mcause
    bne t0, s4, fail
    csrr t0, mtval
    bne t0, s5, fail
    csrr t0, mepc
    bne t0, s7, fail
    csrw mepc, s6
    addi s3, s3, 1
    mret
.balign 4
fail:
    FAIL 92
.section .data, "aw", @progbits
.balign 8
cell: .dword 0
