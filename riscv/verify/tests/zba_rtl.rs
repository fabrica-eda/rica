use std::fs;
use std::path::Path;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TOP: &str = r#"
module ZbaTestTop #(param ENABLE_ZBA: bit = 0) (
    instr: input logic<32>, a: input logic<64>, b: input logic<64>,
    result: output logic<64>, illegal: output logic,
    word_op: output logic, use_rs2: output logic, write_enable: output logic,
) {
    inst d: DecodeBus;
    inst e: ExecuteBus;
    inst h: ShiftHighBus;
    inst decoder: Decoder #(ENABLE_ZBA: ENABLE_ZBA) (decode: d);
    inst execute: ExecuteUnit #(ENABLE_ZBA: ENABLE_ZBA) (execute: e);
    inst high: ShiftHighUnit (shift: h);
    var shifted: logic<64>;
    always_comb {
        d.instr = instr;
        e.alu_op = d.alu_op;
        e.subtract = d.alu_op == 6'd1;
        e.word_op = d.word_op;
        e.link = 1'b0;
        e.operand_a = a;
        e.operand_b = b;
        if d.use_immediate { e.operand_b = d.immediate; }
        h.operand = e.shift_result;
        h.amount = e.operand_b[5:3];
        h.arithmetic = 1'b0;
        for i in 0..64 { shifted[i] = h.result[63 - i]; }
        result = e.arithmetic;
        if e.result_use_logic { result = e.logic_result; }
        if e.result_use_shift { result = shifted; }
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
        "ShiftHighUnit",
    ] {
        source.push_str(&fs::read_to_string(root.join(format!("src/{name}.veryl"))).unwrap());
    }
    source.push_str(TOP);
    source
}

fn simulator(enabled: bool) -> Simulator<NativeBackend> {
    SimulatorBuilder::new(&source(), "ZbaTestTop")
        .param("ENABLE_ZBA", u64::from(enabled))
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
        io.set(ports[0], instruction);
        io.set(ports[1], a);
        io.set(ports[2], b);
    })
    .unwrap();
    value(sim, "result")
}

fn register_instruction(shift: u32, unsigned_word: bool) -> u32 {
    let funct7 = if shift == 0 { 4 } else { 16 };
    funct7 << 25
        | 2 << 20
        | 1 << 15
        | (shift * 2) << 12
        | 3 << 7
        | if unsigned_word { 0x3b } else { 0x33 }
}

#[test]
fn all_zba_operations_match_integer_oracle_and_use_full_width_results() {
    exercise_oracle(simulator(true));
}

#[test]
fn mapped_zba_operations_match_the_same_independent_integer_oracle() {
    let source = source().replace(
        "module ZbaTestTop #(param ENABLE_ZBA: bit = 0)",
        "module ZbaTestTop #(param ENABLE_ZBA: bit = 1)",
    );
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "zba_mapped", "ZbaTestTop").unwrap();
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

fn exercise_oracle(mut sim: Simulator<NativeBackend>) {
    let mut values = vec![
        0,
        1,
        2,
        u64::MAX,
        u64::MAX - 1,
        1 << 63,
        (1 << 63) - 1,
        0x8000_0000,
        0xffff_ffff,
        0xffff_ffff_0000_0000,
        0x0123_4567_89ab_cdef,
    ];
    let mut random = 0x627a_ba91_c7e4_032d_u64;
    for _ in 0..32 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        values.push(random);
    }
    let mut comparisons = 0;
    for uw in [false, true] {
        for shift in u32::from(!uw)..=3 {
            let instruction = register_instruction(shift, uw);
            for &a in &values {
                for &b in &values {
                    let index = if uw { u64::from(a as u32) } else { a };
                    let expected = index.wrapping_shl(shift).wrapping_add(b);
                    assert_eq!(
                        evaluate(&mut sim, instruction, a, b),
                        expected,
                        "instr={instruction:08x} a={a:016x} b={b:016x}"
                    );
                    assert_eq!(value(&mut sim, "illegal"), 0);
                    assert_eq!(value(&mut sim, "word_op"), 0);
                    assert_eq!(value(&mut sim, "write_enable"), 1);
                    assert_eq!(value(&mut sim, "use_rs2"), 1);
                    comparisons += 1;
                }
            }
        }
    }
    for shift in 0..64 {
        let instruction = 0x0800_101b | shift << 20 | 1 << 15 | 3 << 7;
        for &a in &values {
            assert_eq!(
                evaluate(&mut sim, instruction, a, u64::MAX),
                u64::from(a as u32) << shift
            );
            assert_eq!(value(&mut sim, "illegal"), 0);
            assert_eq!(value(&mut sim, "word_op"), 0);
            assert_eq!(value(&mut sim, "use_rs2"), 0);
            comparisons += 1;
        }
    }
    println!("Zba integer oracle: {comparisons} comparisons passed");
}

#[test]
fn zba_enable_and_neighboring_encodings_have_exact_legality() {
    for enabled in [false, true] {
        let mut sim = simulator(enabled);
        for opcode in [0x33, 0x3b, 0x13, 0x1b] {
            for funct7 in [4, 5, 16, 17] {
                for funct3 in 0..8 {
                    let instruction =
                        funct7 << 25 | 2 << 20 | 1 << 15 | funct3 << 12 | 3 << 7 | opcode;
                    let base_legal = (opcode == 0x13 && funct3 != 1 && funct3 != 5)
                        || (opcode == 0x1b && funct3 == 0);
                    let zba_legal = (opcode == 0x3b && funct7 == 4 && funct3 == 0)
                        || ((opcode == 0x33 || opcode == 0x3b)
                            && funct7 == 16
                            && matches!(funct3, 2 | 4 | 6))
                        || (opcode == 0x1b && matches!(funct7, 4 | 5) && funct3 == 1);
                    evaluate(&mut sim, instruction, 0, 0);
                    assert_eq!(
                        value(&mut sim, "illegal") == 0,
                        base_legal || (enabled && zba_legal),
                        "enabled={enabled} instr={instruction:08x}"
                    );
                }
            }
        }
    }
}
