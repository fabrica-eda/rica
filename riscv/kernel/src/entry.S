.section .text.init, "ax", @progbits
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
1:
    bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    la t0, trap_entry
    csrw mtvec, t0
    call kernel_main
3:
    j 3b

.section .text.trap, "ax", @progbits
.balign 4
.globl trap_entry
trap_entry:
    addi sp, sp, -288
    sd zero, 0(sp)
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        sd x\r, (8 * \r)(sp)
    .endr
    addi t0, sp, 288
    sd t0, 16(sp)
    csrr t0, mepc
    sd t0, 256(sp)
    csrr t0, mstatus
    sd t0, 264(sp)
    csrr t0, mcause
    sd t0, 272(sp)
    csrr t0, mtval
    sd t0, 280(sp)
    mv a0, sp
    call rust_trap
.globl enter_context
enter_context:
    mv sp, a0
    ld t0, 256(sp)
    csrw mepc, t0
    ld t0, 264(sp)
    csrw mstatus, t0
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        ld x\r, (8 * \r)(sp)
    .endr
    ld sp, 16(sp)
    mret

# Fixed-width probes expose the exact fault and recovery PCs to Rust. Ordinary
# kernel code still uses compressed instructions.
.option push
.option norvc
.balign 4
.globl probe_load, probe_load_instruction, probe_load_resume
probe_load:
    li a1, 77
probe_load_instruction:
    ld a1, 0(a0)
probe_load_resume:
    mv a0, a1
    ret
.globl probe_store, probe_store_instruction, probe_store_resume
probe_store:
probe_store_instruction:
    sd a1, 0(a0)
probe_store_resume:
    ret
.globl probe_jump, probe_jump_instruction, probe_jump_resume
probe_jump:
probe_jump_instruction:
    jalr t0, 0(a0)
probe_jump_resume:
    mv a0, t0
    ret
.option pop
