.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    la x1, scratch
    lui x2, 0x12345
    addi x2, x2, 0x678
    sw x2, 0(x1)
    lw x3, 0(x1)
    xor x4, x3, x2
    bne x4, x0, fail_1

    addi x5, x2, 1
    sw x5, 4(x1)
    lw x6, 4(x1)
    bne x6, x5, fail_2
    PASS

fail_1: FAIL 1
fail_2: FAIL 2

.section .data, "aw", @progbits
.balign 4
scratch:
    .word 0
    .word 0
