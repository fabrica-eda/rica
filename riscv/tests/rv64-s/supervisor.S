.include "test_macros.inc"
# Supervisor privilege, delegation and SRET on the whole core. M sets up a
# PMP entry covering the address space, then moves between M, S and U.
# Each trap is checked against an expectation (cause, xEPC, continuation and
# privilege to resume in) by the M or S handler. EBREAK is never delegated
# and serves as the way back to M. A failure reports (step << 1) | 1.

.macro STEP n
    li s9, \n
.endm

# Expect a trap with this cause at this PC, taken by the S (1) or M (3)
# handler; resume at cont in mode priv (0 = U, 1 = S, 3 = M). The handler
# defaults to the resumed mode. Each handler rejects traps meant for the other.
.macro EXPECT cause, epc, cont, priv, handler=-1
    li s6, \cause
    la s4, \epc
    la s8, \cont
    li s10, \priv
    .if \handler == -1
    li s11, \priv
    .else
    li s11, \handler
    .endif
.endm

.macro EXPECT_INTERRUPT cause, epc, cont, priv
    li s6, 1
    slli s6, s6, 63
    ori s6, s6, \cause
    la s4, \epc
    la s8, \cont
    li s10, \priv
    li s11, \priv
.endm

# M-mode: return to target in mode priv through MRET.
.macro ENTER priv, target
    la t0, \target
    csrw mepc, t0
    li t0, 0x1800
    csrc mstatus, t0
    li t0, \priv << 11
    csrs mstatus, t0
    mret
.endm

# S-mode: return to target in U (SPP = 0) through SRET.
.macro SRET_TO_USER target
    la t0, \target
    csrw sepc, t0
    li t0, 0x100
    csrc sstatus, t0
    sret
.endm

.section .text.init,"ax",@progbits
.globl _start
_start:
    li s9, 100                 # setup failures must not report PASS
    li s6, -1
    la t0, m_trap
    csrw mtvec, t0
    la t0, s_trap
    csrw stvec, t0
    li t0, 0x3fffffff          # NAPOT over the whole physical space, RWX
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0

    # 1: MISA reports S and U; SATP is Bare only.
    STEP 1
    csrr t0, misa
    li t1, (1 << 18) | (1 << 20)
    and t2, t0, t1
    bne t2, t1, fail
    li t0, (8 << 60) | 0x1234
    csrw satp, t0
    csrr t0, satp
    bnez t0, fail

    # 2: S cannot access M CSRs; undelegated, the trap goes to M with MPP = S.
    STEP 2
    EXPECT 2, t2_mstatus, t2_done, 1, 3
    ENTER 1, t2_supervisor
t2_supervisor:
    csrr t0, sstatus           # S may read its own status
t2_mstatus:
    csrr t0, mstatus
    j fail
t2_done:
    csrr t0, sscratch
    EXPECT 3, t2_back, t2_machine, 3
t2_back:
    ebreak
    j fail
t2_machine:
    srli t0, s3, 11            # MPP at the EBREAK: it came from S
    andi t0, t0, 3
    li t1, 1
    bne t0, t1, fail

    # 3: delegated illegal instruction and U ECALL; STVAL holds the encoding.
    STEP 3
    li t0, (1 << 2) | (1 << 8)
    csrw medeleg, t0
    EXPECT 2, t3_mstatus, t3_continue, 1
    ENTER 1, t3_supervisor
t3_supervisor:
t3_mstatus:
    csrr t0, mstatus
    j fail
t3_continue:
    li t0, 0x300022f3          # csrr t0, mstatus
    bne s5, t0, fail
    andi t0, s3, 0x100         # SPP = S at the trap
    beqz t0, fail
    EXPECT 8, t3_ecall, t3_after_user, 1
    SRET_TO_USER t3_user
t3_user:
t3_ecall:
    ecall
    j fail
t3_after_user:
    andi t0, s3, 0x100         # SPP = U at the trap
    bnez t0, fail

    # 4: U cannot access supervisor CSRs; the trap is delegated to S.
    STEP 4
    EXPECT 2, t4_sstatus, t4_done, 1
    SRET_TO_USER t4_sstatus
t4_sstatus:
    csrr t0, sstatus
    j fail
t4_done:

    # 5: S ECALL is delegated only when MEDELEG[9] is set.
    STEP 5
    EXPECT 9, t5_ecall, t5_machine, 3
t5_ecall:
    ecall
    j fail
t5_machine:
    li t0, 1 << 9
    csrs medeleg, t0
    EXPECT 9, t5_delegated, t5_done, 1
    ENTER 1, t5_delegated
t5_delegated:
    ecall
    j fail
t5_done:
    EXPECT 3, t5_back, t5_cleared, 3
t5_back:
    ebreak
    j fail
t5_cleared:
    li t0, 1 << 9
    csrc medeleg, t0

    # 6: SFENCE.VMA and WFI are legal in S until TVM/TW trap them; TSR traps
    # SRET, and TVM also traps SATP.
    STEP 6
    li t0, 7 << 20
    csrc mstatus, t0
    ENTER 1, t6_legal
t6_legal:
    sfence.vma
    wfi
    csrr t0, satp
    EXPECT 3, t6_legal_back, t6_set, 3
t6_legal_back:
    ebreak
    j fail
t6_set:
    li t0, 3 << 20             # TVM and TW; the S handler itself needs SRET
    csrs mstatus, t0
    EXPECT 2, t6_satp, t6_sfence_next, 1
    ENTER 1, t6_satp
t6_satp:
    csrr t0, satp
    j fail
t6_sfence_next:
    EXPECT 2, t6_sfence, t6_wfi_next, 1
t6_sfence:
    sfence.vma
    j fail
t6_wfi_next:
    EXPECT 2, t6_wfi, t6_done, 1
t6_wfi:
    wfi
    j fail
t6_done:
    EXPECT 3, t6_back, t6_tsr, 3
t6_back:
    ebreak
    j fail
t6_tsr:
    # TSR traps SRET in S. Undelegate the illegal instruction so M handles it.
    li t0, 3 << 20
    csrc mstatus, t0
    li t0, 1 << 22
    csrs mstatus, t0
    li t0, 1 << 2
    csrc medeleg, t0
    EXPECT 2, t6_sret, t6_clear, 3
    ENTER 1, t6_sret
t6_sret:
    sret
    j fail
t6_clear:
    li t0, 1 << 22
    csrc mstatus, t0
    li t0, 1 << 2
    csrs medeleg, t0
    # M may still use SFENCE.VMA.
    sfence.vma

    # 7: counters below M: S needs MCOUNTEREN, U needs both enables.
    STEP 7
    li t0, 0b101
    csrw mcounteren, t0
    csrw scounteren, zero
    ENTER 1, t7_supervisor
t7_supervisor:
    rdcycle t0
    rdinstret t0
    EXPECT 2, t7_cycle, t7_enable, 1
    SRET_TO_USER t7_cycle
t7_cycle:
    rdcycle t0
    j fail
t7_enable:
    li t0, 1
    csrw scounteren, t0
    EXPECT 2, t7_instret, t7_done, 1
    SRET_TO_USER t7_user
t7_user:
    rdcycle t0
t7_instret:
    rdinstret t0
    j fail
t7_done:

    # 8: a delegated supervisor timer interrupt waits for SIE in S.
    STEP 8
    EXPECT 3, t8_back, t8_machine, 3
t8_back:
    ebreak
    j fail
t8_machine:
    li t0, 1 << 5
    csrw mideleg, t0
    csrw mie, t0
    csrs mip, t0               # STIP is software-owned in M
    csrsi mstatus, 8           # delegated interrupts are never taken in M
    nop
    nop
    csrr t0, sip
    li t1, 1 << 5
    bne t0, t1, fail
    li t0, 2
    csrc sstatus, t0
    ENTER 1, t8_supervisor
t8_supervisor:
    nop
    nop
    EXPECT_INTERRUPT 5, t8_wait, t8_taken, 1
    csrsi sstatus, 2
t8_wait:
    j t8_wait
t8_taken:
    # 9: in U the delegated interrupt is taken even though SIE is clear.
    STEP 9
    li t0, 0x22                # SIE = SPIE = 0, so U runs with SIE clear
    csrc sstatus, t0
    EXPECT_INTERRUPT 5, t9_wait, t9_taken, 1
    SRET_TO_USER t9_wait
t9_wait:
    j t9_wait
t9_taken:
    andi t0, s3, 0x100         # interrupted U
    bnez t0, fail

    # 10: undelegated, the same interrupt goes from S to M despite MIE = 0.
    STEP 10
    EXPECT 3, t10_back, t10_machine, 3
t10_back:
    ebreak
    j fail
t10_machine:
    csrw mideleg, zero
    li t0, 0x88                # MIE = MPIE = 0, so S runs with MIE clear
    csrc mstatus, t0
    EXPECT_INTERRUPT 5, t10_wait, t10_taken, 3
    ENTER 1, t10_wait
t10_wait:
    j t10_wait
t10_taken:
    li t0, 1 << 5
    csrc mip, t0
    csrw mie, zero
    srli t0, s3, 11            # MPP at the interrupt
    andi t0, t0, 3
    li t1, 1
    bne t0, t1, fail

    # 11: MRET can enter U directly; U ECALL is still delegated to S.
    STEP 11
    EXPECT 8, t11_ecall, t11_supervisor, 1
    ENTER 0, t11_ecall
t11_ecall:
    ecall
    j fail
t11_supervisor:
    csrrw t0, sscratch, s9
    csrr t1, sscratch
    bne t1, s9, fail
    EXPECT 3, t11_back, t12_machine, 3
t11_back:
    ebreak
    j fail

    # 12: load and store access faults delegate by their own MEDELEG bits;
    # STVAL/MTVAL hold the address. Phase A delegates stores only, phase B
    # loads only.
t12_machine:
    STEP 12
    li t0, 0x200091ff          # NAPOT 4 KiB at 0x80024000, no permission
    csrw pmpaddr0, t0
    li t0, 0x3fffffff
    csrw pmpaddr1, t0
    li t0, 0x1f18
    csrw pmpcfg0, t0
    li s1, 0x80024008
    li t0, 1 << 7
    csrs medeleg, t0
    EXPECT 5, t12_load, t12_store_entry, 3
    ENTER 0, t12_load
t12_load:
    ld t0, 0(s1)
    j fail
t12_store_entry:
    bne s5, s1, fail
    EXPECT 7, t12_store, t12_phase_b, 1
    ENTER 0, t12_store
t12_store:
    sd zero, 0(s1)
    j fail
t12_phase_b:
    bne s5, s1, fail
    EXPECT 3, t12_back, t12_machine_b, 3
t12_back:
    ebreak
    j fail
t12_machine_b:
    li t0, 1 << 7
    csrc medeleg, t0
    li t0, 1 << 5
    csrs medeleg, t0
    EXPECT 7, t12_store_b, t12_load_entry, 3
    ENTER 0, t12_store_b
t12_store_b:
    sd zero, 0(s1)
    j fail
t12_load_entry:
    bne s5, s1, fail
    EXPECT 5, t12_load_b, t12_done, 1
    ENTER 0, t12_load_b
t12_load_b:
    ld t0, 0(s1)
    j fail
t12_done:
    bne s5, s1, fail
    EXPECT 3, t12_back_b, t13_machine, 3
t12_back_b:
    ebreak
    j fail

    # 13: atomic access faults follow the same bits: LR reports a load
    # fault (5), AMO a store fault (7). MEDELEG still delegates loads only.
t13_machine:
    STEP 13
    EXPECT 5, t13_lr, t13_amo_entry, 1
    ENTER 0, t13_lr
.option push
.option arch, +a
t13_lr:
    lr.d t0, (s1)
    j fail
t13_amo_entry:
    bne s5, s1, fail
    EXPECT 7, t13_amo, t13_swap, 3
    SRET_TO_USER t13_amo
t13_amo:
    amoadd.d t0, t0, (s1)
    j fail
t13_swap:
    bne s5, s1, fail
    li t0, 1 << 5
    csrc medeleg, t0
    li t0, 1 << 7
    csrs medeleg, t0
    EXPECT 7, t13_sc, t13_lr2_entry, 1
    ENTER 0, t13_sc
t13_sc:
    sc.d t0, t0, (s1)
    j fail
t13_lr2_entry:
    EXPECT 5, t13_lr2, t13_done, 3
    SRET_TO_USER t13_lr2
t13_lr2:
    lr.w t0, (s1)
    j fail
.option pop
t13_done:
    bne s5, s1, fail

    # 14: atomics to the boot memory pass PMP but fault in the atomic unit
    # (not atomic-capable): LR 5, others 7, delegated by their own bits.
    STEP 14
    li t0, 1 << 7
    csrc medeleg, t0
    li t0, 1 << 5
    csrs medeleg, t0
    la s1, _start
    EXPECT 5, t14_lr, t14_amo_entry, 1
    ENTER 0, t14_lr
.option push
.option arch, +a
t14_lr:
    lr.d t0, (s1)
    j fail
t14_amo_entry:
    bne s5, s1, fail
    EXPECT 7, t14_amo, t14_swap, 3
    SRET_TO_USER t14_amo
t14_amo:
    amoadd.d t0, t0, (s1)
    j fail
t14_swap:
    bne s5, s1, fail
    li t0, 1 << 5
    csrc medeleg, t0
    li t0, 1 << 7
    csrs medeleg, t0
    EXPECT 7, t14_sc, t14_lr2_entry, 1
    ENTER 0, t14_sc
t14_sc:
    sc.d t0, t0, (s1)
    j fail
t14_lr2_entry:
    bne s5, s1, fail
    EXPECT 5, t14_lr2, t14_done, 3
    SRET_TO_USER t14_lr2
t14_lr2:
    lr.w t0, (s1)
    j fail
.option pop
t14_done:
    bne s5, s1, fail

    # 15: misaligned atomics fault in the atomic unit as misaligned (LR 4,
    # others 6), again delegated by their own bits only.
    STEP 15
    li t0, 1 << 7
    csrc medeleg, t0
    li t0, 1 << 4
    csrs medeleg, t0
    li s1, 0x80026004          # permitted RAM, not doubleword aligned
    EXPECT 4, t15_lr, t15_amo_entry, 1
    ENTER 0, t15_lr
.option push
.option arch, +a
t15_lr:
    lr.d t0, (s1)
    j fail
t15_amo_entry:
    bne s5, s1, fail
    EXPECT 6, t15_amo, t15_swap, 3
    SRET_TO_USER t15_amo
t15_amo:
    amoadd.d t0, t0, (s1)
    j fail
t15_swap:
    bne s5, s1, fail
    li t0, 1 << 4
    csrc medeleg, t0
    li t0, 1 << 6
    csrs medeleg, t0
    EXPECT 6, t15_sc, t15_lr2_entry, 1
    ENTER 0, t15_sc
t15_sc:
    sc.d t0, t0, (s1)
    j fail
t15_lr2_entry:
    bne s5, s1, fail
    addi s1, s1, -2            # word LR two bytes past a word boundary
    EXPECT 4, t15_lr2, t15_done, 3
    SRET_TO_USER t15_lr2
t15_lr2:
    lr.w t0, (s1)
    j fail
.option pop
t15_done:
    bne s5, s1, fail
    j pass

pass:
    PASS
fail:
    li t0, 1
    slli t1, s9, 1
    or t1, t1, t0
    la t0, tohost
    sd t1, 0(t0)
1:  j 1b

# Handlers check the expectation, record xTVAL in s5 and the status at trap
# entry in s3, then resume. Each clears its previous-enable bit so a level
# interrupt is not retaken on return.
.balign 4
m_trap:
    li t0, 3
    bne s11, t0, m_mismatch
    csrr t0, mcause
    bne t0, s6, m_mismatch
    csrr t0, mepc
    bne t0, s4, m_mismatch
    csrr s5, mtval
    csrr s3, mstatus
    li s6, -1
    csrw mepc, s8
    li t0, 0x1880
    csrc mstatus, t0
    slli t0, s10, 11
    csrs mstatus, t0
    mret
m_mismatch:
    j fail

.balign 4
s_trap:
    li t0, 1
    bne s11, t0, fail
    csrr t0, scause
    bne t0, s6, fail
    csrr t0, sepc
    bne t0, s4, fail
    csrr s5, stval
    csrr s3, sstatus
    li s6, -1
    csrw sepc, s8
    li t0, 0x120
    csrc sstatus, t0
    slli t0, s10, 8
    csrs sstatus, t0
    sret
