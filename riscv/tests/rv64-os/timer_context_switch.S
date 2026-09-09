.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    li sp, 0x80039000
    la t0, trap_handler
    csrw mtvec, t0
    la t0, context_slots
    li t1, 0x8003a000 - 256
    sd t1, 8(t0)
    la t0, task_one
    sd t0, 0(t1)
    li t0, 0x8003a000
    sd t0, 16(t1)
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        li t0, 0x200 + \r
        sd t0, (8 * \r)(t1)
    .endr
    # Save the initial task-zero PC through MRET, then seed every register.
    li t0, 0x80
    csrw mie, t0
    csrs mstatus, t0
    la t0, task_zero
    csrw mepc, t0
    li t0, 0x0200bff8
    ld t1, 0(t0)
    li t2, 3000
    add t1, t1, t2
    li t0, 0x02004000
    sd t1, 0(t0)
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        li x\r, 0x100 + \r
    .endr
    mret

.macro TASK base, counter, stack
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30
        li x31, \base + \r
        bne x\r, x31, fail_1
    .endr
    li x31, \stack
    bne sp, x31, fail_1
    # Interruptions can fall between any operand/compare or memory pair.
    la x31, \counter
    ld x30, 0(x31)
    addi x30, x30, 1
    sd x30, 0(x31)
    li x30, \base + 30
.endm

task_zero:
    TASK 0x100, count_zero, 0x80039000
    j task_zero
task_one:
    TASK 0x200, count_one, 0x8003a000
    j task_one

.balign 4
trap_handler:
    addi sp, sp, -256
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        sd x\r, (8 * \r)(sp)
    .endr
    addi t0, sp, 256
    sd t0, 16(sp)
    csrr t0, mepc
    sd t0, 0(sp)
    csrr t0, mcause
    li t1, 0x8000000000000007
    bne t0, t1, fail_2
    csrr t0, mtval
    bnez t0, fail_2
    la t0, current_task
    ld t1, 0(t0)
    la t2, context_slots
    slli t3, t1, 3
    add t3, t2, t3
    sd sp, 0(t3)
    xori t1, t1, 1
    sd t1, 0(t0)
    slli t3, t1, 3
    add t3, t2, t3
    ld sp, 0(t3)
    la t0, switches
    ld t1, 0(t0)
    addi t1, t1, 1
    sd t1, 0(t0)
    li t2, 16
    beq t1, t2, finished
    # Arm the next slice before restoring the interrupted register bank.
    li t0, 0x0200bff8
    ld t1, 0(t0)
    li t2, 3000
    add t1, t1, t2
    li t0, 0x02004000
    sd t1, 0(t0)
    ld t0, 0(sp)
    csrw mepc, t0
    .irp r,1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
        ld x\r, (8 * \r)(sp)
    .endr
    ld sp, 16(sp)
    mret
finished:
    li t0, 0x02004000
    li t1, -1
    sd t1, 0(t0)
    la t0, count_zero
    ld t1, 0(t0)
    li t2, 2
    bltu t1, t2, fail_3
    la t0, count_one
    ld t1, 0(t0)
    bltu t1, t2, fail_3
    PASS
fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
.section .data, "aw", @progbits
.balign 8
context_slots: .dword 0, 0
current_task: .dword 0
switches: .dword 0
count_zero: .dword 0
count_one: .dword 0
