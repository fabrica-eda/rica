.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    la x1, scratch
    la x2, source
    ld x3, 0(x2)
    sd x3, 0(x1)
    ld x4, 0(x1)
    xor x5, x3, x4
    bne x5, x0, fail_1

    # LW sign-extends bit 31, while SW preserves the upper four bytes.
    lw x6, 8(x2)
    ld x7, 16(x2)
    bne x6, x7, fail_2
    sw x6, 0(x1)
    ld x8, 0(x1)
    ld x9, 24(x2)
    bne x8, x9, fail_3
    PASS

fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3

.section .rodata, "a", @progbits
.balign 8
source:
    .dword 0x123456789abcdef0
    .word 0x80000001
    .word 0
    .dword 0xffffffff80000001
    .dword 0x1234567880000001

.section .data, "aw", @progbits
.balign 8
scratch:
    .dword 0
