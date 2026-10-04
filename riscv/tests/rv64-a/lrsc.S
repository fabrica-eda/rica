.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, fail
    csrw mtvec, t0
    la s0, cell
    la s1, other
    li s2, 0x1234567880000000
    sd s2, 0(s0)
    sc.d t0, zero, (s0)
    beqz t0, fail
    lr.w.aq t0, (s0)
    li t1, -2147483648
    bne t0, t1, fail
    li t2, 0xfedcba9876543210
    sc.w.rl t0, t2, (s0)
    bnez t0, fail
    ld t0, 0(s0)
    li t1, 0x1234567876543210
    bne t0, t1, fail
    sc.d t0, zero, (s0)
    beqz t0, fail
    lr.d t0, (s0)
    # SC to another address consumes the reservation.
    sc.d t1, zero, (s1)
    beqz t1, fail
    sc.d t1, zero, (s0)
    beqz t1, fail
    lr.d t0, (s0)
    sd t2, 0(s0)
    sc.d t1, zero, (s0)
    beqz t1, fail
    lr.d t0, (s0)
    amoadd.d zero, zero, (s1)
    sc.d t1, zero, (s0)
    beqz t1, fail
    # Second LR replaces the first; zero destination still creates reservation.
    lr.d zero, (s0)
    lr.d zero, (s1)
    sc.d t1, t2, (s1)
    bnez t1, fail
    ld t0, 0(s1)
    bne t0, t2, fail
    sd zero, 0(s0)
    li s3, 64
loop:
    # Constrained loop: no stores, loads or SYSTEM between LR and SC.
    lr.d.aqrl t0, (s0)
    addi t0, t0, 1
    sc.d.aqrl t1, t0, (s0)
    bnez t1, loop
    addi s3, s3, -1
    bnez s3, loop
    ld t0, 0(s0)
    li t1, 64
    bne t0, t1, fail
    # Trap and MRET cannot carry a reservation across task switches.
    la t0, trap
    csrw mtvec, t0
    lr.d t0, (s0)
    ecall
    sc.d t0, zero, (s0)
    beqz t0, fail
    PASS
.balign 4
trap:
    csrr t0, mcause
    li t1, 11
    bne t0, t1, fail
    csrr t0, mepc
    addi t0, t0, 4
    csrw mepc, t0
    mret
.balign 4
fail:
    FAIL 91
.section .data, "aw", @progbits
.balign 8
cell: .dword 0
other: .dword 0
