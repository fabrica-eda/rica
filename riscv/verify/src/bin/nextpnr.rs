use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{JtaggBinding, MappingOptions, PllBinding, map_to_ecp5_with_options};

fn validated_pll(path: &Path) -> Result<PllBinding, Box<dyn Error>> {
    let pll: PllBinding = serde_json::from_str(&fs::read_to_string(path)?)?;
    let input_mhz = pll
        .attributes
        .get("FREQUENCY_PIN_CLKI")
        .ok_or("board PLL has no FREQUENCY_PIN_CLKI")?
        .parse::<f64>()?;
    let input_divider = pll
        .parameters
        .get("CLKI_DIV")
        .ok_or("board PLL has no CLKI_DIV")?
        .parse::<u32>()?;
    if input_divider == 0 {
        return Err("board PLL CLKI_DIV must not be zero".into());
    }
    let phase_detector_mhz = input_mhz / f64::from(input_divider);
    if phase_detector_mhz <= 10.0 {
        return Err(format!(
            "board PLL phase detector is {phase_detector_mhz} MHz; ECP5 period/cycle jitter is specified only above 10 MHz"
        )
        .into());
    }
    Ok(pll)
}

fn board_pll(project: &Path) -> Result<PllBinding, Box<dyn Error>> {
    validated_pll(&project.join("pll-12-to-124p8.json"))
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let top = env::args().nth(1).unwrap_or_else(|| "Core".to_owned());
    let (output_name, timing_goal_mhz) = match top.as_str() {
        "Core" => ("riscv-125.nextpnr.json", 125),
        "Top" => ("riscv-top-125.nextpnr.json", 125),
        "HardwareTestTop" => ("riscv-hardware-test-125.nextpnr.json", 125),
        // Keep the standalone ALU diagnostics at their intentionally harder
        // 150 MHz target even though the integrated CPU now targets 125 MHz.
        "FullAluTimingTop" => ("alu-full-single-cycle-150.nextpnr.json", 150),
        "FunnelShiftTimingTop" => ("funnel-shift-single-cycle-150.nextpnr.json", 150),
        _ => {
            return Err(format!(
                "expected Core, Top, HardwareTestTop, FullAluTimingTop, or FunnelShiftTimingTop, received {top}"
            )
            .into());
        }
    };
    let mut source = verify_riscv::core_source(project)?;
    for file in [
        "AluTimingTop.veryl",
        "Top.veryl",
        "HardwareMemory.veryl",
        "HardwareTestTop.veryl",
    ] {
        source.push('\n');
        source.push_str(&fs::read_to_string(project.join("src").join(file))?);
    }
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
        let pll = board_pll(project)?;
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

#[cfg(test)]
mod tests {
    use super::validated_pll;
    use std::path::Path;

    #[test]
    fn board_pll_keeps_phase_detector_in_specified_range() {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        validated_pll(&project.join("pll-12-to-124p8.json")).unwrap();
        assert!(validated_pll(&project.join("pll-12-to-126.json")).is_err());
    }
}
