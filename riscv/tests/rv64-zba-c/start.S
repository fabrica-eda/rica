.option norelax
.section .text.init, "ax", @progbits
.globl _start
_start:
    li sp, 0x8003e000
    call run
    la t0, tohost
    sd a0, 0(t0)
1:  j 1b
.section .tohost, "aw", @progbits
.balign 8
.globl tohost
tohost:
    .dword 0
