.section .text
.globl _start
_start:
    li a0, 1
    la a1, message
    li a2, message_end-message
    li a7, 64
    ecall
    li t0, message_end-message
    bne a0, t0, failed
    li a0, 0xfee1dead
    li a1, 672274793
    li a2, 0x4321fedc
    li a3, 0
    li a7, 142
    ecall
1:  j 1b
failed:
    li a0, 1
    li a7, 93
    ecall
    j 1b
.section .rodata
message: .ascii "RICA_LINUX_USERSPACE_OK\n"
message_end:
