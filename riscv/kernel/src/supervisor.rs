//! S-kernel with two U tasks isolated by Sv39 page tables.
//!
//! Each task has its own root table: its text page and data page at fixed
//! virtual addresses with U set, and supervisor-only mappings of the kernel
//! (1 GiB identity) and the timer. The tasks run the same probes as the
//! U/PMP kernel; here every forbidden access is a page fault. Timer slices
//! arrive as the S timer interrupt that the M firmware forwards.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
};
global_asm!(include_str!("supervisor_entry.S"));
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
#[repr(C, align(4096))]
struct Table([u64; 512]);
const EMPTY: Table = Table([0; 512]);
// Per task: root, level-1 and level-0 tables.
static mut TABLES: [[Table; 3]; 2] = [[EMPTY, EMPTY, EMPTY], [EMPTY, EMPTY, EMPTY]];
static mut FRAMES: [Frame; 2] = [ZERO; 2];
static mut CURRENT: usize = 0;
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
const TIME: *const u64 = 0x0200bff8 as *const u64;
const TIMEBASE_HZ: *const u64 = 0x0200bff0 as *const u64;
// Physical layout and the tasks' virtual layout.
const USER_TEXT_PHYSICAL: u64 = 0x80020000;
const TASK_PAGE_PHYSICAL: u64 = 0x80024000;
const USER_TEXT: u64 = 0x10000;
const TASK_PAGE: u64 = 0x20000;
const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const U: u64 = 16;
const A: u64 = 64;
const D: u64 = 128;
unsafe extern "C" {
    fn supervisor_enter(f: *const Frame) -> !;
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
    static probe_mmio_store: u8;
    static probe_cross_store: u8;
    static probe_user_sret: u8;
    static probe_cross_fetch_resume: u8;
    static probe_nx_resume: u8;
}
fn machine_call(service: u64, argument: u64) {
    unsafe {
        asm!("ecall", in("a7") service, in("a0") argument, options(nostack));
    }
}
fn finish(code: u64) -> ! {
    machine_call(1, if code == 0 { 1 } else { code * 2 + 1 });
    loop {
        core::hint::spin_loop()
    }
}
fn check(ok: bool, code: u64) {
    if !ok {
        finish(code)
    }
}
/// The task's virtual address of a symbol in the U text page.
fn user_address(symbol: *const u8) -> u64 {
    symbol as u64 - USER_TEXT_PHYSICAL + USER_TEXT
}
fn pte(physical: u64, flags: u64) -> u64 {
    (physical >> 12) << 10 | flags
}
unsafe fn select(id: usize) -> *const Frame {
    unsafe {
        let root = addr_of!(TABLES[id][0]) as u64;
        asm!("csrw satp,{0}", "sfence.vma", in(reg) (8u64 << 60) | (root >> 12), options(nostack));
        machine_call(0, read_volatile(TIME).wrapping_add(SLICE_TICKS));
        CURRENT = id;
        addr_of!(FRAMES).cast::<Frame>().add(id)
    }
}
#[unsafe(no_mangle)]
extern "C" fn supervisor_main() -> ! {
    unsafe {
        // 50 microseconds, independent of the selected board clock.
        SLICE_TICKS = (read_volatile(TIMEBASE_HZ) / 20_000).max(1);
        for id in 0..2 {
            let tables = &mut *addr_of_mut!(TABLES[id]);
            let [root, level1, level0] = tables;
            root.0[0] = pte(addr_of!(*level1) as u64, V);
            // Kernel, task pages and tables: supervisor-only identity.
            root.0[2] = pte(0x80000000, V | R | W | X | A | D);
            level1.0[0] = pte(addr_of!(*level0) as u64, V);
            // The timer, as a supervisor-only 2 MiB page.
            level1.0[0x10] = pte(0x02000000, V | R | W | A | D);
            level0.0[(USER_TEXT >> 12) as usize] = pte(USER_TEXT_PHYSICAL, V | R | X | U | A);
            let page = TASK_PAGE_PHYSICAL + id as u64 * 4096;
            level0.0[(TASK_PAGE >> 12) as usize] = pte(page, V | R | W | U | A | D);
            write_volatile((page + 4092) as *mut u32, 0xdeadbeef);
            let f = &mut *addr_of_mut!(FRAMES).cast::<Frame>().add(id);
            f.pc = user_address(user_task as *const () as *const u8);
            f.status = 0x20; // SPP = U, SPIE
            f.x[2] = TASK_PAGE + 4096;
            f.x[8] = TASK_PAGE;
            f.x[9] = TASK_PAGE_PHYSICAL + (1 - id) as u64 * 4096;
            f.x[18] = addr_of!(user_kernel_guard) as u64;
            f.x[19] = id as u64;
        }
        asm!("csrw scounteren,zero", "csrw sie,{0}", in(reg) 1u64 << 5, options(nostack));
        supervisor_enter(select(0));
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn supervisor_trap(frame: *const Frame) -> *const Frame {
    unsafe {
        let id = CURRENT;
        check(id < 2, 2);
        let mut f = read_volatile(frame);
        check(f.status & 0x100 == 0, 3); // trap actually originated in U
        check(
            read_volatile(addr_of!(user_kernel_guard)) == 0x123456789abcdef0,
            4,
        );
        let probes = [
            (user_address(addr_of!(probe_user_csr)), 2),
            (user_address(addr_of!(probe_user_mret)), 2),
            (user_address(addr_of!(probe_user_wfi)), 2),
            (user_address(addr_of!(probe_user_cycle)), 2),
            (user_address(addr_of!(probe_kernel_load)), 13),
            (user_address(addr_of!(probe_kernel_store)), 15),
            (user_address(addr_of!(probe_kernel_amo)), 15),
            (user_address(addr_of!(probe_kernel_lr)), 13),
            (user_address(addr_of!(probe_kernel_sc)), 15),
            (user_address(addr_of!(probe_other_load)), 13),
            (user_address(addr_of!(probe_other_store)), 15),
            (user_address(addr_of!(probe_mmio_store)), 15),
            (user_address(addr_of!(probe_cross_store)), 6),
            (user_address(addr_of!(probe_user_sret)), 2),
        ];
        let mut switch = false;
        if f.cause == 0x8000000000000005 {
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
        } else if f.cause == 12 {
            if f.pc == USER_TEXT + 0xffe {
                check(f.value == USER_TEXT + 0x1000, 17);
                FAULTS[id] |= 1 << 15;
                f.pc = user_address(addr_of!(probe_cross_fetch_resume));
            } else {
                check(f.pc == f.x[8] && f.value == f.pc, 8);
                FAULTS[id] |= 1 << 14;
                f.pc = user_address(addr_of!(probe_nx_resume));
            }
        } else {
            let n = probes
                .iter()
                .position(|p| p.0 == f.pc)
                .unwrap_or_else(|| finish(9));
            check(f.cause == probes[n].1, 10);
            let expected = if n < 4 || n == 13 {
                // The encoding, read through the kernel's identity mapping.
                read_volatile((f.pc - USER_TEXT + USER_TEXT_PHYSICAL) as *const u32) as u64
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
                check(FAULTS[n] == 0xffff, 13);
                check(YIELDS[n] == 8, 14);
                check(TICKS[n] > 0, 15);
                check(BAD_SP[n], 16);
                check(
                    read_volatile((TASK_PAGE_PHYSICAL + n as u64 * 4096 + 4092) as *const u32)
                        == 0xdeadbeef,
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
