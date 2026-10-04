//! Compare observable PMP capture state from two actual Core RTL snapshots.
//! Usage: cargo run -p verify-riscv --example pmp_capture_equivalence -- OLD NEW
use celox::SimulatorBuilder;
use std::{env, fs};

fn capture_module(path: &str, name: &str, stages: u32) -> String {
    let source = fs::read_to_string(path).unwrap();
    let start = source
        .find("// At the lower-frequency target, capture address")
        .unwrap();
    let start = start + source[start..].find("always_ff").unwrap();
    let end = start
        + source[start..]
            .find("// Hold the frontend through")
            .unwrap();
    format!(
        r#"module {name} (
            clk: input clock, rst_n: input reset,
            backend_wait: input logic, pmp_data_wait: input logic,
            lsu_address: input logic<64>, pmp_data_last_next: input logic<33>,
            pmp_data_high_next: input logic,
            pmp_data_address_q: output logic<64>, pmp_data_last_q: output logic<33>,
            pmp_data_high_q: output logic, pmp_data_stage_q: output logic<2>,
            pmp_data_allowed_q: output logic,
        ) {{
            const ENABLE_U: bit = 1;
            const PMP_DATA_STAGES: u32 = {stages};
            // Exercise an arbitrary combinational consumer of the saved data.
            // State is compared independently below, so this does not replace
            // the full-core tests of the real permission checker.
            let pmp_data_comb_allowed: logic = pmp_data_address_q[0]
                ^ pmp_data_address_q[31] ^ pmp_data_last_q[17] ^ pmp_data_high_q;
            {}
        }}"#,
        &source[start..end]
    )
}

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "expected OLD-Core.veryl NEW-Core.veryl");
    for stages in [2, 3] {
        let source = capture_module(&args[1], "Before", stages)
            + &capture_module(&args[2], "After", stages)
            + r#"
module CaptureMiter (
    clk: input clock, rst_n: input reset,
    backend_wait: input logic, pmp_data_wait: input logic,
    lsu_address: input logic<64>, pmp_data_last_next: input logic<33>,
    pmp_data_high_next: input logic, mismatch: output logic,
) {
    var a_address: logic<64>; var b_address: logic<64>;
    var a_last: logic<33>; var b_last: logic<33>;
    var a_high: logic; var b_high: logic;
    var a_stage: logic<2>; var b_stage: logic<2>;
    var a_allowed: logic; var b_allowed: logic;
    inst original_capture: Before (
        clk, rst_n, backend_wait, pmp_data_wait, lsu_address,
        pmp_data_last_next, pmp_data_high_next,
        pmp_data_address_q: a_address, pmp_data_last_q: a_last,
        pmp_data_high_q: a_high, pmp_data_stage_q: a_stage,
        pmp_data_allowed_q: a_allowed,
    );
    inst updated_capture: After (
        clk, rst_n, backend_wait, pmp_data_wait, lsu_address,
        pmp_data_last_next, pmp_data_high_next,
        pmp_data_address_q: b_address, pmp_data_last_q: b_last,
        pmp_data_high_q: b_high, pmp_data_stage_q: b_stage,
        pmp_data_allowed_q: b_allowed,
    );
    assign mismatch = (a_stage != b_stage) || (a_allowed != b_allowed)
        || ((a_stage != 2'd0) && (a_address != b_address))
        || ((a_stage >= 2'd2) && ((a_last != b_last) || (a_high != b_high)));
}
"#;
        for mapped in [false, true] {
            let mut sim = if mapped {
                let rtl = struo_frontend_veryl::analyze_and_lower(
                    &source,
                    "pmp_capture_equivalence",
                    "CaptureMiter",
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
                SimulatorBuilder::new(&source, "CaptureMiter")
                    .build_native()
                    .unwrap()
            };
            let clk = sim.signal("clk");
            let reset = sim.signal("rst_n");
            let backend = sim.signal("backend_wait");
            let wait = sim.signal("pmp_data_wait");
            let address = sim.signal("lsu_address");
            let last = sim.signal("pmp_data_last_next");
            let high = sim.signal("pmp_data_high_next");
            let mismatch = sim.signal("mismatch");
            let mut random = 0x17a1_7749_20c5_d601_u64;
            for cycle in 0..16384 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                sim.modify(|io| {
                    io.set(clk, 0u8);
                    io.set(reset, u8::from(cycle != 0 && cycle % 257 != 0));
                    io.set(backend, ((random >> 40) & 1) as u8);
                    io.set(wait, ((random >> 41) & 1) as u8);
                    io.set(address, random);
                    io.set_wide(last, (random.rotate_left(21) & 0x1_ffff_ffff).into());
                    io.set(high, ((random >> 42) & 1) as u8);
                })
                .unwrap();
                sim.modify(|io| io.set(clk, 1u8)).unwrap();
                assert_eq!(
                    sim.get(mismatch)
                        .to_u64_digits()
                        .first()
                        .copied()
                        .unwrap_or(0),
                    0,
                    "stages={stages} mapped={mapped} cycle={cycle}"
                );
            }
            println!("PMP capture equivalence PASS stages={stages} mapped={mapped} cycles=16384");
        }
    }
}
