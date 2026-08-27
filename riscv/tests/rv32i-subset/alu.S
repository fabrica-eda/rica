.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    lui x1, 0x12345
    addi x1, x1, 0x678
    addi x2, x0, -1
    add x3, x1, x2
    addi x4, x1, -1
    bne x3, x4, fail_1

    sub x5, x3, x1
    bne x5, x2, fail_2
    xor x6, x1, x2
    xori x7, x1, -1
    bne x6, x7, fail_3
    or x8, x1, x2
    bne x8, x2, fail_4
    and x9, x1, x2
    bne x9, x1, fail_5
    ori x10, x0, 0x55
    andi x11, x10, 0x0f
    addi x12, x0, 5
    bne x11, x12, fail_6
    PASS

fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
fail_5: FAIL 5
fail_6: FAIL 6
