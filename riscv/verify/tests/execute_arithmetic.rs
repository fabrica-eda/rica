use std::fs;
use std::path::Path;

use celox::SimulatorBuilder;

const TOP: &str = r#"
module ArithmeticTop (
    a: input logic<64>, b: input logic<64>, subtract: input logic, word_op: input logic,
    result: output logic<64>,
) {
    inst e: ExecuteBus;
    inst execute: ExecuteUnit (execute: e);
    always_comb {
        e.alu_op = if subtract ? 4'd1 : 4'd0;
        e.subtract = subtract;
        e.word_op = word_op;
        e.link = 1'b0;
        e.operand_a = a;
        e.operand_b = b;
        result = e.arithmetic;
    }
}
"#;

fn source() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut source = String::new();
    for name in ["PipelineInterfaces", "ExecuteUnit", "ShiftUnit"] {
        source.push_str(&fs::read_to_string(root.join(format!("src/{name}.veryl"))).unwrap());
    }
    source + TOP
}

fn expected(a: u64, b: u64, subtract: bool, word_op: bool) -> u64 {
    let sum = if subtract {
        a.wrapping_sub(b)
    } else {
        a.wrapping_add(b)
    };
    if word_op {
        sum as u32 as i32 as i64 as u64
    } else {
        sum
    }
}

#[test]
fn add_subtract_and_word_results_match_wrapping_integer_arithmetic() {
    // Operands concentrate on carries and borrows that cross the 16-bit
    // slice boundaries, plus random values.
    let mut operands = vec![0, 1, u64::MAX, 1 << 63, (1 << 63) - 1];
    for bit in [15, 16, 31, 32, 47, 48, 63] {
        for delta in [-1_i64, 0, 1] {
            operands.push((1_u64 << bit).wrapping_add_signed(delta));
            operands.push(((1_u64 << bit) - 1).wrapping_add_signed(delta));
        }
    }
    let mut seed = 0x6a09_e667_f3bc_c908_u64;
    for _ in 0..48 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        operands.push(seed);
    }
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(
                &source(),
                "execute_arithmetic",
                "ArithmeticTop",
            )
            .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source(), "ArithmeticTop")
                .build_native()
                .unwrap()
        };
        let [a, b, subtract, word_op, result] =
            ["a", "b", "subtract", "word_op", "result"].map(|name| sim.signal(name));
        let mut checked = 0;
        for &x in &operands {
            for &y in &operands {
                for sub in [false, true] {
                    for word in [false, true] {
                        sim.modify(|io| {
                            io.set(a, x);
                            io.set(b, y);
                            io.set(subtract, u8::from(sub));
                            io.set(word_op, u8::from(word));
                        })
                        .unwrap();
                        let got = sim
                            .get(result)
                            .to_u64_digits()
                            .first()
                            .copied()
                            .unwrap_or(0);
                        assert_eq!(
                            got,
                            expected(x, y, sub, word),
                            "mapped={mapped} a={x:x} b={y:x} subtract={sub} word={word}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 30_000, "{checked}");
    }
}
