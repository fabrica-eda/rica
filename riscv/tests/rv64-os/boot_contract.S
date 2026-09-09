.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, unexpected_trap
    csrw mtvec, t0
    csrr s0, misa
    li t0, -1
    csrrw t1, misa, zero
    bne t1, s0, fail_1
    csrrw t1, misa, t0
    bne t1, s0, fail_1
    csrrs t1, misa, t0
    bne t1, s0, fail_1
    csrrc t1, misa, t0
    bne t1, s0, fail_1
    csrrwi t1, misa, 0
    bne t1, s0, fail_1
    csrrsi t1, misa, 31
    bne t1, s0, fail_1
    csrrci t1, misa, 31
    bne t1, s0, fail_1
    csrr t1, misa
    bne t1, s0, fail_1

    # WFI may resume without an interrupt. This implementation is a NOP,
    # including with global MIE clear, and counts as a retired instruction.
    csrci mstatus, 8
    csrr t0, minstret
    wfi
    csrr t1, minstret
    sub t1, t1, t0
    li t2, 2
    bne t1, t2, fail_2
    csrsi mstatus, 8
    csrr t0, minstret
    wfi
    csrr t1, minstret
    sub t1, t1, t0
    bne t1, t2, fail_2

    # Minimal M-mode trap frame: exchange stacks through MSCRATCH, preserve
    # the interrupted temporaries, and return to the next instruction.
    li sp, 0x8003d000
    li t0, 0x8003e000
    csrw mscratch, t0
    la t0, trap_handler
    csrw mtvec, t0
    li t0, 0x123456789abcdef0
    li t1, 0xfedcba9876543210
    li s1, 0
    ecall
    li t2, 0x123456789abcdef0
    bne t0, t2, fail_3
    li t2, 0xfedcba9876543210
    bne t1, t2, fail_3
    li t2, 0x8003d000
    bne sp, t2, fail_3
    li t2, 1
    bne s1, t2, fail_3
    csrr t0, mscratch
    li t2, 0x8003e000
    bne t0, t2, fail_3
    csrr t0, mstatus
    andi t0, t0, 0x88
    li t2, 0x88
    bne t0, t2, fail_3
    PASS

.balign 4
trap_handler:
    csrrw sp, mscratch, sp
    addi sp, sp, -16
    sd t0, 0(sp)
    sd t1, 8(sp)
    csrr t0, mcause
    li t1, 11
    bne t0, t1, unexpected_trap
    csrr t0, mtval
    bnez t0, unexpected_trap
    csrr t0, mepc
    addi t0, t0, 4
    csrw mepc, t0
    addi s1, s1, 1
    ld t0, 0(sp)
    ld t1, 8(sp)
    addi sp, sp, 16
    csrrw sp, mscratch, sp
    mret

.balign 4
unexpected_trap: FAIL 4
fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
