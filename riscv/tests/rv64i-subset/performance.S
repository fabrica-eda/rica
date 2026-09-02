.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    # Give every implementation event counter a distinct non-zero high half.
    # The physical JTAG readback checks these exact halves, which exercises
    # both response banks and every selector without requiring 2^32 cycles.
    addi t0, x0, 0
    csrw 0x7c0, t0
    li t1, 0x0000000100000010
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 1
    bne t2, t3, fail_1

    addi t0, x0, 1
    csrw 0x7c0, t0
    li t1, 0x0000000200000020
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 2
    bne t2, t3, fail_2

    addi t0, x0, 2
    csrw 0x7c0, t0
    li t1, 0x0000000300000030
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 3
    bne t2, t3, fail_3

    addi t0, x0, 3
    csrw 0x7c0, t0
    li t1, 0x0000000400000040
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 4
    bne t2, t3, fail_4

    addi t0, x0, 4
    csrw 0x7c0, t0
    li t1, 0x0000000500000050
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 5
    bne t2, t3, fail_5

    addi t0, x0, 5
    csrw 0x7c0, t0
    li t1, 0x0000000600000060
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 6
    bne t2, t3, fail_6

    addi t0, x0, 6
    csrw 0x7c0, t0
    li t1, 0x0000000700000070
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 7
    bne t2, t3, fail_7

    addi t0, x0, 7
    csrw 0x7c0, t0
    li t1, 0x0000000800000080
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 8
    bne t2, t3, fail_8

    addi t0, x0, 8
    csrw 0x7c0, t0
    li t1, 0x0000000900000090
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    srli t2, t2, 32
    addi t3, x0, 9
    bne t2, t3, fail_9

    # Keep mcycle above minstret and the sum of the four seeded event values.
    # Later increments only change the low halves during this short program.
    li t0, 0x0000001000000050
    csrw mcycle, t0
    csrr t1, cycle
    srli t1, t1, 32
    addi t2, x0, 16
    bne t1, t2, fail_10

    li t0, 0x0000000800000060
    csrw minstret, t0
    csrr t1, instret
    srli t1, t1, 32
    addi t2, x0, 8
    bne t1, t2, fail_11

    PASS

fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
fail_5: FAIL 5
fail_6: FAIL 6
fail_7: FAIL 7
fail_8: FAIL 8
fail_9: FAIL 9
fail_10: FAIL 10
fail_11: FAIL 11
