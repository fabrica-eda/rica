//! Single-hart, non-nested M-mode console. Boot and the trap handler are the
//! only writers; tasks never print while an interrupt can preempt a write.
use core::ptr::{addr_of, read_volatile, write_volatile};

const BASE: usize = 0x0200_1000;
const INPUT: &[u8] = b"ping\n";
static mut RECEIVED: usize = 0;

fn read(offset: usize) -> u64 {
    unsafe { read_volatile((BASE + offset) as *const u64) }
}

fn write(offset: usize, value: u8) {
    unsafe { write_volatile((BASE + offset) as *mut u8, value) }
}

pub fn init() {
    super::check(read(0x20) == 0x434f_4e01, 40);
    write(0x18, 1);
    text(b"rica: boot M-mode\n");
}

pub fn text(bytes: &[u8]) {
    for &byte in bytes {
        // The host runs independently of the CPU and drains even while MIE
        // is clear or tohost has frozen the counters. Never overwrite a full
        // FIFO. A disconnected host intentionally applies backpressure.
        while read(0x10) & 1 == 0 {
            core::hint::spin_loop();
        }
        write(0, byte);
    }
}

fn hex(value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    text(b"0x");
    for shift in (0..16).rev() {
        text(&[DIGITS[((value >> (shift * 4)) & 15) as usize]]);
    }
}

pub fn fault(cause: u64, pc: u64, value: u64) {
    text(b"fault cause=");
    hex(cause);
    text(b" pc=");
    hex(pc);
    text(b" tval=");
    hex(value);
    text(b"\n");
}

/// Only the trap handler mutates RECEIVED. Reading RX_DATA consumes one byte;
/// an empty read is harmless, including an IRQ still crossing the clock bridge.
pub fn interrupt() {
    let already_done = done();
    loop {
        let value = read(8);
        if value & 0x100 == 0 {
            break;
        }
        unsafe {
            super::check(RECEIVED < INPUT.len(), 41);
            super::check(value as u8 == INPUT[RECEIVED], 42);
            RECEIVED += 1;
        }
    }
    if done() && !already_done {
        write(0x18, 0);
        text(b"console: pong irq=MEI rx=5\n");
    }
    super::check(read(0x30) == 0, 43);
}

pub fn done() -> bool {
    unsafe { read_volatile(addr_of!(RECEIVED)) == INPUT.len() }
}

pub fn finish(code: u64) {
    write(0x18, 0);
    if code == 0 {
        text(b"tasks: PASS\nkernel: PASS\n");
    } else {
        text(b"kernel: FAIL code=");
        hex(code);
        text(b"\n");
    }
}
