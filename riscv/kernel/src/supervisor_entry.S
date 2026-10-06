# Minimal M firmware, the S-kernel trap path and the isolated U task.
#
# M delegates every U exception the kernel handles and the S timer
# interrupt, then enters S. S calls M through ECALL for two services:
# a7 = 0 programs the next timer interrupt at a0 (and clears STIP),
# a7 = 1 reports a0 through tohost and stops. The M timer interrupt is
# forwarded to S as STIP, SBI style.

.equ MTIMECMP, 0x02004000
.equ MEDELEG, (1 << 1) | (1 << 2) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 12) | (1 << 13) | (1 << 15)
.equ MIDELEG, (1 << 1) | (1 << 5)

.section .text.init,"ax",@progbits
.globl _start
_start:
    csrw mie, zero
    csrci mstatus, 8
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    la t0, __bss_start
    la t1, __bss_end
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:  la t0, __machine_stack_top
    csrw mscratch, t0
    la t0, machine_trap
    csrw mtvec, t0
    # S and U may reach all physical memory; Sv39 isolates them.
    li t0, 0x3fffffff
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0
    li t0, MEDELEG
    csrw medeleg, t0
    li t0, MIDELEG
    csrw mideleg, t0
    csrw mcounteren, zero
    li t0, 0x1800
    csrc mstatus, t0
    li t0, 0x800
    csrs mstatus, t0
    la t0, supervisor_start
    csrw mepc, t0
    mret

.balign 4
machine_trap:
    csrrw sp, mscratch, sp
    sd t0, 0(sp)
    sd t1, 8(sp)
    csrr t0, mcause
    bltz t0, machine_interrupt
    li t1, 9
    bne t0, t1, machine_fatal
    bnez a7, machine_finish
    li t0, MTIMECMP
    sd a0, 0(t0)
    li t0, 1 << 5
    csrc mip, t0
    li t0, 1 << 7
    csrs mie, t0
    csrr t0, mepc
    addi t0, t0, 4
    csrw mepc, t0
    j machine_return
machine_interrupt:
    slli t0, t0, 1
    li t1, 7 << 1
    bne t0, t1, machine_fatal
    li t0, 1 << 7
    csrc mie, t0
    li t0, 1 << 5
    csrs mip, t0
machine_return:
    ld t0, 0(sp)
    ld t1, 8(sp)
    csrrw sp, mscratch, sp
    mret
machine_fatal:
    li a0, (200 << 1) | 1
machine_finish:
    la t0, tohost
    sd a0, 0(t0)
1:  j 1b

.section .text,"ax",@progbits
.globl supervisor_start
supervisor_start:
    la sp, __boot_stack_top
    la t0, supervisor_trap_entry
    csrw stvec, t0
    call supervisor_main
1:  j 1b

.section .text.trap,"ax",@progbits
.balign 4
supervisor_trap_entry:
    # SSCRATCH always holds a kernel-owned stack while U runs. No register
    # is stored through the untrusted interrupted SP.
    csrrw sp, sscratch, sp
    addi sp, sp, -288
    sd zero, 0(sp)
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        sd x\r,(8*\r)(sp)
    .endr
    csrr t0, sscratch
    sd t0, 16(sp)
    csrr t0, sepc
    sd t0, 256(sp)
    csrr t0, sstatus
    sd t0, 264(sp)
    csrr t0, scause
    sd t0, 272(sp)
    csrr t0, stval
    sd t0, 280(sp)
    mv a0, sp
    call supervisor_trap
.globl supervisor_enter
supervisor_enter:
    mv sp, a0
    ld t0, 256(sp)
    csrw sepc, t0
    ld t0, 264(sp)
    csrw sstatus, t0
    la t0, __boot_stack_top
    csrw sscratch, t0
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        ld x\r,(8*\r)(sp)
    .endr
    ld sp, 16(sp)
    sret

# U task text: one page at virtual address 0x10000 (physical 0x80020000).
# The page after it is not mapped.
.section .user_text,"ax",@progbits
.option push
.option norvc
.option arch,+a
.globl user_task
user_task:
    # s0 = own page, s1 = the other task's physical page, s2 = kernel guard,
    # s3 = task ID.
    li s4,0
    li s5,0x12345
    sd s5,0(s0)
    amoadd.d.aqrl t0,zero,(s0)
    bne t0,s5,user_bad
1:  lr.d.aq t0,(s0)
    sc.d.rl t1,t0,(s0)
    bnez t1,1b
.globl probe_user_csr
probe_user_csr: csrr t0,mstatus
.globl probe_user_mret
probe_user_mret: mret
.globl probe_user_wfi
probe_user_wfi: wfi
.globl probe_user_cycle
probe_user_cycle: rdcycle t0
.globl probe_kernel_load
probe_kernel_load: ld t0,0(s2)
.globl probe_kernel_store
probe_kernel_store: sd zero,0(s2)
.globl probe_kernel_amo
probe_kernel_amo: amoadd.d.aqrl t0,s5,(s2)
.globl probe_kernel_lr
probe_kernel_lr: lr.d.aq t0,(s2)
.globl probe_kernel_sc
probe_kernel_sc: sc.d.rl t0,s5,(s2)
.globl probe_other_load
probe_other_load: ld t0,0(s1)
.globl probe_other_store
probe_other_store: sd zero,0(s1)
    li t1,0x02004000
.globl probe_mmio_store
probe_mmio_store: sd zero,0(t1)
.globl probe_user_sret
probe_user_sret: sret
    # Own data page: not executable.
    jalr ra,0(s0)
.globl probe_nx_resume
probe_nx_resume:
    li t1,0x10ffc
    jalr ra,0(t1)
.globl probe_cross_fetch_resume
probe_cross_fetch_resume:
    li t1,4092
    add t1,s0,t1
.globl probe_cross_store
probe_cross_store: sd zero,0(t1)
    # A deliberately unusable userspace SP must not affect the trap stack.
    mv s6,sp
    mv sp,s2
    li a7,2
    ecall
    mv sp,s6
2:  addi s4,s4,1
    li t0,1000
3:  addi t0,t0,-1
    bnez t0,3b
    ld t0,0(s0)
    bne t0,s5,user_bad
    li a7,0
    ecall
    li t0,8
    bltu s4,t0,2b
    li a7,1
    ecall
user_bad:
    li a7,99
    ecall
    j user_bad

    # The last 32-bit instruction crosses into the unmapped page.
    .org 4092
    .2byte 0x0001
    .2byte 0x0013
.section .user_tail,"ax",@progbits
    .2byte 0x0000
.option pop
