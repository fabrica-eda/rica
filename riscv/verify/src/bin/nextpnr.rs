use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::Value;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{
    JtaggBinding, MappingOptions, PhysicalFeedback, PllBinding, RegisterEnableFanoutConstraint,
    map_to_ecp5_with_options,
};

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

fn validate_board_report(path: &Path) -> Result<(), Box<dyn Error>> {
    let report: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let fmax = report["fmax"]
        .as_object()
        .ok_or("nextpnr report has no fmax object")?;
    let mut fabric_clocks = fmax
        .iter()
        .filter(|(name, _)| name.contains("glbnet"))
        .map(|(name, timing)| {
            let achieved = timing["achieved"]
                .as_f64()
                .ok_or("nextpnr fmax entry has no achieved frequency")?;
            let constraint = timing["constraint"]
                .as_f64()
                .ok_or("nextpnr fmax entry has no constraint frequency")?;
            Ok((name.as_str(), achieved, constraint))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if fabric_clocks.len() != 2 {
        return Err(format!(
            "expected exactly two global fabric clocks in nextpnr report, found {}",
            fabric_clocks.len()
        )
        .into());
    }
    fabric_clocks.sort_by(|lhs, rhs| rhs.2.total_cmp(&lhs.2));
    let (cpu_clock, cpu_achieved, cpu_constraint) = fabric_clocks[0];
    let (memory_clock, memory_achieved, memory_constraint) = fabric_clocks[1];
    if cpu_achieved < cpu_constraint || memory_achieved < memory_constraint {
        return Err(format!(
            "interior timing failed: CPU {cpu_achieved:.2}/{cpu_constraint:.2} MHz, memory {memory_achieved:.2}/{memory_constraint:.2} MHz"
        )
        .into());
    }

    let critical_paths = report["critical_paths"]
        .as_array()
        .ok_or("nextpnr report has no critical_paths array")?;
    let path_delay = |from_clock: &str, to_clock: &str| -> Result<f64, Box<dyn Error>> {
        let from = format!("posedge {from_clock}");
        let to = format!("posedge {to_clock}");
        let path = critical_paths
            .iter()
            .find(|path| path["from"].as_str() == Some(&from) && path["to"].as_str() == Some(&to))
            .ok_or_else(|| format!("nextpnr report has no {from} -> {to} critical path"))?;
        path["path"]
            .as_array()
            .ok_or("nextpnr critical path has no segment array")?
            .iter()
            .try_fold(0.0, |total, segment| {
                Ok(total
                    + segment["delay"]
                        .as_f64()
                        .ok_or("nextpnr path segment has no delay")?)
            })
    };
    let cpu_to_memory = path_delay(cpu_clock, memory_clock)?;
    let memory_to_cpu = path_delay(memory_clock, cpu_clock)?;
    let cpu_period = 1000.0 / cpu_constraint;
    // CLKOS2 is an integer divide-by-two of phase-aligned CLKOS. A request
    // launched on the intervening CPU edge has only one CPU period before the
    // next memory edge; allowing a full memory period would miss that case.
    if cpu_to_memory > cpu_period || memory_to_cpu > cpu_period {
        return Err(format!(
            "related-clock boundary failed: CPU->memory {cpu_to_memory:.3}/{cpu_period:.3} ns, memory->CPU {memory_to_cpu:.3}/{cpu_period:.3} ns"
        )
        .into());
    }
    println!(
        "Board timing valid: CPU {cpu_achieved:.2}/{cpu_constraint:.2} MHz, memory {memory_achieved:.2}/{memory_constraint:.2} MHz; CPU->memory {cpu_to_memory:.3}/{cpu_period:.3} ns, memory->CPU {memory_to_cpu:.3}/{cpu_period:.3} ns"
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut arguments = env::args().skip(1).collect::<Vec<_>>().into_iter();
    let top = arguments.next().unwrap_or_else(|| "Core".to_owned());
    if top == "--validate-board-report" {
        let report = arguments
            .next()
            .ok_or("--validate-board-report requires a nextpnr report path")?;
        if arguments.next().is_some() {
            return Err("--validate-board-report accepts exactly one path".into());
        }
        return validate_board_report(Path::new(&report));
    }
    let feedback_arguments = arguments.collect::<Vec<_>>();
    if feedback_arguments.len() % 2 != 0 {
        return Err("each physical feedback step requires REPORT.json and PLACED.json".into());
    }
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
        "InstructionBuffer.veryl",
        "HardwareMemory.veryl",
        "JtagDebugTransport.veryl",
        "HardwareTestTop.veryl",
    ] {
        source.push('\n');
        source.push_str(&fs::read_to_string(project.join("src").join(file))?);
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
    for [report, placed] in feedback_arguments.as_chunks::<2>().0 {
        let feedback = PhysicalFeedback::from_nextpnr_json(
            &fs::read_to_string(report)?,
            &fs::read_to_string(placed)?,
        )?;
        mapped = mapped.apply_physical_feedback(&feedback);
    }
    if top == "HardwareTestTop" {
        let report = mapped.apply_register_enable_fanout_constraints(&[
            RegisterEnableFanoutConstraint::new("ff_core.csr_file.*", 4),
        ])?;
        println!(
            "CSR clock-enable fanout: {} FFs, {} branches",
            report.rewired_registers, report.inserted_branches
        );
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
