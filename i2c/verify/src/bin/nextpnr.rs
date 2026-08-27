use std::error::Error;
use std::fs;
use std::path::Path;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{JtaggBinding, MappingOptions, PllBinding, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = [
        "src/I2cTarget250.veryl",
        "src/I2cDebugTop.veryl",
        "src/ClockDomainProbe.veryl",
        "src/JtagI2cDebugTop.veryl",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(project.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let design = analyze_and_lower(&source, "rica_i2c", "JtagI2cDebugTop")?;
    let synthesized = synthesize(&design)?;
    let mut mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 300,
            ..MappingOptions::default()
        },
    )?;
    mapped.bind_jtagg(&JtaggBinding::with_prefix("jtag"))?;
    let pll: PllBinding =
        serde_json::from_str(&fs::read_to_string(project.join("pll-12-to-250.json"))?)?;
    mapped.bind_pll(&pll)?;

    let output = project.join("build/jtag-i2c-er2.nextpnr.json");
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
