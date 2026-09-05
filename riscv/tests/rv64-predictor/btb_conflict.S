.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li a0, 0
    li a1, 256
    j site_a
.balign 64
site_a:
    addi a0, a0, 1
    j site_b
.balign 64
site_b:
    addi a1, a1, -1
    # This branch and the jump above differ by 64 bytes: they share a
    # 32-entry halfword PC index but occupy different rows with 64 entries.
    bnez a1, site_a
    li a2, 256
    bne a0, a2, fail
    PASS
fail:
    FAIL 2
