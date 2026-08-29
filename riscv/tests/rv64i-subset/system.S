.include "test_macros.inc"

.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, trap_handler
    csrw mtvec, t0

    # Register and immediate CSR forms return the old value and update exactly
    # the requested bits. Keep consumers adjacent to exercise CSR forwarding.
    addi t1, x0, 0x55
    csrrw t2, mscratch, t1
    addi t3, t2, 1
    addi t4, x0, 1
    bne t3, t4, fail_1
    csrrs t2, mscratch, x0
    bne t2, t1, fail_2

    addi t3, x0, 0x0a
    csrrs t2, mscratch, t3
    bne t2, t1, fail_3
    addi t4, x0, 0x5f
    csrrc t2, mscratch, t3
    bne t2, t4, fail_4
    csrr t2, mscratch
    bne t2, t1, fail_5

    csrwi mscratch, 3
    csrrsi t2, mscratch, 4
    addi t3, x0, 3
    bne t2, t3, fail_6
    csrrci t2, mscratch, 1
    addi t3, x0, 7
    bne t2, t3, fail_7
    csrr t2, mscratch
    addi t3, x0, 6
    bne t2, t3, fail_8

    # There is no instruction cache, so architectural FENCE.I is a no-op.
    fence.i

    # Illegal instruction: MCAUSE=2 and MTVAL contains the bad encoding.
    addi s0, x0, 2
    la s1, illegal_site
    addi s2, x0, -1
    srli s2, s2, 32
illegal_site:
    .word 0xffffffff
after_illegal:

    # EBREAK and ECALL report zero MTVAL and return through MRET.
    addi s0, x0, 3
    la s1, ebreak_site
    addi s2, x0, 0
ebreak_site:
    ebreak
after_ebreak:

    # Verify the MIE/MPIE/MPP stack on both trap entry and MRET.
    addi t0, x0, 8
    csrw mstatus, t0
    addi s0, x0, 11
    la s1, ecall_site
    addi s2, x0, 0
ecall_site:
    ecall
after_ecall:
    csrr t0, mstatus
    lui t1, 0x2
    addi t1, t1, -0x778
    bne t0, t1, fail_12

    PASS

trap_handler:
    csrr t0, mcause
    bne t0, s0, fail_9
    csrr t0, mepc
    bne t0, s1, fail_10
    csrr t0, mtval
    bne t0, s2, fail_11

    addi t1, x0, 11
    bne s0, t1, trap_resume
    csrr t0, mstatus
    lui t1, 0x2
    addi t1, t1, -0x780
    bne t0, t1, fail_13

trap_resume:
    addi t0, s1, 4
    csrw mepc, t0
    mret

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
