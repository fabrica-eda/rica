#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
#[cfg(feature = "access-faults")]
use core::ptr::addr_of;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

global_asm!(include_str!("entry.S"));

const MTIMECMP: *mut u64 = 0x0200_4000 as *mut u64;
const MTIME: *const u64 = 0x0200_bff8 as *const u64;
const SLICE: u64 = 3000; // The platform timer runs at 62.4 MHz.
const STACK_SIZE: usize = 16 * 1024;

#[repr(C, align(16))]
pub struct TrapFrame {
    x: [u64; 32],
    pc: u64,
    status: u64,
    cause: u64,
    value: u64,
}
const _: () = assert!(size_of::<TrapFrame>() == 288);

#[repr(C)]
#[derive(Clone, Copy)]
struct TrapRecord {
    cause: u64,
    pc: u64,
    value: u64,
}

#[repr(align(16))]
struct Stacks([[u8; STACK_SIZE]; 2]);

static mut STACKS: Stacks = Stacks([[0; STACK_SIZE]; 2]);
static mut CONTEXTS: [*mut TrapFrame; 2] = [core::ptr::null_mut(); 2];
static mut CURRENT: usize = 0;
static mut DONE: [bool; 2] = [false; 2];
static mut COUNTS: [u64; 2] = [0; 2];
static mut YIELDS: u64 = 0;
static mut TICKS: u64 = 0;
static mut TASK_TICKS: [u64; 2] = [0; 2];
static mut PROBE_EXPECTED: TrapRecord = TrapRecord {
    cause: 0,
    pc: 0,
    value: 0,
};
static mut PROBE_RESUME: u64 = 0;
static mut PROBE_COUNT: usize = 0;
#[unsafe(no_mangle)]
static mut kernel_trap_records: [TrapRecord; 8] = [TrapRecord {
    cause: 0,
    pc: 0,
    value: 0,
}; 8];
#[unsafe(no_mangle)]
#[unsafe(link_section = ".tohost")]
pub static mut tohost: u64 = 0;

unsafe extern "C" {
    fn enter_context(frame: *mut TrapFrame) -> !;
    #[cfg(feature = "access-faults")]
    fn probe_load(address: u64) -> u64;
    #[cfg(feature = "access-faults")]
    fn probe_store(address: u64, value: u64);
    #[cfg(feature = "access-faults")]
    fn probe_jump(address: u64) -> u64;
    #[cfg(feature = "access-faults")]
    static probe_load_instruction: u8;
    #[cfg(feature = "access-faults")]
    static probe_load_resume: u8;
    #[cfg(feature = "access-faults")]
    static probe_store_instruction: u8;
    #[cfg(feature = "access-faults")]
    static probe_store_resume: u8;
    #[cfg(feature = "access-faults")]
    static probe_jump_instruction: u8;
    #[cfg(feature = "access-faults")]
    static probe_jump_resume: u8;
}

fn finish(code: u64) -> ! {
    unsafe {
        asm!("csrci mstatus, 8", "csrw mie, zero", options(nostack));
        write_volatile(MTIMECMP, u64::MAX);
        write_volatile(
            addr_of_mut!(tohost),
            if code == 0 { 1 } else { (code << 1) | 1 },
        );
    }
    loop {
        core::hint::spin_loop();
    }
}

fn check(condition: bool, code: u64) {
    if !condition {
        finish(code);
    }
}

fn arm_timer() {
    unsafe {
        write_volatile(MTIMECMP, read_volatile(MTIME).wrapping_add(SLICE));
    }
}

#[unsafe(no_mangle)]
extern "C" fn kernel_main() -> ! {
    unsafe {
        write_volatile(MTIMECMP, u64::MAX);
    }
    #[cfg(feature = "access-faults")]
    access_probes();
    let gp: u64;
    unsafe {
        asm!("mv {}, gp", out(reg) gp, options(nomem, nostack));
    }
    // All kernel state is initialized with interrupts disabled. Subsequently,
    // only the non-nested M-mode trap handler touches scheduler state.
    unsafe {
        for id in 0..2 {
            let top = addr_of_mut!(STACKS.0)
                .cast::<u8>()
                .add((id + 1) * STACK_SIZE);
            let frame = top.sub(size_of::<TrapFrame>()).cast::<TrapFrame>();
            let mut initial = TrapFrame {
                x: [0; 32],
                pc: task as *const () as u64,
                status: 0x1880,
                cause: 0,
                value: 0, // MPP=M, MPIE=1
            };
            initial.x[2] = top as u64;
            initial.x[3] = gp;
            initial.x[10] = id as u64;
            frame.write(initial);
            CONTEXTS[id] = frame;
        }
        asm!("csrw mie, {}", in(reg) 0x80_u64, options(nostack));
        arm_timer();
        enter_context(CONTEXTS[0]);
    }
}

extern "C" fn task(id: usize) -> ! {
    check(id < 2, 10);
    let mut checksum = 0x1234_5678_9abc_def0_u64 ^ id as u64;
    let guard = [0x1122_3344_5566_7788_u64 ^ id as u64; 8];
    for round in 0..16_u64 {
        for inner in 0..32_u64 {
            checksum =
                checksum.rotate_left(7).wrapping_mul(0x1000_0000_01b3) ^ (round * 32 + inner);
        }
        unsafe {
            // The two tasks have disjoint counters. Volatile accesses make
            // progress observable while the compiler keeps other state live.
            write_volatile(addr_of_mut!(COUNTS).cast::<u64>().add(id), round + 1);
            asm!("ecall", in("a7") 0_u64, options(nostack));
        }
        for &word in core::hint::black_box(&guard) {
            check(word == 0x1122_3344_5566_7788_u64 ^ id as u64, 11);
        }
    }
    // Filled from the byte-independent host workload oracle in build.py.
    let expected = if id == 0 {
        0xd721_6b1b_6ab9_df9b
    } else {
        0x9c77_6af0_3aed_cfd4
    };
    check(checksum == expected, 12 + id as u64);
    // Yield and timer periods can align so that a finite workload sees every
    // tick in the other task. Demand a real preemption before this task exits;
    // coverage must not depend on clock phase or scheduler/handler latency.
    while unsafe { read_volatile(addr_of_mut!(TASK_TICKS).cast::<u64>().add(id)) } == 0 {
        core::hint::spin_loop();
    }
    unsafe {
        asm!("ecall", in("a7") 1_u64, options(nostack));
    }
    finish(14)
}

/// The assembly entry owns the interrupted stack frame. Return the frame to
/// restore; Rust never holds a reference across the switch to another stack.
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_trap(frame: *mut TrapFrame) -> *mut TrapFrame {
    // MIE is cleared by hardware on entry. No handler path enables nesting.
    unsafe {
        let f = &mut *frame;
        if PROBE_RESUME != 0 {
            check(f.cause == PROBE_EXPECTED.cause, 20);
            check(f.pc == PROBE_EXPECTED.pc, 21);
            check(f.value == PROBE_EXPECTED.value, 22);
            check(PROBE_COUNT < 8, 23);
            kernel_trap_records[PROBE_COUNT] = TrapRecord {
                cause: f.cause,
                pc: f.pc,
                value: f.value,
            };
            PROBE_COUNT += 1;
            f.pc = PROBE_RESUME;
            PROBE_RESUME = 0;
            return frame;
        }
        check(CURRENT < 2, 24);
        match f.cause {
            11 => {
                f.pc += 4; // ECALL has no compressed encoding.
                match f.x[17] {
                    0 => YIELDS += 1,
                    1 => DONE[CURRENT] = true,
                    _ => finish(25),
                }
            }
            0x8000_0000_0000_0007 => {
                check(f.value == 0, 26);
                TICKS += 1;
                TASK_TICKS[CURRENT] += 1;
            }
            _ => finish(27),
        }
        CONTEXTS[CURRENT] = frame;
        if DONE[0] && DONE[1] {
            check(COUNTS[0] == 16 && COUNTS[1] == 16 && YIELDS == 32, 28);
            check(TICKS >= 2 && TASK_TICKS[0] != 0 && TASK_TICKS[1] != 0, 29);
            #[cfg(feature = "access-faults")]
            check(PROBE_COUNT == 8, 30);
            finish(0);
        }
        let other = CURRENT ^ 1;
        if !DONE[other] {
            CURRENT = other;
        }
        // ECALL also switches tasks, but must not continually postpone a timer
        // tick. Only timer interrupts start a new time slice.
        if f.cause & (1 << 63) != 0 {
            arm_timer();
        }
        CONTEXTS[CURRENT]
    }
}

#[cfg(feature = "access-faults")]
fn access_probes() {
    fn expect(cause: u64, pc: u64, value: u64, resume: u64) {
        unsafe {
            PROBE_EXPECTED = TrapRecord { cause, pc, value };
            PROBE_RESUME = resume;
        }
    }
    unsafe {
        static mut SENTINEL: u64 = 0x0123_4567_89ab_cdef;
        let alias = addr_of!(SENTINEL) as u64 | (1 << 32);
        expect(
            5,
            addr_of!(probe_load_instruction) as u64,
            alias,
            addr_of!(probe_load_resume) as u64,
        );
        check(probe_load(alias) == 77 && PROBE_RESUME == 0, 31);
        expect(
            7,
            addr_of!(probe_store_instruction) as u64,
            alias,
            addr_of!(probe_store_resume) as u64,
        );
        probe_store(alias, 0);
        check(
            read_volatile(addr_of!(SENTINEL)) == 0x0123_4567_89ab_cdef && PROBE_RESUME == 0,
            32,
        );
        let tail = 0x8003_eff8 as *mut u64;
        write_volatile(tail, 0xdead_beef_1234_5678);
        expect(
            7,
            addr_of!(probe_store_instruction) as u64,
            0x8003_f000,
            addr_of!(probe_store_resume) as u64,
        );
        probe_store(0x8003_efff, 0);
        check(
            read_volatile(tail) == 0xdead_beef_1234_5678 && PROBE_RESUME == 0,
            33,
        );
        expect(
            5,
            addr_of!(probe_load_instruction) as u64,
            0x8003_f000,
            addr_of!(probe_load_resume) as u64,
        );
        check(probe_load(0x8003_efff) == 77 && PROBE_RESUME == 0, 34);
        for target in [0x0000_0001_8000_0000_u64, 0xffff_ffff_8000_0000] {
            expect(1, target, target, addr_of!(probe_jump_resume) as u64);
            check(
                probe_jump(target | 1) == addr_of!(probe_jump_instruction) as u64 + 4
                    && PROBE_RESUME == 0,
                35,
            );
        }
        // A 32-bit instruction at the last halfword faults on its second
        // parcel; a C.NOP there retires and faults on the following fetch.
        // Exercise the real instruction cache and the out-of-range line reply.
        for compressed in [false, true] {
            write_volatile(
                tail,
                if compressed {
                    0x0001_beef_1234_5678
                } else {
                    0x0013_beef_1234_5678
                },
            );
            asm!("fence.i", options(nostack));
            expect(
                1,
                if compressed { 0x8003_f000 } else { 0x8003_effe },
                0x8003_f000,
                addr_of!(probe_jump_resume) as u64,
            );
            check(
                probe_jump(0x8003_effe) == addr_of!(probe_jump_instruction) as u64 + 4
                    && PROBE_RESUME == 0,
                36,
            );
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    finish(63)
}
