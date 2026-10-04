//! Single-hart M-mode syscall ABI. Trap entry disables interrupts, so the
//! shared update has one non-nested owner. This does not advertise ISA atomics.
use core::arch::asm;

pub const GET_ID: u64 = 2;
pub const FETCH_ADD: u64 = 3;
pub const CLOCK: u64 = 4;
pub const ENOSYS: u64 = (-38_i64) as u64;

pub fn call(number: u64, argument: u64) -> u64 {
    let mut result = argument;
    unsafe {
        asm!("ecall", in("a7") number, inlateout("a0") result, options(nostack));
    }
    result
}

pub fn probe(id: usize) {
    super::check(call(GET_ID, 0) == id as u64, 45);
    super::check(call(u64::MAX, 123) == ENOSYS, 46);
    let before = call(CLOCK, 0);
    let after = call(CLOCK, 0);
    super::check(after.wrapping_sub(before) < (1 << 63), 47);
}
