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

    # The read-only unprivileged counter aliases observe the same monotonically
    # advancing state as their writable machine-mode counterparts.
    csrr t0, mcycle
    csrr t1, cycle
    bltu t1, t0, fail_14
    sub t2, t1, t0
    addi t3, x0, 64
    bgeu t2, t3, fail_15

    csrr t0, minstret
    addi t2, x0, 0
    csrr t1, instret
    bltu t1, t0, fail_16
    beq t1, t0, fail_17

    # Machine counters are writable. Exact post-write values depend on cycles
    # and retirements between the write and read, so bound the small delta.
    addi t0, x0, 0x100
    csrw mcycle, t0
    csrr t1, cycle
    bltu t1, t0, fail_18
    sub t2, t1, t0
    addi t3, x0, 64
    bgeu t2, t3, fail_19

    # Counter RMW instructions set and clear the requested bits. A synchronous
    # instruction source can put frontend-empty cycles before the following
    # read, so admit a small monotonic mcycle delta after the atomic write.
    addi t1, x0, 0x400
    csrrs t2, mcycle, t1
    csrr t3, mcycle
    or t4, t2, t1
    bltu t3, t4, fail_24
    sub t5, t3, t4
    addi t6, x0, 64
    bgeu t5, t6, fail_24
    and t5, t3, t1
    bne t5, t1, fail_24
    addi t1, x0, 0x100
    csrrc t2, mcycle, t1
    csrr t3, mcycle
    xori t4, t1, -1
    and t4, t2, t4
    bltu t3, t4, fail_25
    sub t5, t3, t4
    addi t6, x0, 64
    bgeu t5, t6, fail_25
    and t5, t3, t1
    bne t5, x0, fail_25

    addi t0, x0, 0x80
    csrw minstret, t0
    nop
    csrr t1, instret
    addi t2, t0, 1
    bne t1, t2, fail_20

    # 0x7c0 selects an implementation-defined stall class and 0x7c1 accesses
    # its 64-bit counter. Selector two is redirect/flush. A JAL must add at
    # least one event after the explicit clear, independent of fetch latency.
    addi t0, x0, 2
    csrw 0x7c0, t0
    csrw 0x7c1, x0
    jal x0, stall_redirect_target
    addi x0, x0, 0
stall_redirect_target:
    csrr t3, 0x7c1
    beq t3, x0, fail_22

    # Selector one is memory wait. Straight-line CSR traffic does not generate
    # a memory wait, so the selected data register retains an exact write.
    addi t0, x0, 1
    csrw 0x7c0, t0
    addi t1, x0, 0x55
    csrw 0x7c1, t1
    csrr t2, 0x7c1
    bne t2, t1, fail_23

    # Writes to cycle/instret are illegal even though each alias shares the
    # compact storage index of its writable machine counterpart.
    addi s0, x0, 2
    la s1, cycle_write_site
    li s2, 0xc0029073
cycle_write_site:
    .word 0xc0029073 # csrw cycle, t0
after_cycle_write:

    addi s0, x0, 2
    la s1, instret_write_site
    li s2, 0xc0229073
instret_write_site:
    .word 0xc0229073 # csrw instret, t0
after_instret_write:

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
fail_14: FAIL 14
fail_15: FAIL 15
fail_16: FAIL 16
fail_17: FAIL 17
fail_18: FAIL 18
fail_19: FAIL 19
fail_20: FAIL 20
fail_22: FAIL 22
fail_23: FAIL 23
fail_24: FAIL 24
fail_25: FAIL 25
