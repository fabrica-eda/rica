use std::error::Error;
use std::fs;
use std::path::Path;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{MappingOptions, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = [
        "PipelineInterfaces.veryl",
        "RegisterFile.veryl",
        "Decoder.veryl",
        "ExecuteUnit.veryl",
        "Core.veryl",
    ]
    .into_iter()
    .map(|file| fs::read_to_string(project.join("src").join(file)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let design = analyze_and_lower(&source, "rica_riscv", "Core")?;
    let synthesized = synthesize(&design)?;
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 150,
            ..MappingOptions::default()
        },
    )?;

    let output = project.join("build/riscv-150.nextpnr.json");
    fs::create_dir_all(output.parent().unwrap())?;
    fs::write(&output, mapped.to_nextpnr_json()?)?;
    println!(
        "Struo mapped nextpnr JSON: {} cells, retimed={}, {}",
        mapped.cells().len(),
        mapped.retiming().applied,
        output.display()
    );
    Ok(())
}
