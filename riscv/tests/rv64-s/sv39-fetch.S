.include "test_macros.inc"
# Sv39 instruction translation on the whole core. S code runs at its
# physical address through a 1 GiB identity leaf and calls small functions
# through 4 KiB mappings of separate physical pages: execute permission,
# U/S, A, invalid and wide PTEs, PMA and PMP of the frame, denied PTE reads,
# non-canonical PCs, ITLB replacement, SFENCE.VMA, and 32-bit instructions
# crossing into a page that is not the physically next one. Each trap is
# checked against an expectation (cause, xEPC, handler, xTVAL in s1).
# A failure reports (step << 1) | 1 through M.

.equ PTE_V, 0x01
.equ PTE_R, 0x02
.equ PTE_W, 0x04
.equ PTE_X, 0x08
.equ PTE_U, 0x10
.equ PTE_A, 0x40
.equ PTE_D, 0x80
.equ PTE_RWAD, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D
.equ PTE_XA, PTE_V | PTE_X | PTE_A

.equ ROOT,  0x80020000
.equ L1,    0x80021000
.equ L0F,   0x80022000       # code pages at VA 0x800000
.equ L1C,   0x80023000       # last positive gigabyte
.equ L0C,   0x80024000
.equ SATP,  (8 << 60) | (ROOT >> 12)
.equ USER_ALIAS, 0x40000000
.equ CODE, 0x800000

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

# Static PTE.
.macro PTE table, index, ppn, flags
    li t0, (\ppn << 10) | \flags
    li t1, \table + (\index * 8)
    sd t0, 0(t1)
.endm

# PTE for the physical page holding symbol.
.macro PTE_AT table, index, symbol, flags
    la t0, \symbol
    srli t0, t0, 12
    slli t0, t0, 10
    ori t0, t0, \flags
    li t1, \table + (\index * 8)
    sd t0, 0(t1)
.endm

# Call the function at virtual address va.
.macro CALL_VA va
    li t0, \va
    jalr ra, 0(t0)
.endm

# Call va expecting an instruction fault with this cause, xEPC epc and
# xTVAL tval, taken by S; resume after the call in S.
.macro CALL_FAULTS cause, va, epc, tval
    li s6, \cause
    li s4, \epc
    la s8, 99f
    li s10, 1
    li s11, 1
    li s1, \tval
    li t0, \va
    jalr ra, 0(t0)
    j fail
99: bne s5, s1, fail
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

.macro TO_M
    EXPECT 3, 97f, 96f, 3
97: ebreak
    j fail
96:
.endm

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
    li t0, 0x3fffffff
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0
    # Instruction access and page faults go to S.
    li t0, (1 << 1) | (1 << 12)
    csrw medeleg, t0

    li t0, ROOT
    li t1, ROOT + 0x5000
1:  sd zero, 0(t0)
    addi t0, t0, 8
    bltu t0, t1, 1b

    PTE ROOT, 0, (L1 >> 12), PTE_V
    PTE L1, 4, (L0F >> 12), PTE_V                 # VA 0x800000
    PTE ROOT, 2, 0x80000, PTE_RWAD | PTE_X        # 1 GiB identity
    PTE ROOT, 3, 0x80000, PTE_RWAD | PTE_X | PTE_U  # U alias
    PTE ROOT, 255, (L1C >> 12), PTE_V
    PTE L1C, 511, (L0C >> 12), PTE_V
    PTE_AT L0C, 511, fp_low, PTE_XA               # VA 0x3ffffff000
    # The page after it wraps to VPN[2] = 256: executable if wrongly reached.
    PTE ROOT, 256, 0x80000, PTE_RWAD | PTE_X

    PTE_AT L0F, 0, fp_func, PTE_XA
    PTE_AT L0F, 1, fp_func, PTE_RWAD              # not executable
    PTE_AT L0F, 2, fp_func, PTE_XA | PTE_U
    PTE_AT L0F, 4, fp_func, PTE_V | PTE_X         # A clear
    PTE L0F, 5, 0x100000, PTE_XA                  # frame beyond 32 bits
    PTE L0F, 6, 0x90000, PTE_XA                   # no memory there
    PTE_AT L0F, 7, fp_low, PTE_XA                 # crossing: low half
    PTE_AT L0F, 8, fp_high, PTE_XA                # crossing: high half
    PTE_AT L0F, 9, fp_low, PTE_XA
    PTE_AT L0F, 10, fp_high, PTE_V | PTE_R | PTE_A  # high half not executable
    PTE_AT L0F, 11, fp_low, PTE_XA
    PTE L0F, 12, 0x90000, PTE_XA                  # high half without memory
    PTE_AT L0F, 13, fp_low, PTE_XA                # 14: no mapping
    PTE_AT L0F, 18, fp_func, PTE_XA               # remapped in step 8
    PTE_AT L0F, 19, fp_func, PTE_XA
    PTE_AT L0F, 20, fp_func, PTE_XA
    PTE_AT L0F, 21, fp_func, PTE_XA
    PTE_AT L0F, 22, fp_low, PTE_XA | PTE_U        # crossing in U
    PTE_AT L0F, 23, fp_high, PTE_XA | PTE_U

    li t0, SATP
    csrw satp, t0

    # 1: S executes through a 4 KiB mapping of another physical page.
    STEP 1
    ENTER 1, t1_supervisor
t1_supervisor:
    li a0, 0
    CALL_VA CODE
    li t0, 1
    bne a0, t0, fail
    TO_M

    # 2: instruction page faults: no X, a U page from S, no mapping, A clear,
    # and a non-canonical PC.
    STEP 2
    ENTER 1, t2_supervisor
t2_supervisor:
    CALL_FAULTS 12, CODE + 0x1000, CODE + 0x1000, CODE + 0x1000
    CALL_FAULTS 12, CODE + 0x2000, CODE + 0x2000, CODE + 0x2000
    CALL_FAULTS 12, CODE + 0x3000, CODE + 0x3000, CODE + 0x3000
    CALL_FAULTS 12, CODE + 0x4000, CODE + 0x4000, CODE + 0x4000
    # Its low 39 bits would select an executable page.
    CALL_FAULTS 12, (1 << 39) + CODE, (1 << 39) + CODE, (1 << 39) + CODE
    TO_M

    # 3: instruction access faults: a frame beyond 32 bits (from the walk)
    # and a frame without memory (PMA).
    STEP 3
    ENTER 1, t3_supervisor
t3_supervisor:
    CALL_FAULTS 1, CODE + 0x5000, CODE + 0x5000, CODE + 0x5000
    CALL_FAULTS 1, CODE + 0x6000, CODE + 0x6000, CODE + 0x6000
    TO_M

    # 4: a 32-bit instruction crossing into a page that is not the physical
    # successor executes with the second page's high half. The second page
    # is a separate ITLB miss.
    STEP 4
    ENTER 1, t4_supervisor
t4_supervisor:
    li a0, 0
    CALL_VA CODE + 0x7ffe
    li t0, 0x123
    bne a0, t0, fail
    li a0, 0
    CALL_VA CODE + 0x7ffe          # both pages now in the ITLB
    li t0, 0x123
    bne a0, t0, fail
    TO_M

    # 5: faults on the second parcel report its address in xTVAL; xEPC is
    # the instruction's. The last positive page's successor is not canonical.
    STEP 5
    ENTER 1, t5_supervisor
t5_supervisor:
    CALL_FAULTS 12, CODE + 0x9ffe, CODE + 0x9ffe, CODE + 0xa000
    CALL_FAULTS 1, CODE + 0xbffe, CODE + 0xbffe, CODE + 0xc000
    CALL_FAULTS 12, CODE + 0xdffe, CODE + 0xdffe, CODE + 0xe000
    CALL_FAULTS 12, 0x3ffffffffe, 0x3ffffffffe, 0x4000000000
    TO_M

    # 6: U executes U pages, including a crossing instruction, and may not
    # execute S pages.
    STEP 6
    ENTER 1, t6_supervisor
t6_supervisor:
    li a0, 0
    la ra, t6_user_back
    li t0, USER_ALIAS
    add ra, ra, t0
    li t0, CODE + 0x2000
    csrw sepc, t0
    li t0, 0x100
    csrc sstatus, t0
    sret
t6_user_back:
    li t0, 1
    bne a0, t0, fail
    li a0, 0
    CALL_VA CODE + 0x16ffe
    li t0, 0x123
    bne a0, t0, fail
    li s6, 12                      # U calls an S page
    li s4, CODE
    la s8, t6_after
    li s10, 0
    li s11, 1
    li s1, CODE
    li t0, CODE
    jalr ra, 0(t0)
    j fail
t6_after:
    bne s5, s1, fail
    USER_TO_M

    # 7: PMP: no execute on the frame or on the crossing instruction's second
    # frame (but none needed on its physical successor), and a denied PTE
    # read during the walk.
    STEP 7
    la t0, fp_func
    srli t0, t0, 2
    ori t0, t0, 0x1ff
    csrw pmpaddr0, t0
    li t0, 0x3fffffff
    csrw pmpaddr1, t0
    li t0, 0x1f19                  # entry 0: R only
    csrw pmpcfg0, t0
    ENTER 1, t7_frame
t7_frame:
    CALL_FAULTS 1, CODE, CODE, CODE
    TO_M
    la t0, fp_high
    srli t0, t0, 2
    ori t0, t0, 0x1ff
    csrw pmpaddr0, t0
    ENTER 1, t7_second
t7_second:
    CALL_FAULTS 1, CODE + 0x7ffe, CODE + 0x7ffe, CODE + 0x8000
    TO_M
    la t0, fp_gap                  # the physical successor is not fetched
    srli t0, t0, 2
    ori t0, t0, 0x1ff
    csrw pmpaddr0, t0
    li t0, 0x1f18
    csrw pmpcfg0, t0
    ENTER 1, t7_gap
t7_gap:
    li a0, 0
    CALL_VA CODE + 0x7ffe
    li t0, 0x123
    bne a0, t0, fail
    TO_M
    li t0, (L0F >> 2) | 0x1ff
    csrw pmpaddr0, t0
    li t0, 0x1f18                  # entry 0: no access
    csrw pmpcfg0, t0
    ENTER 1, t7_table
t7_table:
    sfence.vma
    CALL_FAULTS 1, CODE, CODE, CODE
    TO_M
    li t0, 0x3fffffff
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0

    # 8: SFENCE.VMA makes a remapped code page visible; three pages called
    # in turn replace each other in the two-entry ITLB.
    STEP 8
    ENTER 1, t8_supervisor
t8_supervisor:
    li a0, 0
    CALL_VA CODE + 0x12000
    li t0, 1
    bne a0, t0, fail
    la t0, fp_alt
    srli t0, t0, 12
    slli t0, t0, 10
    ori t0, t0, PTE_XA
    li t1, L0F + 18 * 8
    sd t0, 0(t1)
    sfence.vma
    li a0, 0
    CALL_VA CODE + 0x12000
    li t0, 2
    bne a0, t0, fail
    li a0, 0
    li s2, 4
1:  CALL_VA CODE + 0x13000
    CALL_VA CODE + 0x14000
    CALL_VA CODE + 0x15000
    addi s2, s2, -1
    bnez s2, 1b
    li t0, 12
    bne a0, t0, fail
    TO_M
    j pass

pass:
    PASS
fail:
    li s7, 1
    ebreak
1:  j 1b

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

# Code pages, each in its own physical page. fp_low ends with the low half
# of "addi a0, a0, 0x123"; fp_gap, its physical successor, starts with the
# high half of "addi a0, a0, 0x7ff"; fp_high starts with the right one.
.section .text.pages,"ax",@progbits
.balign 4096
fp_func:
    addi a0, a0, 1
    ret
.balign 4096
fp_alt:
    addi a0, a0, 2
    ret
.balign 4096
fp_low:
    .skip 0xffe
    .half 0x0513
fp_gap:
    .half 0x7ff5
    ret
.balign 4096
fp_high:
    .half 0x1235
    ret
.balign 4096
