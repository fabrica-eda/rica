use std::fs;
use std::path::Path;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TOP: &str = r#"
module ZbbTestTop #(param ENABLE_ZBB: bit = 0, param EARLY_SIMPLE: bit = 0) (
    instr: input logic<32>, a: input logic<64>, b: input logic<64>,
    result: output logic<64>, illegal: output logic,
    word_op: output logic, use_rs2: output logic, write_enable: output logic,
) {
    inst d: DecodeBus;
    inst e: ExecuteBus;
    inst decoder: Decoder #(ENABLE_ZBB: ENABLE_ZBB) (decode: d);
    inst execute: ExecuteUnit (execute: e);
    var partial: logic<64>;
    var operand_b: logic<64>;
    var slow_result: logic<64>;
    var simple_result: logic<64>;
    inst simple: SimpleBitManipUnit #(ENABLE_ZBB: ENABLE_ZBB) (
        operation: d.alu_op[4:0], operand_a: a, operand_b: operand_b, result: simple_result,
    );
    inst low: BitManipUnit #(EARLY_SIMPLE: EARLY_SIMPLE) (
        operation: d.alu_op[4:0], word_op: d.word_op,
        operand_a: a, operand_b: operand_b, partial: partial,
    );
    inst high: BitManipHighUnit #(EARLY_SIMPLE: EARLY_SIMPLE) (
        operation: d.alu_op[4:0], word_op: d.word_op, amount: operand_b[5:3],
        partial: partial, operand_b: operand_b,
        compare_equal: e.compare_equal, compare_less: e.compare_less,
        sign_a: e.compare_sign_a, sign_b: e.compare_sign_b, result: slow_result,
    );
    always_comb {
        result = slow_result;
        if EARLY_SIMPLE { result = slow_result | simple_result; }
        d.instr = instr;
        operand_b = b;
        if d.use_immediate { operand_b = d.immediate; }
        e.alu_op = d.alu_op;
        e.zba_control = 3'd0;
        e.subtract = 1'b0;
        e.word_op = d.word_op;
        e.link = 1'b0;
        e.operand_a = a;
        e.operand_b = operand_b;
        illegal = d.illegal;
        word_op = d.word_op;
        use_rs2 = d.use_rs2;
        write_enable = d.write_enable;
    }
}
"#;

fn source() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut source = String::new();
    for name in [
        "PipelineInterfaces",
        "Decoder",
        "ExecuteUnit",
        "ShiftUnit",
        "BitManipUnit",
    ] {
        source.push_str(&fs::read_to_string(root.join(format!("src/{name}.veryl"))).unwrap());
    }
    source.push_str(TOP);
    source
}

fn simulator(enabled: bool) -> Simulator<NativeBackend> {
    SimulatorBuilder::new(&source(), "ZbbTestTop")
        .param("ENABLE_ZBB", u64::from(enabled))
        .build_native()
        .unwrap()
}

fn value(sim: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    sim.get(sim.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn evaluate(sim: &mut Simulator<NativeBackend>, instruction: u32, a: u64, b: u64) -> u64 {
    let ports = [sim.signal("instr"), sim.signal("a"), sim.signal("b")];
    sim.modify(|io| {
        io.set(ports[0], instruction | 1 << 15 | 3 << 7);
        io.set(ports[1], a);
        io.set(ports[2], b);
    })
    .unwrap();
    value(sim, "result")
}

fn sext32(x: u32) -> u64 {
    x as i32 as i64 as u64
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Andn,
    Orn,
    Xnor,
    Min,
    Minu,
    Max,
    Maxu,
    Rol,
    Ror,
    Rolw,
    Rorw,
    Clz,
    Ctz,
    Cpop,
    Clzw,
    Ctzw,
    Cpopw,
    Sextb,
    Sexth,
    Zexth,
    Orcb,
    Rev8,
}

impl Op {
    fn encoding(self) -> u32 {
        match self {
            Self::Andn => 0x40007033,
            Self::Orn => 0x40006033,
            Self::Xnor => 0x40004033,
            Self::Min => 0x0a004033,
            Self::Minu => 0x0a005033,
            Self::Max => 0x0a006033,
            Self::Maxu => 0x0a007033,
            Self::Rol => 0x60001033,
            Self::Ror => 0x60005033,
            Self::Rolw => 0x6000103b,
            Self::Rorw => 0x6000503b,
            Self::Clz => 0x60001013,
            Self::Ctz => 0x60101013,
            Self::Cpop => 0x60201013,
            Self::Clzw => 0x6000101b,
            Self::Ctzw => 0x6010101b,
            Self::Cpopw => 0x6020101b,
            Self::Sextb => 0x60401013,
            Self::Sexth => 0x60501013,
            Self::Zexth => 0x0800403b,
            Self::Orcb => 0x28705013,
            Self::Rev8 => 0x6b805013,
        }
    }
    fn oracle(self, a: u64, b: u64) -> u64 {
        match self {
            Self::Andn => a & !b,
            Self::Orn => a | !b,
            Self::Xnor => !(a ^ b),
            Self::Min => (a as i64).min(b as i64) as u64,
            Self::Minu => a.min(b),
            Self::Max => (a as i64).max(b as i64) as u64,
            Self::Maxu => a.max(b),
            Self::Rol => a.rotate_left(b as u32),
            Self::Ror => a.rotate_right(b as u32),
            Self::Rolw => sext32((a as u32).rotate_left(b as u32)),
            Self::Rorw => sext32((a as u32).rotate_right(b as u32)),
            Self::Clz => a.leading_zeros().into(),
            Self::Ctz => a.trailing_zeros().into(),
            Self::Cpop => a.count_ones().into(),
            Self::Clzw => (a as u32).leading_zeros().into(),
            Self::Ctzw => (a as u32).trailing_zeros().into(),
            Self::Cpopw => (a as u32).count_ones().into(),
            Self::Sextb => a as i8 as i64 as u64,
            Self::Sexth => a as i16 as i64 as u64,
            Self::Zexth => a as u16 as u64,
            Self::Orcb => u64::from_le_bytes(a.to_le_bytes().map(|b| if b == 0 { 0 } else { 255 })),
            Self::Rev8 => a.swap_bytes(),
        }
    }
}

fn exercise_oracle(mut sim: Simulator<NativeBackend>) {
    let mut values = vec![
        0,
        1,
        2,
        u64::MAX,
        u64::MAX - 1,
        0x80000000,
        0xffffffff,
        0xffffffff00000000,
        0x0123456789abcdef,
        0x0100010001000100,
        0x8000800080008000,
    ];
    for bit in 0..64 {
        values.extend([1u64 << bit, !(1u64 << bit)]);
    }
    let mut random = 0xbbaab45e91234102u64;
    for _ in 0..16 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        values.push(random);
    }
    let mut comparisons = 0;
    let mut check = |op: Op, encoding: u32, a: u64, b: u64| {
        assert_eq!(
            evaluate(&mut sim, encoding, a, b),
            op.oracle(a, b),
            "{op:?} instr={encoding:08x} a={a:016x} b={b:016x}"
        );
        assert_eq!(value(&mut sim, "illegal"), 0, "{op:?}");
        assert_eq!(value(&mut sim, "write_enable"), 1, "{op:?}");
        comparisons += 1;
    };
    for op in [
        Op::Andn,
        Op::Orn,
        Op::Xnor,
        Op::Min,
        Op::Minu,
        Op::Max,
        Op::Maxu,
    ] {
        for &a in &values {
            for &b in &values {
                check(op, op.encoding() | 2 << 20, a, b);
            }
        }
    }
    for op in [Op::Rol, Op::Ror, Op::Rolw, Op::Rorw] {
        for &a in &values {
            for amount in 0..128 {
                check(op, op.encoding() | 2 << 20, a, 0xfedc000000000000 | amount);
            }
        }
    }
    for (op, encoding, amounts) in [(Op::Ror, 0x60005013, 64), (Op::Rorw, 0x6000501b, 32)] {
        for &a in &values {
            for amount in 0..amounts {
                check(op, encoding | amount << 20, a, amount.into());
            }
        }
    }
    for op in [
        Op::Clz,
        Op::Ctz,
        Op::Cpop,
        Op::Clzw,
        Op::Ctzw,
        Op::Cpopw,
        Op::Sextb,
        Op::Sexth,
        Op::Zexth,
        Op::Orcb,
        Op::Rev8,
    ] {
        for &a in &values {
            check(op, op.encoding(), a, u64::MAX);
        }
    }
    for byte in 0..256u64 {
        for lane in 0..8 {
            let a = byte << (lane * 8);
            for op in [Op::Clz, Op::Ctz, Op::Cpop, Op::Clzw, Op::Ctzw, Op::Cpopw] {
                check(op, op.encoding(), a, 0);
            }
        }
    }
    for _ in 0..4096 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        for op in [Op::Clz, Op::Ctz, Op::Cpop, Op::Clzw, Op::Ctzw, Op::Cpopw] {
            check(op, op.encoding(), random, 0);
        }
    }
    println!("Zbb integer oracle: {comparisons} comparisons passed");
}

#[test]
fn all_24_zbb_instructions_match_independent_integer_oracle() {
    exercise_oracle(simulator(true));
}

fn mapped_oracle(early: bool) {
    let source = source().replace("param ENABLE_ZBB: bit = 0", "param ENABLE_ZBB: bit = 1");
    let source = if early {
        source.replace("param EARLY_SIMPLE: bit = 0", "param EARLY_SIMPLE: bit = 1")
    } else {
        source
    };
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "zbb_mapped", "ZbbTestTop").unwrap();
    let synthesized = struo_synth::synthesize(&rtl).unwrap();
    let mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &synthesized.netlist,
        struo_target_ecp5::MappingOptions {
            timing_goal_mhz: 175,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(mapped.retiming().equivalence_signed_off);
    exercise_oracle(
        struo_celox::ecp5_simulator(&mapped)
            .unwrap()
            .build_native()
            .unwrap(),
    );
}

#[test]
fn reserved_encodings_and_disabled_zbb_are_illegal() {
    for enabled in [false, true] {
        let mut sim = simulator(enabled);
        // Every immediate payload checks exact unary selectors and roriw shamt[5].
        for opcode in [0x13, 0x1b, 0x33, 0x3b] {
            for payload in 0..4096 {
                for f3 in [1, 4, 5, 6, 7] {
                    let instruction = payload << 20 | f3 << 12 | opcode;
                    let f7 = payload >> 5;
                    let base = if opcode == 0x13 {
                        !matches!(f3, 1 | 5)
                            || (f3 == 1 && payload >> 6 == 0)
                            || (f3 == 5 && matches!(payload >> 6, 0 | 16))
                    } else if opcode == 0x1b {
                        (f3 == 1 && f7 == 0) || (f3 == 5 && matches!(f7, 0 | 32))
                    } else if opcode == 0x33 {
                        f7 == 0 || (f7 == 32 && f3 == 5)
                    } else {
                        (f7 == 0 && matches!(f3, 1 | 5)) || (f7 == 32 && f3 == 5)
                    };
                    let zbb = (opcode == 0x33 && f7 == 32 && matches!(f3, 4 | 6 | 7))
                        || (opcode == 0x33 && f7 == 5 && f3 >= 4)
                        || (matches!(opcode, 0x33 | 0x3b) && f7 == 48 && matches!(f3, 1 | 5))
                        || (opcode == 0x13 && f3 == 5 && payload >> 6 == 24)
                        || (opcode == 0x1b && f3 == 5 && f7 == 48)
                        || (matches!(opcode, 0x13 | 0x1b)
                            && f3 == 1
                            && matches!(payload, 0x600..=0x602))
                        || (opcode == 0x13 && f3 == 1 && matches!(payload, 0x604 | 0x605))
                        || (opcode == 0x3b && f3 == 4 && payload == 0x080)
                        || (opcode == 0x13 && f3 == 5 && matches!(payload, 0x287 | 0x6b8));
                    evaluate(&mut sim, instruction, 0, 0);
                    assert_eq!(
                        value(&mut sim, "illegal") == 0,
                        base || (enabled && zbb),
                        "enabled={enabled} instr={instruction:08x}"
                    );
                }
            }
        }
    }
}

#[test]
fn oracle_instruction_bytes_match_the_external_assembler() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let register = [
        ("andn", Op::Andn),
        ("orn", Op::Orn),
        ("xnor", Op::Xnor),
        ("min", Op::Min),
        ("minu", Op::Minu),
        ("max", Op::Max),
        ("maxu", Op::Maxu),
        ("rol", Op::Rol),
        ("ror", Op::Ror),
        ("rolw", Op::Rolw),
        ("rorw", Op::Rorw),
    ];
    let unary = [
        ("clz", Op::Clz),
        ("ctz", Op::Ctz),
        ("cpop", Op::Cpop),
        ("clzw", Op::Clzw),
        ("ctzw", Op::Ctzw),
        ("cpopw", Op::Cpopw),
        ("sext.b", Op::Sextb),
        ("sext.h", Op::Sexth),
        ("zext.h", Op::Zexth),
        ("orc.b", Op::Orcb),
        ("rev8", Op::Rev8),
    ];
    let mut cases = Vec::new();
    for (name, op) in register {
        cases.push((format!("{name} x3, x1, x2"), op.encoding() | 2 << 20));
    }
    for (name, op) in unary {
        cases.push((format!("{name} x3, x1"), op.encoding()));
    }
    for (name, base, amounts) in [("rori", 0x60005013, 64), ("roriw", 0x6000501b, 32)] {
        for amount in 0..amounts {
            cases.push((format!("{name} x3, x1, {amount}"), base | amount << 20));
        }
    }
    let program = cases
        .iter()
        .map(|(asm, _)| asm.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut child = Command::new("/usr/lib/llvm-20/bin/llvm-mc")
        .args(["-triple=riscv64", "-mattr=+zbb", "--show-encoding"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(program.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let encoded = text
        .lines()
        .filter_map(|line| {
            line.split_once("encoding: [").map(|(_, tail)| {
                let bytes = tail
                    .trim_end_matches(']')
                    .split(',')
                    .map(|b| u8::from_str_radix(b.trim().trim_start_matches("0x"), 16).unwrap())
                    .collect::<Vec<_>>();
                u32::from_le_bytes(bytes.try_into().unwrap())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(encoded.len(), cases.len());
    for ((assembly, instruction), actual) in cases.iter().zip(encoded) {
        assert_eq!(*instruction | 1 << 15 | 3 << 7, actual, "{assembly}");
    }
}

#[test]
fn mapped_zbb_matches_the_same_independent_integer_oracle() {
    mapped_oracle(false);
}

#[test]
fn early_simple_zbb_matches_independent_integer_oracle() {
    exercise_oracle(
        SimulatorBuilder::new(&source(), "ZbbTestTop")
            .param("ENABLE_ZBB", 1)
            .param("EARLY_SIMPLE", 1)
            .build_native()
            .unwrap(),
    );
}

#[test]
fn mapped_early_simple_zbb_matches_independent_integer_oracle() {
    mapped_oracle(true);
}
