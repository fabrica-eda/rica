.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    # Fill the instruction cache, then change a previously executed function.
    call patched_function
    li t2, 1
    bne a0, t2, fail
    la t3, patched_function
    li t4, 0x00200513             # addi a0, zero, 2
    sw t4, 0(t3)
    fence.i
    call patched_function
    li t2, 2
    bne a0, t2, fail

    # The fallthrough may already be in the fetch/decode queues. FENCE.I must
    # restart those queues as well as invalidating cached instruction data.
    la t3, following
    li t4, 0x00300513             # addi a0, zero, 3
    sw t4, 0(t3)
    fence.i
following:
    addi a0, zero, 1
    li t2, 3
    bne a0, t2, fail

    # Patch a 32-bit instruction that straddles two 64-bit cache lines.
    call cross_function
    li t2, 1
    bne a0, t2, fail
    la t3, cross_function
    li t4, 0x00500513             # addi a0, zero, 5
    sh t4, 0(t3)
    srli t4, t4, 16
    sh t4, 2(t3)
    # Zifencei requires base implementations to ignore the reserved operands.
    .word 0xffff9f8f             # FENCE.I with nonzero funct12, rs1 and rd
    call cross_function
    li t2, 5
    bne a0, t2, fail

    # The final halfword in a line is independently cacheable with Zca.
    call compressed_function
    li t2, 1
    bne a0, t2, fail
    la t3, compressed_function
    li t4, 0x4511                # c.li a0, 4
    sh t4, 0(t3)
    fence.i
    call compressed_function
    li t2, 4
    bne a0, t2, fail
    PASS
fail:
    FAIL 1

.balign 8
patched_function:
    addi a0, zero, 1
    ret
.balign 8
    .2byte 1, 1, 1
cross_function:
    addi a0, zero, 1
    ret
.balign 8
    .2byte 1, 1, 1
compressed_function:
    .2byte 0x4505                # c.li a0, 1
    ret
