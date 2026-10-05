.include "test_macros.inc"
#ifndef PLATFORM_TIMEBASE_HZ
#define PLATFORM_TIMEBASE_HZ 62400000
#endif
.section .text.init, "ax", @progbits
.globl _start
_start:
    li sp, 0x8003d000
    la t0, trap_handler
    csrw mtvec, t0
    li t0, -1
    csrw mie, t0
    csrr t1, mie
    # S-capable harts also implement the supervisor enables.
    csrr t2, misa
    srli t2, t2, 18
    andi t2, t2, 1
    li t3, 0x888
    beqz t2, 1f
    li t3, 0xaaa
1:  bne t1, t3, fail_1
    csrw mie, zero
    li s0, 0x02000000
    li s1, 0x02004000
    li s2, 0x0200bff8
    ld t0, -8(s2)
    li t1, PLATFORM_TIMEBASE_HZ
    bne t0, t1, fail_1
    ld t0, 0(s2)
    nop
    ld t1, 0(s2)
    bgeu t0, t1, fail_1
    # Publish two pending levels with both global/local masks disabled.
    li t0, 1
    sw t0, 0(s0)
    sd zero, 0(s1)
    li s3, 0
    li s4, 0
    li s5, 0
wait_pending:
    csrr t0, mip
    andi t0, t0, 0x88
    li t1, 0x88
    bne t0, t1, wait_pending
    # MIP writes cannot acknowledge platform-owned pending bits.
    csrw mip, zero
    csrr t0, mip
    andi t0, t0, 0x88
    bne t0, t1, fail_2
    li t0, 0x88
    csrw mie, t0
    bnez s3, fail_2
    csrsi mstatus, 8
resume:
    # The handler must have serviced MSI before MTI. Returning with MTI
    # still pending must trap again before this successor can execute.
    li t0, 1
    bne s3, t0, fail_3
    beqz s4, fail_3
    li t0, 0x37
    bne s5, t0, fail_3
    csrci mstatus, 8
    csrr t0, mip
    andi t0, t0, 0x88
    bnez t0, fail_3
    PASS

.balign 4
trap_handler:
    addi sp, sp, -32
    sd t0, 0(sp)
    sd t1, 8(sp)
    sd t2, 16(sp)
    csrr t0, mtval
    bnez t0, fail_4
    csrr t0, mstatus
    andi t0, t0, 0x88
    li t1, 0x80
    bne t0, t1, fail_4
    csrr t0, mepc
    la t1, resume
    bne t0, t1, fail_4
    csrr t0, mcause
    li t1, 0x8000000000000003
    beq t0, t1, software
    li t1, 0x8000000000000007
    bne t0, t1, fail_4
    li t0, 1
    bne s3, t0, fail_4
    addi s4, s4, 1
    li t0, -1
    sd t0, 0(s1)
    # Eventual MTIP clearing is allowed; keep the ordering signature once.
    li s5, 0x37
    j return
software:
    bnez s3, fail_4
    bnez s4, fail_4
    addi s3, s3, 1
    li s5, 3
    sw zero, 0(s0)
return:
    ld t0, 0(sp)
    ld t1, 8(sp)
    ld t2, 16(sp)
    addi sp, sp, 32
    mret
fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
