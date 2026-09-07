use std::{fs, path::Path};

use celox::{NativeBackend, Simulator, SimulatorBuilder};

struct Unit(Simulator<NativeBackend>);

impl Unit {
    fn with_dsp(use_dsp: bool) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = ["MulDivUnit.veryl", "DspMultiply64.veryl"]
            .map(|name| fs::read_to_string(root.join("src").join(name)).unwrap())
            .join("\n");
        let sim = SimulatorBuilder::new(&source, "MulDivUnit")
            .param("USE_DSP", u64::from(use_dsp))
            .build_native()
            .unwrap();
        Self::reset(sim)
    }

    fn mapped(use_dsp: bool) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = ["MulDivUnit.veryl", "DspMultiply64.veryl"]
            .map(|name| fs::read_to_string(root.join("src").join(name)).unwrap())
            .join("\n")
            .replace(
                "param USE_DSP: bit = 0",
                &format!("param USE_DSP: bit = {}", u8::from(use_dsp)),
            );
        let rtl =
            struo_frontend_veryl::analyze_and_lower(&source, "mapped_mdu", "MulDivUnit").unwrap();
        let mapped = struo_target_ecp5::map_to_ecp5_with_options(
            &struo_synth::synthesize(&rtl).unwrap().netlist,
            struo_target_ecp5::MappingOptions {
                timing_goal_mhz: 175,
                ..Default::default()
            },
        )
        .unwrap();
        Self::reset(
            struo_celox::ecp5_simulator(&mapped)
                .unwrap()
                .build_native()
                .unwrap(),
        )
    }

    fn reset(sim: Simulator<NativeBackend>) -> Self {
        let mut unit = Self(sim);
        for port in [
            "rst_n",
            "start",
            "cancel",
            "consume",
            "word_op",
            "operation",
            "operand_a",
            "operand_b",
        ] {
            unit.set(port, 0);
        }
        unit.tick();
        unit.set("rst_n", 1);
        unit
    }

    fn set(&mut self, port: &str, value: u64) {
        let signal = self.0.signal(port);
        self.0
            .modify(|io| {
                if port.starts_with("operand_") {
                    io.set(signal, value);
                } else {
                    io.set(signal, value as u8);
                }
            })
            .unwrap();
    }

    fn get(&mut self, port: &str) -> u64 {
        let signal = self.0.signal(port);
        self.0
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let event = self.0.event("clk");
        self.0.tick(event).unwrap();
    }

    fn start(&mut self, op: u8, word: bool, a: u64, b: u64) {
        self.set("operation", op.into());
        self.set("word_op", word.into());
        self.set("operand_a", a);
        self.set("operand_b", b);
        self.set("start", 1);
        self.tick();
        self.set("start", 0);
    }

    fn finish(&mut self) -> (u64, usize) {
        for cycle in 0..=70 {
            if self.get("done") != 0 {
                return (self.get("result"), cycle);
            }
            self.tick();
        }
        panic!("operation did not finish");
    }

    fn consume(&mut self) {
        self.set("consume", 1);
        self.tick();
        self.set("consume", 0);
        assert_eq!(self.get("done"), 0);
    }
}

fn reference(op: u8, word: bool, a: u64, b: u64) -> u64 {
    let unsigned_a = if word { u64::from(a as u32) } else { a };
    let unsigned_b = if word { u64::from(b as u32) } else { b };
    let signed_a = if word {
        i128::from(a as i32)
    } else {
        i128::from(a as i64)
    };
    let signed_b = if word {
        i128::from(b as i32)
    } else {
        i128::from(b as i64)
    };
    let value = match op {
        0 => a.wrapping_mul(b),
        1 => ((signed_a * signed_b) >> 64) as u64,
        2 => ((signed_a * i128::from(b)) >> 64) as u64,
        3 => ((u128::from(a) * u128::from(b)) >> 64) as u64,
        4 => {
            if signed_b == 0 {
                u64::MAX
            } else {
                (signed_a / signed_b) as u64
            }
        }
        5 => unsigned_a.checked_div(unsigned_b).unwrap_or(u64::MAX),
        6 => {
            if signed_b == 0 {
                a
            } else {
                (signed_a % signed_b) as u64
            }
        }
        7 => unsigned_a.checked_rem(unsigned_b).unwrap_or(unsigned_a),
        _ => unreachable!(),
    };
    if word {
        value as i32 as i64 as u64
    } else {
        value
    }
}

#[test]
fn mapped_dsp_pipeline_preserves_full_product_and_seven_cycle_latency() {
    use std::collections::VecDeque;
    use struo_target_ecp5::{Ecp5Cell, MappingOptions, map_to_ecp5_with_options};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = fs::read_to_string(root.join("src/DspMultiply64.veryl")).unwrap();
    let rtl =
        struo_frontend_veryl::analyze_and_lower(&source, "mapped_dsp", "DspMultiply64").unwrap();
    let netlist = struo_synth::synthesize(&rtl).unwrap().netlist;
    let mapped = map_to_ecp5_with_options(
        &netlist,
        MappingOptions {
            timing_goal_mhz: 175,
            ..MappingOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        mapped
            .cells()
            .iter()
            .filter(|cell| matches!(cell, Ecp5Cell::Multiplier { .. }))
            .count(),
        16
    );
    let mut sim = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    let a = sim.signal("a");
    let b = sim.signal("b");
    let product = sim.signal("product");
    let clk = sim.event("clk");
    let edge = [
        0,
        1,
        u64::MAX,
        u64::MAX - 1,
        1 << 63,
        (1 << 32) - 1,
        0x1234_5678_9abc_def0,
    ];
    let mut vectors = edge
        .iter()
        .flat_map(|&x| edge.map(|y| (x, y)))
        .collect::<Vec<_>>();
    let mut random = 0xf053_4321_8834_934a_u64;
    for _ in 0..128 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let x = random;
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        vectors.push((x, random));
    }
    vectors.extend([(0, 0); 6]);
    let mut pending = VecDeque::new();
    for (cycle, (x, y)) in vectors.into_iter().enumerate() {
        sim.modify(|io| {
            io.set(a, x);
            io.set(b, y);
        })
        .unwrap();
        sim.tick(clk).unwrap();
        pending.push_back(u128::from(x) * u128::from(y));
        if pending.len() == 7 {
            let words = sim.get(product).to_u64_digits();
            let got = u128::from(words.first().copied().unwrap_or(0))
                | (u128::from(words.get(1).copied().unwrap_or(0)) << 64);
            assert_eq!(
                got,
                pending.pop_front().unwrap(),
                "cycle={cycle}, current a={x:016x}, b={y:016x}, got={got:032x}"
            );
        }
    }
}

#[test]
fn all_rv64m_operations_match_independent_integer_arithmetic() {
    check_arithmetic(false);
}

#[test]
fn dsp_rv64m_operations_match_independent_integer_arithmetic() {
    check_arithmetic(true);
}

fn check_arithmetic(use_dsp: bool) {
    let mut unit = Unit::with_dsp(use_dsp);
    let edges = [
        0,
        1,
        2,
        3,
        u64::MAX,
        u64::MAX - 1,
        1 << 63,
        (1 << 63) - 1,
        0x8000_0000,
        0xffff_ffff,
        0x1234_5678_8765_4321,
    ];
    let mut random = 0x532e_769b_13ae_2171_u64;
    for word in [false, true] {
        for op in 0..8 {
            if word && (1..4).contains(&op) {
                continue;
            }
            let mut pairs = edges
                .iter()
                .flat_map(|&a| edges.map(|b| (a, b)))
                .collect::<Vec<_>>();
            for _ in 0..128 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let a = random;
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                pairs.push((a, random));
            }
            for (a, b) in pairs {
                unit.start(op, word, a, b);
                // Inputs can change after the request without changing its result.
                unit.set("operand_a", !a);
                unit.set("operand_b", !b);
                let (got, cycles) = unit.finish();
                assert_eq!(
                    got,
                    reference(op, word, a, b),
                    "op={op}, word={word}, a={a:x}, b={b:x}"
                );
                assert!(
                    cycles
                        <= if op < 4 {
                            if use_dsp { 11 } else { 36 }
                        } else if word {
                            35
                        } else {
                            67
                        }
                );
                unit.consume();
            }
        }
    }
}

#[test]
fn cancellation_and_result_backpressure_do_not_leak_old_operations() {
    check_cancellation(false);
}

#[test]
fn dsp_cancellation_and_result_backpressure_do_not_leak_old_operations() {
    check_cancellation(true);
}

fn check_cancellation(use_dsp: bool) {
    let mut unit = Unit::with_dsp(use_dsp);
    for cancelled_op in [0, 1, 4] {
        for cycles in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 17, 33, 65, 68] {
            unit.start(cancelled_op, false, 0x8000_0000_0000_0001, 7);
            for _ in 0..cycles {
                unit.tick();
            }
            unit.set("cancel", 1);
            unit.tick();
            unit.set("cancel", 0);
            assert_eq!(unit.get("done"), 0);
            unit.start(0, false, 123, 17);
            let (result, _) = unit.finish();
            assert_eq!(result, 2091);
            // Reasserting start while a result is held cannot replace it.
            unit.set("start", 1);
            unit.set("operand_a", 0);
            for _ in 0..5 {
                unit.tick();
                assert_eq!(unit.get("done"), 1);
                assert_eq!(unit.get("result"), result);
            }
            unit.set("start", 0);
            unit.consume();
        }
    }
}

#[test]
fn mapped_mdu_preserves_native_results_and_completion_cycles() {
    let edge = [
        0,
        1,
        u64::MAX,
        1 << 63,
        (1 << 32) - 1,
        0x1234_5678_9abc_def0,
    ];
    for use_dsp in [false, true] {
        let mut native = Unit::with_dsp(use_dsp);
        let mut mapped = Unit::mapped(use_dsp);
        for word in [false, true] {
            for op in 0..8 {
                if word && (1..4).contains(&op) {
                    continue;
                }
                for a in edge {
                    for b in edge {
                        native.start(op, word, a, b);
                        mapped.start(op, word, a, b);
                        let expected = native.finish();
                        let actual = mapped.finish();
                        assert_eq!(
                            actual, expected,
                            "DSP={use_dsp}, op={op}, word={word}, a={a:016x}, b={b:016x}"
                        );
                        assert_eq!(actual.0, reference(op, word, a, b));
                        native.consume();
                        mapped.consume();
                    }
                }
            }
        }
    }
}
