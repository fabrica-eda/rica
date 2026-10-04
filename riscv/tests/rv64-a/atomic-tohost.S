.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, tohost
    li t1, 1
    # Terminal atomic must retire on the write acknowledgment, before the
    # board freezes the performance counters for this success result.
    amoadd.d.aqrl zero, t1, (t0)
1:  j 1b
