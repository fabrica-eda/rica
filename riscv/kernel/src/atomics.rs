//! Actual A instructions for the opt-in hardware-atomic workload.
use core::arch::asm;
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use core::sync::atomic::{AtomicU64, Ordering};
static COUNT: AtomicU64 = AtomicU64::new(0);
static mut LRSC_COUNT: u64 = 0;

pub fn init() {
    let misa: u64;
    unsafe {
        asm!("csrr {}, misa", out(reg) misa, options(nomem, nostack));
    }
    super::check(misa & 1 != 0, 53);
}

pub fn step() -> u64 {
    let old = COUNT.fetch_add(1, Ordering::SeqCst);
    unsafe {
        // Four-instruction constrained loop. Trap entry may invalidate the
        // reservation; retry, without placing any call/store inside the loop.
        asm!(
            ".option push", ".option arch, +a", ".option norvc",
            "2:", "lr.d.aq {value}, ({address})", "addi {value}, {value}, 1",
            "sc.d.rl {status}, {value}, ({address})", "bnez {status}, 2b", ".option pop",
            value = out(reg) _, status = out(reg) _,
            address = in(reg) addr_of_mut!(LRSC_COUNT), options(nostack),
        );
    }
    old
}

pub fn complete() {
    unsafe {
        super::check(COUNT.load(Ordering::Relaxed) == 32, 54);
        super::check(read_volatile(addr_of!(LRSC_COUNT)) == 32, 55);
    }
}
