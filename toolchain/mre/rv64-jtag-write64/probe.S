.option norvc
.option norelax
.section .text.init,"ax",@progbits
.globl _start
_start:
    addi x31, x0, 0x5a
    addi x1, x0, 0x100
    addi x2, x0, 1
    sd x2, 0(x1)
1:  j 1b
.section .tohost,"aw",@progbits
.globl tohost
tohost:
    .dword 0
