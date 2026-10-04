.include "test_macros.inc"
.section .text.init,"ax",@progbits
.globl _start
_start:
    la t0, trap_handler
    csrw mtvec,t0
    li s0,0x80024000
    li s1,0x12345678
    sd s1,0(s0)
    li s7,0
    srli t0,s0,2
    csrw pmpaddr0,t0
    li t0,0x11                 # unlocked NA4, R only
    csrw pmpcfg0,t0
    sw s1,0(s0)                # unlocked entry bypassed in M
    la s4,partial_m
    mv s5,s0
    li s6,5
partial_m:
    ld t0,0(s0)                # partial match fails even in M
    li t0,0x20000
    csrc mstatus,t0
    li t0,0x1800
    csrc mstatus,t0
    li t0,0x20000              # MPRV=1, MPP=U
    csrs mstatus,t0
    lw t1,0(s0)                # fetch remains in M; data obeys PMP R
    bne t1,s1,fail
    la s4,mprv_store
    mv s5,s0
    li s6,7
mprv_store:
    sw zero,0(s0)
    # Trap clears MPRV. M-mode can still write an unlocked R-only entry.
    sw s1,0(s0)
    la s4,mprv_unmatched
    addi s5,s0,4
    li s6,5
    li t0,0x1800
    csrc mstatus,t0
    li t0,0x20000
    csrs mstatus,t0
mprv_unmatched:
    lw t1,4(s0)
    la s4,locked_store
    mv s5,s0
    li s6,7
    li t0,0x91                 # lock NA4 R-only region
    csrw pmpcfg0,t0
locked_store:
    sw zero,0(s0)              # immediately consume the new lock/permissions
    li t0,0x91
    csrw pmpcfg0,zero          # lock cannot be cleared
    csrw pmpaddr0,zero
    csrr t1,pmpcfg0
    bne t0,t1,fail
    srli t0,s0,2
    csrr t1,pmpaddr0
    bne t0,t1,fail
    lw t1,0(s0)
    bne t1,s1,fail
    li t0,4
    bne s7,t0,fail
    PASS
trap_handler:
    csrr t0,mcause
    bne t0,s6,fail
    csrr t0,mepc
    bne t0,s4,fail
    csrr t0,mtval
    bne t0,s5,fail
    addi s7,s7,1
    li t0,0x20000
    csrc mstatus,t0
    addi t0,s4,4
    csrw mepc,t0
    mret
fail:
    li t0,0x20000
    csrc mstatus,t0
    FAIL 1
