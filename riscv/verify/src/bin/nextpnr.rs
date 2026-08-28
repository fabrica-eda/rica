use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{MappingOptions, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let top = env::args().nth(1).unwrap_or_else(|| "Core".to_owned());
    let output_name = match top.as_str() {
        "Core" => "riscv-150.nextpnr.json",
        "Top" => "riscv-top-150.nextpnr.json",
        _ => return Err(format!("expected Core or Top, received {top}").into()),
    };
    let source = [
        "PipelineInterfaces.veryl",
        "RegisterFile.veryl",
        "Decoder.veryl",
        "ExecuteUnit.veryl",
        "Core.veryl",
        "Top.veryl",
    ]
    .into_iter()
    .map(|file| fs::read_to_string(project.join("src").join(file)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let design = analyze_and_lower(&source, "rica_riscv", &top)?;
    let synthesized = synthesize(&design)?;
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 150,
            ..MappingOptions::default()
        },
    )?;

    let output = project.join("build").join(output_name);
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
