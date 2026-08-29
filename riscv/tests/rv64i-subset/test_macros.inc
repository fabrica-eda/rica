.option norvc
.option norelax

.macro REPORT value
    la t0, tohost
    li t1, \value
    sd t1, 0(t0)
1:  j 1b
.endm

.macro PASS
    REPORT 1
.endm

.macro FAIL test_number
    REPORT ((\test_number << 1) | 1)
.endm

.pushsection .tohost, "aw", @progbits
.balign 4
.globl tohost
tohost:
    .dword 0
.popsection
