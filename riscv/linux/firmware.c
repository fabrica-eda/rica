/* Single-hart simulation firmware. Runs on Rica, not in the host. */
typedef unsigned long u64;
#define CSR_READ(c) ({ u64 v; __asm__ volatile("csrr %0, " #c : "=r"(v)); v; })
#define CSR_WRITE(c,v) __asm__ volatile("csrw " #c ", %0" :: "r"((u64)(v)) : "memory")
#define MTIME (*(volatile u64 *)0x0200bff8UL)
#define MTIMECMP (*(volatile u64 *)0x02004000UL)
static void putchar(int c) { *(volatile u64 *)0x02001000UL = c; }
static void puts(const char *s) { while (*s) putchar(*s++); }
static void hex(u64 v) { for (int n=60;n>=0;n-=4) putchar("0123456789abcdef"[(v>>n)&15]); }
static void stop(u64 code) { *(volatile u64 *)0x02001008UL=code; for (;;) __asm__ volatile("wfi"); }
extern void trap_entry(void);
extern char trap_stack_top[];
void boot(void) {
    CSR_WRITE(mtvec, trap_entry);
    CSR_WRITE(mscratch, trap_stack_top);
    CSR_WRITE(pmpaddr0, 0x3fffffff);
    CSR_WRITE(pmpcfg0, 0x1f);
    CSR_WRITE(medeleg, 0xb1fb); /* Illegal instructions stay in M for rdtime. */
    CSR_WRITE(mideleg, 0x222);
    CSR_WRITE(mcounteren, 5);
    CSR_WRITE(satp, 0);
    MTIMECMP=~0UL;
    CSR_WRITE(mie, 0);
    puts("Rica simulation SBI: entering Linux at 0x80200000\n");
    CSR_WRITE(mepc, 0x80200000);
    CSR_WRITE(mstatus, 0x800); /* MPP=S, interrupts disabled. */
    __asm__ volatile("li a0, 0; li a1, 0x87e00000; mret" ::: "a0", "a1", "memory");
    __builtin_unreachable();
}
void trap(u64 *r) {
    u64 cause=CSR_READ(mcause), pc=CSR_READ(mepc), value=CSR_READ(mtval);
    if (cause == (1UL<<63 | 7)) {
        __asm__ volatile("csrc mie, %0; csrs mip, %1" :: "r"(128UL), "r"(32UL));
        return;
    }
    if (cause == 2 && (value & 0xfffff07fUL) == 0xc0102073UL) {
        unsigned rd=(value>>7)&31;
        if (rd) r[rd]=MTIME;
        CSR_WRITE(mepc, pc+4);
        return;
    }
    if (cause == 9) {
        u64 ext=r[17], fid=r[16], a0=r[10], a1=r[11];
        long error=0; u64 result=0;
        switch (ext) {
        case 0: case 0x54494d45:
            if (ext && fid) { error=-2; break; }
            MTIMECMP=a0;
            __asm__ volatile("csrc mip, %0; csrs mie, %1" :: "r"(32UL), "r"(128UL));
            break;
        case 1: putchar(a0); break;
        case 2: r[10]=~0UL; CSR_WRITE(mepc,pc+4); return;
        case 8: puts("SBI shutdown\n"); stop(1); break;
        case 0x10:
            switch (fid) {
            case 0: result=0x02000000; break; /* SBI 2.0 */
            case 1: result=0xffffffff; break; /* private experimental implementation */
            case 2: result=1; break;
            case 3: result=a0==0x10 || a0==0x54494d45 || a0==0x53525354 || a0==0x4442434e; break;
            case 4: case 5: case 6: result=0; break;
            default: error=-2;
            }
            break;
        case 0x4442434e:
            if (fid==2) putchar(a0);
            else if (fid==0) {
                if (r[12] || a1<0x80000000 || a1>=0x88000000 || a0>0x88000000-a1) error=-5;
                else { for (u64 i=0;i<a0;i++) putchar(*(volatile unsigned char *)(a1+i)); result=a0; }
            } else if (fid!=1) error=-2;
            break;
        case 0x53525354:
            if (fid || a0>2 || a1>1) { error=-3; break; }
            puts("SBI reset/shutdown\n"); stop(a1 ? 2 : 1); break;
        default: error=-2;
        }
        r[10]=error;
        if (ext>8) r[11]=result;
        CSR_WRITE(mepc,pc+4);
        return;
    }
    puts("FIRMWARE TRAP cause="); hex(cause); puts(" pc="); hex(pc); puts(" value="); hex(value); puts("\n");
    stop(2);
}
