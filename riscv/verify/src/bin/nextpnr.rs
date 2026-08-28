use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{JtaggBinding, MappingOptions, PllBinding, map_to_ecp5_with_options};

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let top = env::args().nth(1).unwrap_or_else(|| "Core".to_owned());
    let (output_name, timing_goal_mhz) = match top.as_str() {
        "Core" => ("riscv-126.nextpnr.json", 126),
        "Top" => ("riscv-top-126.nextpnr.json", 126),
        "HardwareTestTop" => ("riscv-hardware-test-126.nextpnr.json", 126),
        // Keep the standalone ALU diagnostics at their intentionally harder
        // 150 MHz target even though the integrated CPU now targets 126 MHz.
        "FullAluTimingTop" => ("alu-full-single-cycle-150.nextpnr.json", 150),
        "FunnelShiftTimingTop" => ("funnel-shift-single-cycle-150.nextpnr.json", 150),
        _ => {
            return Err(format!(
                "expected Core, Top, HardwareTestTop, FullAluTimingTop, or FunnelShiftTimingTop, received {top}"
            )
            .into());
        }
    };
    let source = [
        "PipelineInterfaces.veryl",
        "RegisterFile.veryl",
        "Decoder.veryl",
        "ExecuteUnit.veryl",
        "ShiftUnit.veryl",
        "ShiftHighUnit.veryl",
        "CompletionUnit.veryl",
        "AluTimingTop.veryl",
        "Core.veryl",
        "Top.veryl",
        "HardwareMemory.veryl",
        "HardwareTestTop.veryl",
    ]
    .into_iter()
    .map(|file| fs::read_to_string(project.join("src").join(file)))
    .collect::<Result<Vec<_>, _>>()?;
    let mut source = source.join("\n");
    if top == "HardwareTestTop" {
        source = source.replace("rica_i2c::", "");
        source.push('\n');
        source.push_str(&fs::read_to_string(
            project.parent().unwrap().join("i2c/src/I2cTarget250.veryl"),
        )?);
    }

    let design = analyze_and_lower(&source, "rica_riscv", &top)?;
    let synthesized = synthesize(&design)?;
    let mut mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz,
            ..MappingOptions::default()
        },
    )?;
    if top == "HardwareTestTop" {
        mapped.bind_jtagg(&JtaggBinding::with_prefix("jtag"))?;
        let pll: PllBinding =
            serde_json::from_str(&fs::read_to_string(project.join("pll-12-to-126.json"))?)?;
        mapped.bind_pll(&pll)?;
    }

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
