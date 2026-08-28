.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    addi x1, x0, -1
    addi x2, x0, 1

    slt x3, x1, x2
    bne x3, x2, fail_1
    sltu x4, x1, x2
    bne x4, x0, fail_2
    slti x5, x1, 0
    bne x5, x2, fail_3
    sltiu x6, x2, -1
    bne x6, x2, fail_4

    blt x1, x2, signed_lt
    FAIL 5
signed_lt:
    bge x2, x1, signed_ge
    FAIL 6
signed_ge:
    bltu x2, x1, unsigned_lt
    FAIL 7
unsigned_lt:
    bgeu x1, x2, unsigned_ge
    FAIL 8
unsigned_ge:
    PASS

fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
