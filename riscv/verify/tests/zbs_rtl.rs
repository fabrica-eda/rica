use std::fs;
use std::path::Path;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TOP: &str = r#"
module ZbsTestTop #(param ENABLE_ZBS: bit = 0) (
    instr: input logic<32>, a: input logic<64>, b: input logic<64>,
    result: output logic<64>, illegal: output logic,
    word_op: output logic, use_rs2: output logic, write_enable: output logic,
) {
    inst d: DecodeBus;
    inst e: ExecuteBus;
    inst decoder: Decoder #(ENABLE_ZBS: ENABLE_ZBS) (decode: d);
    inst execute: ExecuteUnit (execute: e);
    var partial: logic<64>;
    var operand_b: logic<64>;
    inst low: BitManipUnit (
        operation: d.alu_op[4:0], word_op: d.word_op,
        operand_a: a, operand_b: operand_b, partial: partial,
    );
    inst high: BitManipHighUnit (
        operation: d.alu_op[4:0], word_op: d.word_op, amount: operand_b[5:3],
        partial: partial, operand_b: operand_b,
        compare_equal: e.compare_equal, compare_less: e.compare_less,
        sign_a: e.compare_sign_a, sign_b: e.compare_sign_b, result: result,
    );
    always_comb {
        d.instr = instr;
        operand_b = b;
        if d.use_immediate { operand_b = d.immediate; }
        e.alu_op = d.alu_op;
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
    SimulatorBuilder::new(&source(), "ZbsTestTop")
        .param("ENABLE_ZBS", u64::from(enabled))
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

#[derive(Clone, Copy, Debug)]
enum Op {
    Clear,
    Extract,
    Invert,
    Set,
}
impl Op {
    fn encoding(self) -> u32 {
        match self {
            Self::Clear => 0x48001033,
            Self::Extract => 0x48005033,
            Self::Invert => 0x68001033,
            Self::Set => 0x28001033,
        }
    }
    fn oracle(self, a: u64, b: u64) -> u64 {
        let bit = b % 64;
        match self {
            Self::Clear => a & !(1 << bit),
            Self::Extract => (a >> bit) & 1,
            Self::Invert => a ^ (1 << bit),
            Self::Set => a | (1 << bit),
        }
    }
}
fn exercise_oracle(mut sim: Simulator<NativeBackend>) {
    let mut values = vec![0, 1, u64::MAX, 0xffffffff00000000, 0x0123456789abcdef];
    for bit in 0..64 {
        values.extend([1u64 << bit, !(1u64 << bit)]);
    }
    let mut random = 0x392db7a72fbbc584u64;
    for _ in 0..64 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        values.push(random);
    }
    let mut comparisons = 0;
    for op in [Op::Clear, Op::Extract, Op::Invert, Op::Set] {
        for &a in &values {
            for bit in 0..64 {
                for high in [0, 64, 0xffff_ffff_ffff_ffc0] {
                    let b = bit | high;
                    assert_eq!(
                        evaluate(&mut sim, op.encoding() | 2 << 20, a, b),
                        op.oracle(a, b),
                        "{op:?} a={a:016x} b={b:016x}"
                    );
                    assert_eq!(value(&mut sim, "illegal"), 0);
                    assert_eq!(value(&mut sim, "word_op"), 0);
                    assert_eq!(value(&mut sim, "use_rs2"), 1);
                    assert_eq!(value(&mut sim, "write_enable"), 1);
                    comparisons += 1;
                }
                let imm = (op.encoding() ^ 0x20) | (bit as u32) << 20;
                assert_eq!(
                    evaluate(&mut sim, imm, a, !bit),
                    op.oracle(a, bit),
                    "immediate {op:?} a={a:016x} bit={bit}"
                );
                assert_eq!(value(&mut sim, "illegal"), 0);
                assert_eq!(value(&mut sim, "word_op"), 0);
                assert_eq!(value(&mut sim, "use_rs2"), 0);
                assert_eq!(value(&mut sim, "write_enable"), 1);
                comparisons += 1;
            }
        }
    }
    println!("Zbs integer oracle: {comparisons} comparisons passed");
}

#[test]
fn all_eight_zbs_instructions_match_independent_integer_oracle() {
    exercise_oracle(simulator(true));
}

#[test]
fn mapped_zbs_matches_the_same_independent_integer_oracle() {
    let source = source().replace(
        "module ZbsTestTop #(param ENABLE_ZBS: bit = 0)",
        "module ZbsTestTop #(param ENABLE_ZBS: bit = 1)",
    );
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "zbs_mapped", "ZbsTestTop").unwrap();
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
fn reserved_encodings_and_disabled_zbs_are_illegal() {
    let mut checks = 0;
    for enabled in [false, true] {
        let mut sim = simulator(enabled);
        for opcode in [0x13, 0x1b, 0x33, 0x3b] {
            for payload in 0..4096 {
                for f3 in 0..8 {
                    let instruction = payload << 20 | f3 << 12 | opcode;
                    let f7 = payload >> 5;
                    let base = match opcode {
                        0x13 => {
                            !matches!(f3, 1 | 5)
                                || (f3 == 1 && payload >> 6 == 0)
                                || (f3 == 5 && matches!(payload >> 6, 0 | 16))
                        }
                        0x1b => {
                            f3 == 0 || (f3 == 1 && f7 == 0) || (f3 == 5 && matches!(f7, 0 | 32))
                        }
                        0x33 => f7 == 0 || (f7 == 32 && matches!(f3, 0 | 5)),
                        0x3b => {
                            (f7 == 0 && matches!(f3, 0 | 1 | 5))
                                || (f7 == 32 && matches!(f3, 0 | 5))
                        }
                        _ => unreachable!(),
                    };
                    let zbs = (opcode == 0x13
                        && ((f3 == 1 && matches!(payload >> 6, 0x0a | 0x12 | 0x1a))
                            || (f3 == 5 && payload >> 6 == 0x12)))
                        || (opcode == 0x33
                            && ((f3 == 1 && matches!(f7, 0x14 | 0x24 | 0x34))
                                || (f3 == 5 && f7 == 0x24)));
                    evaluate(&mut sim, instruction, 0, 0);
                    assert_eq!(
                        value(&mut sim, "illegal") == 0,
                        base || (enabled && zbs),
                        "enabled={enabled} instr={instruction:08x}"
                    );
                    checks += 1;
                }
            }
        }
    }
    println!("Zbs legality sweep: {checks} encodings passed");
}

#[test]
fn oracle_instruction_bytes_match_the_external_assembler() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cases = Vec::new();
    for (name, op) in [
        ("bclr", Op::Clear),
        ("bext", Op::Extract),
        ("binv", Op::Invert),
        ("bset", Op::Set),
    ] {
        cases.push((format!("{name} x3, x1, x2"), op.encoding() | 2 << 20));
        for bit in 0..64 {
            cases.push((
                format!("{name}i x3, x1, {bit}"),
                (op.encoding() ^ 0x20) | bit << 20,
            ));
        }
    }
    let program = cases
        .iter()
        .map(|(asm, _)| asm.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut child = Command::new("/usr/lib/llvm-20/bin/llvm-mc")
        .args(["-triple=riscv64", "-mattr=+zbs", "--show-encoding"])
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
