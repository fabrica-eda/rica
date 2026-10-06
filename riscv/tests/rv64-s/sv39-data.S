.include "test_macros.inc"
# Sv39 data translation on the whole core. M builds page tables in RAM, then
# S, U and M with MPRV access data through them. S code runs at its physical
# address through a 1 GiB identity leaf; U code runs through a U alias of the
# same memory, USER_ALIAS above it. Each
# trap is checked against an expectation (cause, xEPC, handler, xTVAL = the
# virtual address in s1) as in supervisor.S. A failure reports
# (step << 1) | 1; failures in S or U reach M through EBREAK first.

.equ PTE_V, 0x01
.equ PTE_R, 0x02
.equ PTE_W, 0x04
.equ PTE_X, 0x08
.equ PTE_U, 0x10
.equ PTE_A, 0x40
.equ PTE_D, 0x80
.equ PTE_RWAD, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D

.equ ROOT,   0x80020000
.equ L1,     0x80021000
.equ L0,     0x80022000
.equ ROOT2,  0x80023000
.equ PAGE_A, 0x80030000
.equ PAGE_B, 0x80031000
.equ SATP,   (8 << 60) | (ROOT >> 12)
.equ SATP2,  (8 << 60) | (ROOT2 >> 12)
.equ USER_ALIAS, 0x40000000

.equ MAGIC_A0, 0x0807060504030201
.equ MAGIC_A1, 0x100f0e0d0c0b0a09
.equ MAGIC_B,  0x1122334455667788

.macro STEP n
    li s9, \n
.endm

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

# One instruction expected to trap with xTVAL = s1, then resume after it in
# mode priv.
.macro TRAPS cause, priv, handler, insn:vararg
    EXPECT \cause, 98f, 99f, \priv, \handler
98: \insn
    j fail
99: bne s5, s1, fail
.endm

# Load the doubleword at virtual address va (into s1) and compare it.
.macro LOAD_IS va, value
    li s1, \va
    ld t0, 0(s1)
    li t1, \value
    bne t0, t1, fail
.endm

.macro PTE table, index, ppn, flags
    li t0, (\ppn << 10) | \flags
    li t1, \table + (\index * 8)
    sd t0, 0(t1)
.endm

.macro ENTER priv, target
    la t0, \target
    csrw mepc, t0
    li t0, 0x1800
    csrc mstatus, t0
    li t0, \priv << 11
    csrs mstatus, t0
    mret
.endm

# S-mode: enter U at the U alias of target.
.macro SRET_TO_USER target
    la t0, \target
    li t1, USER_ALIAS
    add t0, t0, t1
    csrw sepc, t0
    li t0, 0x100
    csrc sstatus, t0
    sret
.endm

# Return to M from S or U.
.macro TO_M
    EXPECT 3, 97f, 96f, 3
97: ebreak
    j fail
96:
.endm

# Return to M from U code running at the U alias; M resumes at the
# physical address.
.macro USER_TO_M
    EXPECT 3, 97f, 96f, 3
    li t0, USER_ALIAS
    sub s8, s8, t0
97: ebreak
    j fail
96:
.endm

.section .text.init,"ax",@progbits
.globl _start
_start:
    li s9, 100
    li s7, 0
    li s6, -1
    la t0, m_trap
    csrw mtvec, t0
    la t0, s_trap
    csrw stvec, t0
    li t0, 0x3fffffff          # NAPOT over the whole physical space, RWX
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0
    # Page faults, access faults and misaligned accesses go to S.
    li t0, (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 13) | (1 << 15)
    csrw medeleg, t0

    # Clear the four table pages, then build the tables and test pages.
    li t0, ROOT
    li t1, ROOT + 0x4000
1:  sd zero, 0(t0)
    addi t0, t0, 8
    bltu t0, t1, 1b
    li t0, PAGE_A
    li t1, MAGIC_A0
    sd t1, 0(t0)
    li t1, MAGIC_A1
    sd t1, 8(t0)
    li t1, 5
    sd t1, 0x28(t0)
    li t1, 0x66
    sd t1, 0x30(t0)
    li t0, PAGE_B
    li t1, MAGIC_B
    sd t1, 0(t0)

    PTE ROOT, 0, (L1 >> 12), PTE_V
    PTE L1, 0, (L0 >> 12), PTE_V
    PTE L0, 1, 0x80030, PTE_RWAD                         # A
    PTE L0, 2, 0x80030, PTE_V | PTE_R | PTE_A            # A, read-only
    PTE L0, 3, 0x80031, PTE_RWAD | PTE_U                 # B, user
    PTE L0, 4, 0x80030, PTE_V | PTE_R | PTE_W | PTE_D    # A clear
    PTE L0, 5, 0x80030, PTE_V | PTE_R | PTE_W | PTE_A    # D clear
    PTE L0, 6, 0x80030, PTE_V | PTE_X | PTE_A            # execute-only
    PTE L0, 8, 0x100000, PTE_RWAD                        # PPN beyond 32 bits
    PTE L0, 9, 0x90000, PTE_RWAD                         # no memory there
    li t0, (1 << 54) | (0x80030 << 10) | PTE_RWAD        # reserved bit
    li t1, L0 + 10 * 8
    sd t0, 0(t1)
    PTE L0, 11, 0x80030, PTE_V | PTE_W | PTE_A | PTE_D   # W without R
    PTE L0, 12, 0x80030, PTE_V                           # pointer at level 0
    PTE L0, 13, 0x80030, PTE_V | PTE_R | PTE_A | PTE_D   # dirty, not writable
    PTE L0, 0x30, 0x80031, PTE_RWAD                      # B
    PTE L1, 1, 0x80000, PTE_RWAD                         # 2 MiB at RAM base
    PTE L1, 2, 0x80001, PTE_RWAD                         # misaligned 2 MiB
    # W without R above level 0; as a pointer it would reach ROOT2's leaf.
    PTE L1, 3, (ROOT2 >> 12), PTE_V | PTE_W | PTE_A | PTE_D
    PTE ROOT, 2, 0x80000, PTE_RWAD | PTE_X               # 1 GiB identity
    PTE ROOT, 3, 0x80000, PTE_RWAD | PTE_X | PTE_U       # U alias of it
    PTE ROOT, 4, 0x90000, PTE_V                          # table with no memory
    PTE ROOT2, 0, 0x80000, PTE_RWAD                      # 1 GiB at RAM base
    PTE ROOT2, 2, 0x80000, PTE_RWAD | PTE_X              # code, as in ROOT

    # 1: SATP holds Sv39 and the root page; other modes, a PPN beyond 32-bit
    # physical space, and ASID bits do not change it.
    STEP 1
    li t0, SATP
    csrw satp, t0
    csrr t1, satp
    bne t0, t1, fail
    li t2, (9 << 60) | (ROOT2 >> 12)
    csrw satp, t2
    csrr t1, satp
    bne t0, t1, fail
    li t2, (8 << 60) | (1 << 20)
    csrw satp, t2
    csrr t1, satp
    bne t0, t1, fail
    li t2, SATP2 | (0xffff << 44)
    csrw satp, t2
    csrr t1, satp
    li t2, SATP2
    bne t1, t2, fail
    li t0, SATP
    csrw satp, t0
    # M without MPRV is not translated.
    li t0, PAGE_B
    ld t1, 0(t0)
    li t2, MAGIC_B
    bne t1, t2, fail

    # 2: S loads and stores through 4 KiB, 2 MiB and 1 GiB leaves, including
    # a misaligned access within a page.
    STEP 2
    ENTER 1, t2_supervisor
t2_supervisor:
    LOAD_IS 0x1000, MAGIC_A0
    LOAD_IS 0x1008, MAGIC_A1
    li s1, 0x1006
    lw t0, 0(s1)
    li t1, 0x0a090807
    bne t0, t1, fail
    li s1, 0x1020
    li t0, 0x5a5a
    sd t0, 0(s1)
    LOAD_IS 0x230000, MAGIC_A0
    LOAD_IS 0x230020, 0x5a5a
    LOAD_IS 0x80031000, MAGIC_B
    TO_M
    li t0, PAGE_A
    ld t0, 0x20(t0)
    li t1, 0x5a5a
    bne t0, t1, fail

    # 3: page faults, delegated to S. Loads fill the TLB, so the following
    # store is denied by a hit.
    STEP 3
    ENTER 1, t3_supervisor
t3_supervisor:
    LOAD_IS 0x2000, MAGIC_A0
    li s1, 0x2010
    TRAPS 15, 1, 1, sd t0, 0(s1)
    li s1, 0x7000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0x4000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0x4008
    TRAPS 15, 1, 1, sd t0, 0(s1)
    LOAD_IS 0x5000, MAGIC_A0
    li s1, 0x5008
    TRAPS 15, 1, 1, sd t0, 0(s1)
    li s1, 0xa000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0xb000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0x600000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    LOAD_IS 0xd000, MAGIC_A0
    li s1, 0xd008
    TRAPS 15, 1, 1, sd t0, 0(s1)
    li s1, 0xc000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0x400000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 1 << 39             # not sign-extended from bit 38
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, (1 << 39) + 0x1000
    TRAPS 15, 1, 1, sd t0, 0(s1)
    li s1, -(1 << 38)          # canonical, but no mapping
    TRAPS 13, 1, 1, ld t0, 0(s1)
    TO_M

    # 4: without MEDELEG[13] the page fault goes to M; MTVAL is the VA.
    STEP 4
    li t0, 1 << 13
    csrc medeleg, t0
    ENTER 1, t4_supervisor
t4_supervisor:
    li s1, 0x7008
    TRAPS 13, 1, 3, ld t0, 0(s1)
    TO_M
    li t0, 1 << 13
    csrs medeleg, t0

    # 5: U pages, SUM and MXR.
    STEP 5
    ENTER 1, t5_supervisor
t5_supervisor:
    li s1, 0x3000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li t0, 1 << 18             # SUM
    csrs sstatus, t0
    LOAD_IS 0x3000, MAGIC_B
    li t0, 1 << 18
    csrc sstatus, t0
    li s1, 0x6000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    li s1, 0x6000
    TRAPS 15, 1, 1, sd t0, 0(s1)
    li t0, 1 << 19             # MXR
    csrs sstatus, t0
    LOAD_IS 0x6000, MAGIC_A0
    li t0, 1 << 19
    csrc sstatus, t0
    li s1, 0x6000
    TRAPS 13, 1, 1, ld t0, 0(s1)
    SRET_TO_USER t5_user
t5_user:
    LOAD_IS 0x3000, MAGIC_B
    li s1, 0x1000
    TRAPS 13, 0, 1, ld t0, 0(s1)
    USER_TO_M

    # 6: access faults: a leaf PPN beyond 32 bits, a leaf or a PTE without
    # memory, and PMP denying the final access or a PTE read.
    STEP 6
    ENTER 1, t6_supervisor
t6_supervisor:
    li s1, 0x8000
    TRAPS 5, 1, 1, ld t0, 0(s1)
    li s1, 0x9000
    TRAPS 5, 1, 1, ld t0, 0(s1)
    li s1, 0x9008
    TRAPS 7, 1, 1, sd t0, 0(s1)
    li s1, 1 << 32
    TRAPS 5, 1, 1, ld t0, 0(s1)
    TO_M
    li t0, (PAGE_A >> 2) | 0x1ff   # entry 0: page A, no access
    csrw pmpaddr0, t0
    li t0, 0x3fffffff
    csrw pmpaddr1, t0
    li t0, 0x1f18
    csrw pmpcfg0, t0
    ENTER 1, t6_pmp_page
t6_pmp_page:
    li s1, 0x1000
    TRAPS 5, 1, 1, ld t0, 0(s1)
    li s1, 0x1010
    TRAPS 7, 1, 1, sd t0, 0(s1)
    LOAD_IS 0x30000, MAGIC_B
    TO_M
    li t0, (L0 >> 2) | 0x1ff       # entry 0: the level-0 table
    csrw pmpaddr0, t0
    ENTER 1, t6_pmp_table
t6_pmp_table:
    li s1, 0x5000
    TRAPS 5, 1, 1, ld t0, 0(s1)
    LOAD_IS 0x230000, MAGIC_A0     # 2 MiB leaf: no level-0 read
    TO_M
    li t0, 0x3fffffff
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0

    # 7: a data access crossing a page is address-misaligned.
    STEP 7
    ENTER 1, t7_supervisor
t7_supervisor:
    li s1, 0x1ffc
    TRAPS 4, 1, 1, ld t0, 0(s1)
    li s1, 0x1ffe
    TRAPS 6, 1, 1, sw t0, 0(s1)
    TO_M

    # 8: translated atomics.
    STEP 8
    ENTER 1, t8_supervisor
.option push
.option arch, +a
t8_supervisor:
    li s1, 0x1028
    li t1, 3
    amoadd.d t0, t1, (s1)
    li t1, 5
    bne t0, t1, fail
    li s1, 0x1030
    lr.d t0, (s1)
    li t1, 0x66
    bne t0, t1, fail
    li t1, 0x77
    sc.d t0, t1, (s1)
    bnez t0, fail
    li s1, 0x2008
    TRAPS 15, 1, 1, amoswap.d t0, t0, (s1)
    li s1, 0x4010
    TRAPS 13, 1, 1, lr.d t0, (s1)
    li s1, 0x9010
    TRAPS 7, 1, 1, amoadd.d t0, t0, (s1)
    li s1, 0x1002
    TRAPS 4, 1, 1, lr.w t0, (s1)
.option pop
    TO_M
    li t0, PAGE_A
    ld t1, 0x28(t0)
    li t2, 8
    bne t1, t2, fail
    ld t1, 0x30(t0)
    li t2, 0x77
    bne t1, t2, fail

    # 9: M with MPRV translates loads and stores with MPP's privilege; a
    # fault from M stays in M.
    STEP 9
    li t0, 0x1800
    csrc mstatus, t0
    li t0, (1 << 17) | (1 << 11)   # MPRV, MPP = S
    csrs mstatus, t0
    LOAD_IS 0x1000, MAGIC_A0
    li s1, 0x2000
    TRAPS 15, 3, 3, sd t0, 0(s1)
    # MRET back to M leaves MPP = U and MPRV set: accesses now use U.
    csrr t0, mstatus
    li t1, (1 << 17) | 0x1800
    and t0, t0, t1
    li t1, 1 << 17
    bne t0, t1, fail
    LOAD_IS 0x3000, MAGIC_B
    li s1, 0x1000
    TRAPS 13, 3, 3, ld t0, 0(s1)
    li t0, 1 << 17
    csrc mstatus, t0
    li t0, PAGE_B                  # MPRV clear: not translated
    ld t1, 0(t0)
    li t2, MAGIC_B
    bne t1, t2, fail

    # 10: SFENCE.VMA and SATP writes discard stale translations. M rewrites
    # a PTE without touching the TLB, then S rewrites one through the 1 GiB
    # identity mapping.
    STEP 10
    ENTER 1, t10_supervisor
t10_supervisor:
    LOAD_IS 0x1000, MAGIC_A0
    TO_M
    PTE L0, 1, 0x80031, PTE_RWAD
    ENTER 1, t10_sfence
t10_sfence:
    sfence.vma
    LOAD_IS 0x1000, MAGIC_B
    li s2, L0 + 0x30 * 8
    li t1, (0x80030 << 10) | PTE_RWAD
    sd t1, 0(s2)
    sfence.vma
    LOAD_IS 0x30000, MAGIC_A0
    li t1, (0x80031 << 10) | PTE_RWAD
    sd t1, 0(s2)
    sfence.vma
    LOAD_IS 0x30000, MAGIC_B
    li t0, SATP2
    csrw satp, t0
    LOAD_IS 0x30000, MAGIC_A0      # ROOT2: VA + 0x80000000
    li t0, SATP
    csrw satp, t0
    LOAD_IS 0x30000, MAGIC_B
    csrw satp, zero                # Bare: S is not translated
    li s1, PAGE_A
    ld t0, 0(s1)
    li t1, MAGIC_A0
    bne t0, t1, fail
    TO_M
    j pass

pass:
    PASS
fail:
    li s7, 1
    ebreak
1:  j 1b

# M reports a failure with MPRV clear, from whichever mode failed.
m_report:
    li t0, 1 << 17
    csrc mstatus, t0
    li t0, 1
    slli t1, s9, 1
    or t1, t1, t0
    la t0, tohost
    sd t1, 0(t0)
1:  j 1b

.balign 4
m_trap:
    bnez s7, m_report
    li t0, 3
    bne s11, t0, fail
    csrr t0, mcause
    bne t0, s6, fail
    csrr t0, mepc
    bne t0, s4, fail
    csrr s5, mtval
    csrr s3, mstatus
    li s6, -1
    csrw mepc, s8
    li t0, 0x1880
    csrc mstatus, t0
    slli t0, s10, 11
    csrs mstatus, t0
    mret

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
