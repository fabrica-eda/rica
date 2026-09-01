.include "test_macros.inc"

// Directed RV64C coverage. Keep the checks as ordinary 32-bit instructions
// where their width matters, while the operations under test use explicit
// compressed mnemonics. In particular, the first two groups put a 32-bit
// instruction at halfword offsets within and across an eight-byte fetch line.
.option push
.option rvc
.section .text.init, "ax", @progbits
.globl _start
_start:
    .option push
    .option norvc
    la sp, stack_top
    .option pop

    .balign 4
    c.nop
    .option push
    .option norvc
    addi t0, x0, 29
    .option pop
    c.li t1, 29
    bne t0, t1, fail_1

    .balign 8
    c.nop
    c.nop
    c.nop
    .option push
    .option norvc
    addi t0, x0, 23
    .option pop
    c.li t1, 23
    bne t0, t1, fail_2

    c.li s0, 5
    c.addi s0, 3
    addi t0, x0, 8
    bne s0, t0, fail_3

    c.li s1, -2
    c.addiw s1, 1
    addi t0, x0, -1
    bne s1, t0, fail_4

    c.lui s2, 1
    lui t0, 1
    bne s2, t0, fail_5

    addi t0, sp, 16
    c.addi16sp sp, 16
    bne sp, t0, fail_6
    c.addi16sp sp, -16

    c.addi4spn s0, sp, 32
    addi t0, sp, 32
    bne s0, t0, fail_7

    c.li a0, -7
    c.sdsp a0, 0(sp)
    c.ldsp a1, 0(sp)
    bne a0, a1, fail_8
    c.swsp a0, 8(sp)
    c.lwsp a2, 8(sp)
    bne a0, a2, fail_9

    c.sd a0, 0(s0)
    c.ld a1, 0(s0)
    bne a0, a1, fail_10
    c.sw a0, 8(s0)
    c.lw a2, 8(s0)
    bne a0, a2, fail_11

    c.li s0, 15
    c.slli s0, 2
    c.li s1, 15
    c.sub s0, s1
    addi t0, x0, 45
    bne s0, t0, fail_12

    c.li s0, 15
    c.slli s0, 2
    c.xor s0, s1
    addi t0, x0, 51
    bne s0, t0, fail_13

    c.li s0, 15
    c.slli s0, 2
    c.or s0, s1
    addi t0, x0, 63
    bne s0, t0, fail_14

    c.li s0, 15
    c.slli s0, 2
    c.and s0, s1
    addi t0, x0, 12
    bne s0, t0, fail_15

    c.li s0, 15
    c.slli s0, 2
    c.srli s0, 2
    bne s0, s1, fail_16
    c.li s0, -16
    c.srai s0, 2
    addi t0, x0, -4
    bne s0, t0, fail_17
    c.li s0, 15
    c.andi s0, 7
    addi t0, x0, 7
    bne s0, t0, fail_18

    c.li s0, -1
    c.li s1, 1
    c.addw s0, s1
    bne s0, x0, fail_19
    c.subw s0, s1
    addi t0, x0, -1
    bne s0, t0, fail_20

    c.mv s2, s1
    bne s2, s1, fail_21
    c.add s2, s1
    addi t0, x0, 2
    bne s2, t0, fail_22

    c.li s0, 0
    c.beqz s0, beqz_taken
    j fail_23
beqz_taken:
    c.li s0, 1
    c.bnez s0, bnez_taken
    j fail_24
bnez_taken:
    c.j jump_taken
    j fail_25
jump_taken:
    la s1, jr_target
    c.jr s1
    j fail_26
jr_target:
    la s1, jalr_target
    c.jalr s1
jalr_return:
    c.j jalr_done
jalr_target:
    la t0, jalr_return
    bne ra, t0, fail_27
    c.jr ra
jalr_done:

    // C.EBREAK must report its halfword PC and resume at PC+2.
    la s0, compressed_trap
    csrw mtvec, s0
    c.li s1, 3
    c.li s3, 0
    .balign 4
    c.nop
    la s2, c_ebreak_site
c_ebreak_site:
    c.ebreak
after_c_ebreak:
    c.li s1, 2
    c.li s3, 4
    .balign 4
    c.nop
    la s2, illegal_compressed_site
illegal_compressed_site:
    .2byte 0x0004 // Reserved C.ADDI4SPN with nonzero rd' and nzuimm=0.
after_illegal_compressed:
    c.j compressed_done

    .option push
    .option norvc
    .balign 4
compressed_trap:
    csrr t0, mcause
    bne t0, s1, fail_28
    csrr t0, mepc
    bne t0, s2, fail_29
    andi t1, t0, 2
    beq t1, x0, fail_31
    csrr t0, mtval
    bne t0, s3, fail_30
    addi t0, s2, 2
    csrw mepc, t0
    mret
    .option pop

compressed_done:
    .option pop
    PASS

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
fail_21: FAIL 21
fail_22: FAIL 22
fail_23: FAIL 23
fail_24: FAIL 24
fail_25: FAIL 25
fail_26: FAIL 26
fail_27: FAIL 27
fail_28: FAIL 28
fail_29: FAIL 29
fail_30: FAIL 30
fail_31: FAIL 31

.section .bss, "aw", @nobits
.balign 16
stack:
    .space 256
stack_top:
