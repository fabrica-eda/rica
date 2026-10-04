.include "test_macros.inc"
# PMP permission-cache diagnostics. Every step checks an architectural PMP
# result that a stale, wrongly tagged or wrongly indexed page entry would
# change: privilege tags, PMP writes, signed-offset neighbor pages, pages
# split by a PMP boundary, and instructions crossing out of a cached page.
# A failure reports (step << 1) | 1 through tohost.

.macro STEP n
    li s9, \n
.endm

# Expected trap cause, faulting PC and M-mode continuation. s5 holds mtval.
.macro EXPECT cause, epc, cont
    li s6, \cause
    la s4, \epc
    la s8, \cont
.endm

# Loads/stores use U permissions while fetch remains in M.
.macro USER_DATA
    li t0, 0x1800
    csrc mstatus, t0
    li t0, 0x20000
    csrs mstatus, t0
.endm

.macro MACHINE_DATA
    li t0, 0x20000
    csrc mstatus, t0
.endm

.macro ENTER_USER target
    la t0, \target
    csrw mepc, t0
    li t0, 0x1800
    csrc mstatus, t0
    mret
.endm

.section .text.init,"ax",@progbits
.globl _start
_start:
    la t0, trap_handler
    csrw mtvec, t0
    li s7, 0
    li s9, 0
    li a0, 0x80024000          # data page P
    li a1, 0x80023ff8          # last doubleword of P-1
    li a2, 0x80025000          # first doubleword of P+1
    li t2, 0xff8
    add a3, a0, t2             # last doubleword of P
    li s1, 0x5a5a5a5a12345678
    sd s1, 0(a0)
    sd s1, 8(a0)
    sd s1, 0(a3)
    sd s1, 0(a1)
    sd s1, 0(a2)
    li t0, 0x200091ff          # NAPOT 4 KiB at P
    csrw pmpaddr0, t0
    la t0, user_page
    srli t0, t0, 2
    ori t0, t0, 0x1ff
    csrw pmpaddr1, t0
    la t0, machine_page
    srli t0, t0, 2
    ori t0, t0, 0x1ff
    csrw pmpaddr2, t0
    li t0, 0x20009004          # NA4 at P+0x10
    csrw pmpaddr3, t0

    # 1: an M-mode data fill must not satisfy a later U-permission access.
    STEP 1
    li t0, 0x191d18            # P: no U access; U page RX; M page R
    csrw pmpcfg0, t0
    ld t1, 0(a0)               # unlocked entry does not restrict M
    bne t1, s1, fail
    USER_DATA
    EXPECT 5, t1_user_load, t1_done
    mv s5, a0
t1_user_load:
    ld t1, 0(a0)
    j fail
t1_done:

    # 2: PMP writes invalidate a cached data page, including the next access.
    STEP 2
    li t0, 0x191d1b            # P: U RW
    csrw pmpcfg0, t0
    USER_DATA
    ld t1, 0(a0)
    bne t1, s1, fail
    sd s1, 8(a0)
    EXPECT 7, t2_store, t2_after_store
    addi s5, a0, 8
    li t0, 0x191d19            # P: U R
    csrw pmpcfg0, t0
t2_store:
    sd s1, 8(a0)
    j fail
t2_after_store:
    USER_DATA
    ld t1, 8(a0)
    bne t1, s1, fail
    EXPECT 5, t2_load, t2_done
    mv s5, a0
    li t0, 0x191d18            # P: no U access
    csrw pmpcfg0, t0
t2_load:
    ld t1, 0(a0)
    j fail
t2_done:

    # 3: the signed displacement selects P-1, P or P+1 against the cached P.
    STEP 3
    li t0, 0x191d1b
    csrw pmpcfg0, t0
    USER_DATA
    ld t1, 0(a0)               # cache P
    bne t1, s1, fail
    ld t1, -8(a2)              # base in P+1, effective last doubleword of P
    bne t1, s1, fail
    ld t1, 8(a1)               # base in P-1, effective first doubleword of P
    bne t1, s1, fail
    ld t1, 0(a3)               # last byte at the end of P
    bne t1, s1, fail
    EXPECT 5, t3_up, t3_up_done
    mv s5, a2
t3_up:
    ld t1, 8(a3)               # base in cached P, effective P+1
    j fail
t3_up_done:
    USER_DATA
    ld t1, 0(a0)               # cache P again
    bne t1, s1, fail
    EXPECT 5, t3_down, t3_down_done
    addi s5, a0, -8
t3_down:
    ld t1, -8(a0)              # base in cached P, effective P-1
    j fail
t3_down_done:

    # 4: a page split by a PMP boundary keeps exact per-access checks.
    STEP 4
    li t0, 0x11191d00          # P entry OFF, NA4 U R at P+0x10
    csrw pmpcfg0, t0
    USER_DATA
    lw t1, 0x10(a0)
    EXPECT 5, t4_load, t4_done
    addi s5, a0, 0x14
t4_load:
    lw t1, 0x14(a0)
    j fail
t4_done:
    USER_DATA
    lw t1, 0x10(a0)
    MACHINE_DATA

    # 5: removing X invalidates the cached U fetch page.
    STEP 5
    li t0, 0x191d1b
    csrw pmpcfg0, t0
    EXPECT 8, user_ecall, t5_after_ecall
    li s5, 0
    ENTER_USER user_ecall
    j fail
t5_after_ecall:
    EXPECT 1, user_ecall, t5_done
    la s5, user_ecall
    li t0, 0x19191b            # U page: R only
    csrw pmpcfg0, t0
    ENTER_USER user_ecall
    j fail
t5_done:

    # 6: an M-mode fetch fill must not satisfy a U fetch from the same page.
    STEP 6
    li t0, 0x191d1b            # M page: U R only, unlocked
    csrw pmpcfg0, t0
    j machine_entry
t6_done:

    # 7: a 32-bit instruction leaving a cached executable page checks both
    # parcels; mtval names the denied second parcel.
    STEP 7
    EXPECT 1, user_cross, t7_done
    la s5, user_cross
    addi s5, s5, 2
    ENTER_USER user_cross_prefix
    j fail
t7_done:

    # Locks are reset-only, so the M-mode lock checks run last.
    # 8: locking a cached M data page R-only denies the next store.
    STEP 8
    MACHINE_DATA
    ld t1, 0(a0)
    bne t1, s1, fail
    sd s1, 8(a0)
    EXPECT 7, t8_store, t8_done
    addi s5, a0, 8
    li t0, 0x191d99            # P: locked R
    csrw pmpcfg0, t0
t8_store:
    sd s1, 8(a0)
    j fail
t8_done:
    ld t1, 0(a0)
    bne t1, s1, fail

    # 9: locking the executing M page without X denies its next fetch.
    STEP 9
    j machine_lock
t9_done:

    li t0, 12
    bne s7, t0, fail
    PASS

trap_handler:
    csrr t0, mcause
    bne t0, s6, fail
    csrr t0, mepc
    bne t0, s4, fail
    csrr t0, mtval
    bne t0, s5, fail
    addi s7, s7, 1
    li t0, 0x20000
    csrc mstatus, t0
    li t0, 0x1800
    csrs mstatus, t0           # continue in M
    csrw mepc, s8
    mret

fail:
    li t0, 0x20000
    csrc mstatus, t0
    la t0, tohost
    slli t1, s9, 1
    ori t1, t1, 1
    sd t1, 0(t0)
1:  j 1b

.section .machine_page,"ax",@progbits
.balign 4096
machine_page:
machine_entry:
    EXPECT 1, machine_target, t6_done
    la s5, machine_target
    ENTER_USER machine_target
machine_target:
    nop
    j fail
machine_lock:
    EXPECT 1, machine_locked_fetch, t9_done
    la s5, machine_locked_fetch
    li t0, 0x991d99            # M page: locked R, no X
    csrw pmpcfg0, t0
machine_locked_fetch:
    nop
    j fail

.section .user_page,"ax",@progbits
.balign 4096
user_page:
user_ecall:
    ecall
    j user_ecall
.org 0xffc
user_cross_prefix:
.option push
.option rvc
    c.nop
.option pop
user_cross:
    .2byte 0x0073              # first parcel of a 32-bit ECALL
