use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};

fn get(sim: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    sim.get(sim.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}
fn set(sim: &mut Simulator<NativeBackend>, name: &str, value: u64) {
    let signal = sim.signal(name);
    sim.modify(|io| {
        if matches!(name, "memory_value" | "source_value") {
            io.set(signal, value);
        } else {
            io.set(signal, value as u8);
        }
    })
    .unwrap();
}
fn tick(sim: &mut Simulator<NativeBackend>) {
    let clock = sim.event("clk");
    sim.tick(clock).unwrap();
}
fn reset(sim: &mut Simulator<NativeBackend>) {
    set(sim, "rst_n", 0);
    tick(sim);
    set(sim, "rst_n", 1);
    assert_eq!(get(sim, "response_valid"), 0);
    assert_eq!(get(sim, "request_ready"), 1);
}

fn oracle(op: u64, word: bool, memory: u64, source: u64) -> Option<u64> {
    let value = if word {
        let (a, b) = (memory as u32, source as u32);
        (match op {
            0 => a.wrapping_add(b),
            1 => b,
            4 => a ^ b,
            8 => a | b,
            12 => a & b,
            16 => (a as i32).min(b as i32) as u32,
            20 => (a as i32).max(b as i32) as u32,
            24 => a.min(b),
            28 => a.max(b),
            _ => return None,
        }) as u64
    } else {
        match op {
            0 => memory.wrapping_add(source),
            1 => source,
            4 => memory ^ source,
            8 => memory | source,
            12 => memory & source,
            16 => (memory as i64).min(source as i64) as u64,
            20 => (memory as i64).max(source as i64) as u64,
            24 => memory.min(source),
            28 => memory.max(source),
            _ => return None,
        }
    };
    Some(value)
}

fn transaction(
    sim: &mut Simulator<NativeBackend>,
    op: u64,
    word: bool,
    a: u64,
    b: u64,
    stall: bool,
) {
    assert_eq!(get(sim, "request_ready"), 1);
    set(sim, "request_valid", 1);
    set(sim, "response_ready", 0);
    set(sim, "operation", op);
    set(sim, "word_op", u64::from(word));
    set(sim, "memory_value", a);
    set(sim, "source_value", b);
    tick(sim);
    assert_eq!(get(sim, "request_ready"), 0);
    assert_eq!(get(sim, "response_valid"), 0);
    // Live inputs must not corrupt the captured operation, even if a second
    // producer holds valid while this transaction is busy.
    set(sim, "operation", 31);
    set(sim, "word_op", u64::from(!word));
    set(sim, "memory_value", !a);
    set(sim, "source_value", !b);
    tick(sim);
    assert_eq!(get(sim, "response_valid"), 0);
    tick(sim);
    assert_eq!(get(sim, "response_valid"), 0);
    tick(sim);
    let expected = oracle(op, word, a, b);
    let rd = if word {
        a as u32 as i32 as i64 as u64
    } else {
        a
    };
    for _ in 0..if stall { 4 } else { 1 } {
        assert_eq!(get(sim, "response_valid"), 1);
        assert_eq!(get(sim, "request_ready"), 0);
        assert_eq!(get(sim, "illegal"), u64::from(expected.is_none()));
        assert_eq!(get(sim, "rd_value"), rd);
        assert_eq!(
            get(sim, "write_value"),
            expected.unwrap_or(0),
            "op={op} word={word} a={a:x} b={b:x}"
        );
        tick(sim);
    }
    set(sim, "request_valid", 0);
    set(sim, "response_ready", 1);
    tick(sim);
    assert_eq!(get(sim, "response_valid"), 0);
    assert_eq!(get(sim, "request_ready"), 1);
}

#[test]
fn native_and_mapped_amo_width_signedness_backpressure_and_reset() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/AtomicAlu.veryl"))
            .unwrap();
    let native = SimulatorBuilder::new(&source, "AtomicAlu")
        .build_native()
        .unwrap();
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "atomic_test", "AtomicAlu").unwrap();
    let mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        struo_target_ecp5::MappingOptions {
            timing_goal_mhz: 125,
            ..Default::default()
        },
    )
    .unwrap();
    let mapped = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    for mut sim in [native, mapped] {
        set(&mut sim, "request_valid", 0);
        set(&mut sim, "response_ready", 0);
        reset(&mut sim);
        let corners = [
            0,
            1,
            u64::MAX,
            0x7fff_ffff,
            0x8000_0000,
            0xffff_ffff,
            0x8000_0000_0000_0000,
            0x7fff_ffff_ffff_ffff,
            0xffff_ffff_0000_0001,
            0x1234_5678_8000_0000,
        ];
        for op in 0..32 {
            for word in [false, true] {
                for a in corners {
                    for b in corners {
                        transaction(&mut sim, op, word, a, b, true);
                    }
                }
            }
        }
        let mut random = 0x6723_14ef_abc9_0021_u64;
        for i in 0..2048 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let a = random;
            random = random.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            transaction(
                &mut sim,
                [0, 1, 4, 8, 12, 16, 20, 24, 28][i % 9],
                i % 2 == 0,
                a,
                random,
                false,
            );
        }
        // Reset aborts both an in-flight operation and an unconsumed response.
        for edges in [1, 2, 3, 4] {
            set(&mut sim, "request_valid", 1);
            set(&mut sim, "response_ready", 0);
            for _ in 0..edges {
                tick(&mut sim);
            }
            reset(&mut sim);
            set(&mut sim, "request_valid", 0);
            tick(&mut sim);
            assert_eq!(get(&mut sim, "response_valid"), 0);
            transaction(&mut sim, 0, false, 7, 11, false);
        }
    }
}
