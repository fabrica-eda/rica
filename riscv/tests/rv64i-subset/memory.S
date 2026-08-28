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

    # Signed and unsigned subword loads observe exactly the bytes written.
    addi x10, x0, -1
    sb x10, 1(x1)
    lb x11, 1(x1)
    bne x11, x10, fail_4
    lbu x12, 1(x1)
    addi x13, x0, 255
    bne x12, x13, fail_5

    sh x10, 2(x1)
    lh x14, 2(x1)
    bne x14, x10, fail_6
    lhu x15, 2(x1)
    lui x16, 0x10
    addi x16, x16, -1
    bne x15, x16, fail_7

    lwu x17, 8(x2)
    addi x18, x0, 1
    slli x18, x18, 31
    addi x18, x18, 1
    bne x17, x18, fail_8
    PASS

fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
fail_5: FAIL 5
fail_6: FAIL 6
fail_7: FAIL 7
fail_8: FAIL 8

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
