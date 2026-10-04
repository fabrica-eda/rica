.section .text.init,"ax",@progbits
.globl _start
_start:
    csrw mie, zero
    csrci mstatus, 8
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    la sp, __boot_stack_top
    la t0, __bss_start
    la t1, __bss_end
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:  la t0, user_trap_entry
    csrw mtvec, t0
    call user_kernel_main
3:  j 3b

.section .text.trap,"ax",@progbits
.balign 4
user_trap_entry:
    # MSCRATCH always holds a kernel-owned stack when executing userspace.
    # No register is stored through the untrusted interrupted SP.
    csrrw sp, mscratch, sp
    addi sp, sp, -288
    sd zero, 0(sp)
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        sd x\r,(8*\r)(sp)
    .endr
    csrr t0, mscratch
    sd t0,16(sp)
    csrr t0,mepc
    sd t0,256(sp)
    csrr t0,mstatus
    sd t0,264(sp)
    csrr t0,mcause
    sd t0,272(sp)
    csrr t0,mtval
    sd t0,280(sp)
    mv a0,sp
    call user_trap
.globl user_enter
user_enter:
    mv sp,a0
    ld t0,256(sp)
    csrw mepc,t0
    ld t0,264(sp)
    csrw mstatus,t0
    la t0,__boot_stack_top
    csrw mscratch,t0
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        ld x\r,(8*\r)(sp)
    .endr
    ld sp,16(sp)
    mret

.section .user_text,"ax",@progbits
.option push
.option norvc
.option arch,+a
.globl user_task
user_task:
    # s0 = own page, s1 = other task's page, s2 = kernel guard, s3 = task ID.
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
    # Fetch permission also applies on a warm I-cache line loaded in M-mode.
    jalr ra,0(s0)
.globl probe_nx_resume
probe_nx_resume:
    li t1,0x80020ffc
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

    .org 4092
    .2byte 0x0001
    .2byte 0x0013
.section .user_tail,"ax",@progbits
    .2byte 0x0000
.option pop
