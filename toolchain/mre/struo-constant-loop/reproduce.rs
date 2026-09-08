//! Compare a constant loop-index expression with an elaboration constant.
use std::error::Error;
use std::fs;
use std::path::Path;

use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{MappingOptions, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../toolchain/mre/struo-constant-loop");
    for top in ["Procedural", "Elaborated"] {
        let source = fs::read_to_string(root.join(format!("{top}.veryl")))?;
        let rtl = analyze_and_lower(&source, "constant_loop", top)?;
        let synthesized = synthesize(&rtl)?;
        let mapped = map_to_ecp5_with_options(
            &synthesized.netlist,
            MappingOptions {
                timing_goal_mhz: 175,
                ..MappingOptions::default()
            },
        )?;
        let mut sim = ecp5_simulator(&mapped)?.build_native()?;
        let sp = sim.signal("sp");
        let slots = sim.signal("slots");
        for pointer in 0..8_u8 {
            sim.modify(|io| io.set(sp, pointer))?;
            let observed = sim.get(slots).to_u32_digits().first().copied().unwrap_or(0);
            assert_eq!(observed, 1 << ((pointer + 7) % 8));
        }
        println!(
            "{top}: arithmetic={}, mapped_cells={}, all 8 inputs PASS",
            synthesized.netlist.arithmetic().len(),
            mapped.cells().len(),
        );
        if top == "Elaborated" {
            assert!(synthesized.netlist.arithmetic().is_empty());
        }
    }
    Ok(())
}
