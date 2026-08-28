.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    addi x1, x0, 1

    # RV64 immediate shifts use six shamt bits.
    slli x2, x1, 63
    srli x3, x2, 63
    bne x3, x1, fail_1
    srai x4, x2, 63
    addi x20, x0, -1
    bne x4, x20, fail_2

    # Register shifts mask rs2 to the low six bits.
    addi x5, x0, 63
    sll x6, x1, x5
    bne x6, x2, fail_3
    srl x7, x6, x5
    bne x7, x1, fail_4
    sra x8, x6, x5
    bne x8, x20, fail_5

    # W shifts use five shamt bits and sign-extend bit 31 afterward.
    addi x9, x0, -2
    slliw x10, x9, 1
    addi x21, x0, -4
    bne x10, x21, fail_6
    srliw x11, x9, 31
    bne x11, x1, fail_7
    sraiw x12, x9, 31
    bne x12, x20, fail_8

    addi x13, x0, 31
    sllw x14, x1, x13
    lui x22, 0x80000
    bne x14, x22, fail_9
    srlw x15, x14, x13
    bne x15, x1, fail_10
    sraw x16, x14, x13
    bne x16, x20, fail_11

    addi x17, x0, 64
    sll x18, x1, x17
    bne x18, x1, fail_12
    sllw x19, x1, x17
    bne x19, x1, fail_13
    PASS

fail_1:  FAIL 1
fail_2:  FAIL 2
fail_3:  FAIL 3
fail_4:  FAIL 4
fail_5:  FAIL 5
fail_6:  FAIL 6
fail_7:  FAIL 7
fail_8:  FAIL 8
fail_9:  FAIL 9
fail_10: FAIL 10
fail_11: FAIL 11
fail_12: FAIL 12
fail_13: FAIL 13
