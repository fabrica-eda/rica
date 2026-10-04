.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la sp, stack_top
    la t0, trap
    csrw mtvec, t0
    csrci mstatus, 8
    li s6, 0x02004000
    li s7, 0x0200bff8
    li t0, -1
    sd t0, 0(s6)
    li t0, 128
    csrw mie, t0
    la s0, cell
    li s2, 0
    li s3, 0
    li a1, 1
loop:
    # Sweep the timer boundary across the serialized atomic transaction.
    addi s4, s3, 1
    ld t0, 0(s7)
    add t0, t0, s2
    addi t0, t0, 1
    sd t0, 0(s6)
    csrsi mstatus, 8
    amoadd.d.aqrl a0, a1, (s0)
    bne a0, s2, fail
wait_irq:
    bne s3, s4, wait_irq
    csrci mstatus, 8
    addi s2, s2, 1
    li t0, 64
    bne s2, t0, loop
    ld t0, 0(s0)
    bne t0, s2, fail
    bne s3, s2, fail
    PASS
.balign 4
trap:
    addi sp, sp, -32
    sd t0, 0(sp)
    sd t1, 8(sp)
    csrr t0, mcause
    li t1, 0x8000000000000007
    bne t0, t1, fail
    li t0, -1
    sd t0, 0(s6)
    addi s3, s3, 1
    ld t0, 0(sp)
    ld t1, 8(sp)
    addi sp, sp, 32
    mret
.balign 4
fail:
    FAIL 93
.section .data, "aw", @progbits
.balign 8
cell: .dword 0
.section .bss, "aw", @nobits
.balign 16
.space 256
stack_top:
