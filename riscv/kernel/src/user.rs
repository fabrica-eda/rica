#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
};
global_asm!(include_str!("user_entry.S"));
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Frame {
    x: [u64; 32],
    pc: u64,
    status: u64,
    cause: u64,
    value: u64,
}
const ZERO: Frame = Frame {
    x: [0; 32],
    pc: 0,
    status: 0,
    cause: 0,
    value: 0,
};
static mut FRAMES: [Frame; 2] = [ZERO; 2];
static mut CURRENT: usize = 0;
// Initialized once with interrupts disabled; immutable during scheduling.
static mut SLICE_TICKS: u64 = 1;
static mut DONE: [bool; 2] = [false; 2];
static mut FAULTS: [u64; 2] = [0; 2];
static mut YIELDS: [u64; 2] = [0; 2];
static mut TICKS: [u64; 2] = [0; 2];
static mut BAD_SP: [bool; 2] = [false; 2];
#[unsafe(no_mangle)]
static mut user_kernel_guard: u64 = 0x123456789abcdef0;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".tohost")]
static mut tohost: u64 = 0;
const TIMER: *mut u64 = 0x02004000 as *mut u64;
const TIME: *const u64 = 0x0200bff8 as *const u64;
const TIMEBASE_HZ: *const u64 = 0x0200bff0 as *const u64;
unsafe extern "C" {
    fn user_enter(f: *const Frame) -> !;
    fn user_task();
    static probe_user_csr: u8;
    static probe_user_mret: u8;
    static probe_user_wfi: u8;
    static probe_user_cycle: u8;
    static probe_kernel_load: u8;
    static probe_kernel_store: u8;
    static probe_kernel_amo: u8;
    static probe_kernel_lr: u8;
    static probe_kernel_sc: u8;
    static probe_other_load: u8;
    static probe_other_store: u8;
    static probe_cross_store: u8;
    static probe_cross_fetch_resume: u8;
    static probe_mmio_store: u8;
    static probe_nx_resume: u8;
}
fn finish(code: u64) -> ! {
    unsafe {
        asm!("csrw mie,zero", options(nostack));
        write_volatile(TIMER, u64::MAX);
        write_volatile(
            addr_of_mut!(tohost),
            if code == 0 { 1 } else { code * 2 + 1 },
        );
    }
    loop {
        core::hint::spin_loop()
    }
}
fn check(ok: bool, code: u64) {
    if !ok {
        finish(code)
    }
}
unsafe fn select(id: usize) -> *const Frame {
    unsafe {
        // Reprogram the task's data/stack region before MRET; code is shared RX.
        let page = 0x80024000u64 + id as u64 * 4096;
        asm!("csrw pmpaddr1,{0}",in(reg)((page>>2)|511),options(nostack));
        // 50 microseconds, independent of the selected board clock.
        write_volatile(TIMER, read_volatile(TIME).wrapping_add(SLICE_TICKS));
        CURRENT = id;
        addr_of!(FRAMES).cast::<Frame>().add(id)
    }
}
#[unsafe(no_mangle)]
extern "C" fn user_kernel_main() -> ! {
    unsafe {
        SLICE_TICKS = (read_volatile(TIMEBASE_HZ) / 20_000).max(1);
        let misa: u64;
        asm!("csrr {},misa",out(reg)misa,options(nostack));
        check(misa & (1 << 20) != 0, 1);
        // PMP0 RX 4KiB user text; PMP1 RW task page. Other entries deny by default.
        asm!("csrw pmpaddr0,{0}",in(reg)((0x80020000u64>>2)|511),options(nostack));
        asm!("csrw pmpcfg0,{0}",in(reg)0x1b1du64,options(nostack));
        asm!("csrw mcounteren,zero", options(nostack));
        for id in 0..2 {
            let page = 0x80024000 + id as u64 * 4096;
            write_volatile((page + 4092) as *mut u32, 0xdeadbeef);
            let f = &mut *addr_of_mut!(FRAMES).cast::<Frame>().add(id);
            f.pc = user_task as *const () as u64;
            f.status = 0x80;
            f.x[2] = page + 4096;
            f.x[8] = page;
            f.x[9] = 0x80024000 + (1 - id) as u64 * 4096;
            f.x[18] = addr_of!(user_kernel_guard) as u64;
            f.x[19] = id as u64;
        }
        asm!("csrw mie,{0}",in(reg)128u64,options(nostack));
        user_enter(select(0));
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn user_trap(frame: *const Frame) -> *const Frame {
    unsafe {
        let id = CURRENT;
        check(id < 2, 2);
        let mut f = read_volatile(frame);
        check(f.status & 0x1800 == 0, 3); // trap actually originated in U
        check(
            read_volatile(addr_of!(user_kernel_guard)) == 0x123456789abcdef0,
            4,
        );
        let probes = [
            (addr_of!(probe_user_csr) as u64, 2),
            (addr_of!(probe_user_mret) as u64, 2),
            (addr_of!(probe_user_wfi) as u64, 2),
            (addr_of!(probe_user_cycle) as u64, 2),
            (addr_of!(probe_kernel_load) as u64, 5),
            (addr_of!(probe_kernel_store) as u64, 7),
            (addr_of!(probe_kernel_amo) as u64, 7),
            (addr_of!(probe_kernel_lr) as u64, 5),
            (addr_of!(probe_kernel_sc) as u64, 7),
            (addr_of!(probe_other_load) as u64, 5),
            (addr_of!(probe_other_store) as u64, 7),
            (addr_of!(probe_mmio_store) as u64, 7),
            (addr_of!(probe_cross_store) as u64, 7),
        ];
        let mut switch = false;
        if f.cause == 0x8000000000000007 {
            TICKS[id] += 1;
            switch = true;
        } else if f.cause == 8 {
            check(f.value == 0, 5);
            f.pc += 4;
            match f.x[17] {
                0 => {
                    YIELDS[id] += 1;
                    switch = true;
                }
                1 => {
                    DONE[id] = true;
                    switch = true;
                }
                2 => {
                    check(f.x[2] == addr_of!(user_kernel_guard) as u64, 6);
                    BAD_SP[id] = true;
                }
                _ => finish(7),
            }
        } else if f.cause == 1 {
            if f.pc == 0x80020ffe {
                check(f.value == 0x80021000, 17);
                FAULTS[id] |= 1 << 14;
                f.pc = addr_of!(probe_cross_fetch_resume) as u64;
            } else {
                check(f.pc == f.x[8] && f.value == f.pc, 8);
                FAULTS[id] |= 1 << 13;
                f.pc = addr_of!(probe_nx_resume) as u64;
            }
        } else {
            let n = probes
                .iter()
                .position(|p| p.0 == f.pc)
                .unwrap_or_else(|| finish(9));
            check(f.cause == probes[n].1, 10);
            let expected = if n < 4 {
                read_volatile(f.pc as *const u32) as u64
            } else if n < 9 {
                f.x[18]
            } else if n < 11 {
                f.x[9]
            } else if n == 11 {
                0x02004000
            } else {
                f.x[8] + 4092
            };
            check(f.value == expected, 11);
            check(FAULTS[id] & (1 << n) == 0, 12);
            FAULTS[id] |= 1 << n;
            f.pc += 4;
        }
        write_volatile(addr_of_mut!(FRAMES).cast::<Frame>().add(id), f);
        if DONE[0] && DONE[1] {
            for n in 0..2 {
                check(FAULTS[n] == 0x7fff, 13);
                check(YIELDS[n] == 8, 14);
                check(TICKS[n] > 0, 15);
                check(BAD_SP[n], 16);
                check(
                    read_volatile((0x80024000 + n * 4096 + 4092) as *const u32) == 0xdeadbeef,
                    18,
                );
            }
            finish(0);
        }
        let next = if switch && !DONE[1 - id] { 1 - id } else { id };
        if switch {
            select(next)
        } else {
            addr_of!(FRAMES).cast::<Frame>().add(id)
        }
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    finish(99)
}
