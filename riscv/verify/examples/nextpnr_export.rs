//! Export a Struo-mapped board top as nextpnr-ecp5 JSON for reference P&R.
//!
//! Usage: nextpnr_export PROJECT_DIR TOP PLL_JSON GOAL_MHZ OUTPUT_JSON
//!
//! Every `src/*.veryl` file of PROJECT_DIR forms one compilation unit, as in
//! a Veryl project. The JTAGG block and the EHXPLLL binding are attached as in
//! the board flows; no CE fanout limits or physical feedback are applied, so
//! two RTL snapshots can be compared under identical mapping settings.

use std::{env, error::Error, fs, path::Path};

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{JtaggBinding, MappingOptions, PllBinding, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [project, top, pll, goal, output] = arguments.as_slice() else {
        return Err("usage: nextpnr_export PROJECT_DIR TOP PLL_JSON GOAL_MHZ OUTPUT_JSON".into());
    };
    let mut files = fs::read_dir(Path::new(project).join("src"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "veryl")
    });
    files.sort();
    let mut sources = files
        .iter()
        .map(fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    // A single compilation unit needs packages before their users.
    sources.sort_by_key(|text| {
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("package "))
    });
    let source = sources.join("\n");
    eprintln!(
        "nextpnr-export: analyze/lower {top} from {} files",
        files.len()
    );
    let design = analyze_and_lower(&source, "rica_riscv", top)?;
    let synthesized = synthesize(&design)?;
    let mut mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: goal.parse()?,
            ..MappingOptions::default()
        },
    )?;
    if !mapped.retiming().equivalence_signed_off {
        return Err("Struo mapping equivalence was not signed off".into());
    }
    mapped.bind_jtagg(&JtaggBinding::with_prefix("jtag"))?;
    let pll: PllBinding = serde_json::from_str(&fs::read_to_string(pll)?)?;
    mapped.bind_pll(&pll)?;
    fs::write(output, mapped.to_nextpnr_json()?)?;
    println!(
        "exported {} cells, LUT depth {} -> {}, {output}",
        mapped.cells().len(),
        mapped.retiming().original_lut_depth,
        mapped.retiming().selected_lut_depth,
    );
    Ok(())
}
