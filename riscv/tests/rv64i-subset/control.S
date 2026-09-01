.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    fence rw, rw
auipc_site:
    auipc x1, 0
    la x2, auipc_site
    bne x1, x2, fail_1

    addi x3, x0, 7
    addi x4, x0, 7
    beq x3, x4, beq_taken
    FAIL 2
beq_taken:
    bne x3, x0, bne_taken
    FAIL 3
bne_taken:
    jal x5, jal_target
jal_return:
    FAIL 4
jal_target:
    la x6, jal_return
    bne x5, x6, fail_5
    la x7, jalr_target
    jalr x8, 0(x7)
jalr_return:
    FAIL 6
jalr_target:
    la x9, jalr_return
    bne x8, x9, fail_7

    # A taken branch can have its target load in the speculative pipeline
    # before the registered redirect is applied.  The flushed copy must not
    # emit a memory request; the refetched load must still forward to the
    # immediately dependent branch.
    la x10, redirect_load_value
    addi x11, x0, 0x5a
    beq x0, x0, redirect_load_target
    addi x11, x0, -1
redirect_load_target:
    ld x12, 0(x10)
    beq x12, x11, redirect_load_ok
    FAIL 8
redirect_load_ok:
    PASS

fail_1: FAIL 1
fail_5: FAIL 5
fail_7: FAIL 7

.section .data, "aw", @progbits
.balign 8
redirect_load_value:
    .dword 0x5a
