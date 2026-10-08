use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::Value;

use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{
    Bit, Ecp5Cell, JtaggBinding, MappingOptions, PhysicalFeedback, PllBinding, PllOutput,
    RegisterEnableFanoutConstraint, map_to_ecp5_with_options,
};

fn validated_pll(path: &Path) -> Result<PllBinding, Box<dyn Error>> {
    let pll: PllBinding = serde_json::from_str(&fs::read_to_string(path)?)?;
    validate_pll_phase_detector(&pll)?;
    Ok(pll)
}

fn validate_pll_phase_detector(pll: &PllBinding) -> Result<(), Box<dyn Error>> {
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
    Ok(())
}

fn board_pll(project: &Path) -> Result<PllBinding, Box<dyn Error>> {
    let pll = validated_pll(&project.join("pll-12-to-124p8.json"))?;
    validated_board_clock_relation(&pll)?;
    Ok(pll)
}

#[derive(Clone, Copy, Debug)]
struct BoardClockRelation {
    cpu_mhz: f64,
    memory_mhz: f64,
}

fn pll_u32(pll: &PllBinding, name: &str) -> Result<u32, Box<dyn Error>> {
    Ok(pll
        .parameters
        .get(name)
        .ok_or_else(|| format!("board PLL has no {name}"))?
        .parse()?)
}

fn pll_frequency(pll: &PllBinding, name: &str) -> Result<f64, Box<dyn Error>> {
    Ok(pll
        .attributes
        .get(name)
        .ok_or_else(|| format!("board PLL has no {name}"))?
        .parse()?)
}

fn validated_board_clock_relation(pll: &PllBinding) -> Result<BoardClockRelation, Box<dyn Error>> {
    if pll.reference_clock_port != "clk"
        || pll.output_clock_port != "clk_cpu"
        || pll.locked_port != "pll_locked"
        || pll.fabric_output != PllOutput::Clkos
        || pll.feedback_output != PllOutput::Clkintfb
    {
        return Err("board PLL topology does not match clk -> CLKOS/CLKINTFB -> clk_cpu".into());
    }
    if pll.additional_output_clock_ports.len() != 1
        || pll.additional_output_clock_ports.get("clk_mem") != Some(&PllOutput::Clkos2)
    {
        return Err("board memory clock must bind clk_mem to CLKOS2".into());
    }
    for (parameter, expected) in [
        ("DPHASE_SOURCE", "DISABLED"),
        ("OUTDIVIDER_MUXB", "DIVB"),
        ("OUTDIVIDER_MUXC", "DIVC"),
        ("FEEDBK_PATH", "INT_OP"),
    ] {
        if pll.parameters.get(parameter).map(String::as_str) != Some(expected) {
            return Err(format!("board PLL {parameter} must be {expected}").into());
        }
    }
    if pll.parameters.get("CLKOS_ENABLE").map(String::as_str) != Some("ENABLED")
        || pll.parameters.get("CLKOS2_ENABLE").map(String::as_str) != Some("ENABLED")
    {
        return Err("board CPU and memory PLL outputs must both be enabled".into());
    }

    let cpu_divider = pll_u32(pll, "CLKOS_DIV")?;
    let memory_divider = pll_u32(pll, "CLKOS2_DIV")?;
    if pll_u32(pll, "CLKI_DIV")? != 1
        || pll_u32(pll, "CLKFB_DIV")? != 1
        || pll_u32(pll, "CLKOP_DIV")? != 52
        || cpu_divider != 5
        || memory_divider != 10
    {
        return Err(format!(
            "board PLL dividers do not match the checked 12 -> 624 -> 124.8/62.4 MHz relation: CLKOS_DIV={cpu_divider}, CLKOS2_DIV={memory_divider}"
        )
        .into());
    }
    for parameter in [
        "CLKOS_CPHASE",
        "CLKOS_FPHASE",
        "CLKOS2_CPHASE",
        "CLKOS2_FPHASE",
    ] {
        if pll_u32(pll, parameter)? != 0 {
            return Err(format!(
                "board PLL {parameter} must be zero for the checked edge alignment"
            )
            .into());
        }
    }

    let input_mhz = pll_frequency(pll, "FREQUENCY_PIN_CLKI")?;
    let cpu_mhz = pll_frequency(pll, "FREQUENCY_PIN_CLKOS")?;
    let memory_mhz = pll_frequency(pll, "FREQUENCY_PIN_CLKOS2")?;
    let feedback_mhz = input_mhz * f64::from(pll_u32(pll, "CLKOP_DIV")?);
    if (input_mhz - 12.0).abs() > 1.0e-6
        || (cpu_mhz - 124.8).abs() > 1.0e-6
        || (memory_mhz - 62.4).abs() > 1.0e-6
        || (cpu_mhz * f64::from(cpu_divider) - feedback_mhz).abs() > 1.0e-6
        || (memory_mhz * f64::from(memory_divider) - feedback_mhz).abs() > 1.0e-6
    {
        return Err(format!(
            "board PLL frequency attributes do not match the checked 12 -> 624 -> 124.8/62.4 MHz relation: input {input_mhz}, CPU {cpu_mhz}, memory {memory_mhz} MHz"
        )
        .into());
    }
    Ok(BoardClockRelation {
        cpu_mhz,
        memory_mhz,
    })
}

fn csr_register_enable_limit(name: &str) -> Option<usize> {
    let normalized = name.replacen("ff_board.core.", "ff_core.", 1);
    let name = normalized.as_str();
    let group = name.split_once('[').map_or(name, |entry| entry.0);
    match group {
        "ff_core.csr_file.mcause_q"
        | "ff_core.csr_file.mepc_q"
        | "ff_core.csr_file.mscratch_q"
        | "ff_core.csr_file.mstatus_q"
        | "ff_core.csr_file.mtval_q"
        | "ff_core.csr_file.mtvec_q"
        | "ff_core.csr_file.stall_selector_q" => Some(4),
        "ff_core.csr_file.mcycle_counter.low_q"
        | "ff_core.csr_file.mcycle_counter.high_q"
        | "ff_core.csr_file.minstret_counter.low_q"
        | "ff_core.csr_file.minstret_counter.high_q"
        | "ff_core.csr_file.event_counters.low_q"
        | "ff_core.csr_file.event_counters.high_q"
        | "ff_core.csr_file.predictor_conditional_counter.low_q"
        | "ff_core.csr_file.predictor_conditional_counter.high_q"
        | "ff_core.csr_file.predictor_direction_wrong_counter.low_q"
        | "ff_core.csr_file.predictor_direction_wrong_counter.high_q"
        | "ff_core.csr_file.predictor_btb_miss_counter.low_q"
        | "ff_core.csr_file.predictor_btb_miss_counter.high_q"
        | "ff_core.csr_file.predictor_ras_fallback_counter.low_q"
        | "ff_core.csr_file.predictor_ras_fallback_counter.high_q"
        | "ff_core.csr_file.predictor_repair_counter.low_q"
        | "ff_core.csr_file.predictor_repair_counter.high_q" => Some(8),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum BoardClockDomain {
    Cpu,
    Memory,
}

#[derive(Debug)]
struct ClockTiming {
    name: String,
    achieved: f64,
    constraint: f64,
}

fn mapped_cell_domain(cell: &str) -> Option<BoardClockDomain> {
    // Struo retains the source hierarchy after a chain of explicit physical
    // transformation prefixes. Retiming can therefore produce names such as
    // `retime_ff_core.*`; strip only prefixes emitted by these transformations
    // instead of accepting an arbitrary substring that happens to contain a
    // source module name.
    let mut hierarchy = cell;
    loop {
        let stripped = ["retime_", "ff_", "bram_", "lut_"]
            .iter()
            .find_map(|prefix| hierarchy.strip_prefix(prefix));
        let Some(stripped) = stripped else {
            break;
        };
        hierarchy = stripped;
    }
    hierarchy = hierarchy.strip_prefix("board.").unwrap_or(hierarchy);
    if [
        "core.",
        "g_cache.instruction_cache.",
        "g_buffer.instruction_buffer.",
    ]
    .iter()
    .any(|prefix| hierarchy.starts_with(prefix))
    {
        Some(BoardClockDomain::Cpu)
    } else if hierarchy.starts_with("hardware_memory.") {
        Some(BoardClockDomain::Memory)
    } else {
        None
    }
}

fn posedge_clock(edge: &str) -> Option<&str> {
    edge.strip_prefix("posedge ")
}

fn board_clock_timings(report: &Value) -> Result<(ClockTiming, ClockTiming), Box<dyn Error>> {
    let fmax = report["fmax"]
        .as_object()
        .ok_or("nextpnr report has no fmax object")?;
    let fabric_clocks = fmax
        .iter()
        .filter(|(name, _)| name.contains("glbnet"))
        .map(|(name, timing)| {
            let achieved = timing["achieved"]
                .as_f64()
                .ok_or("nextpnr fmax entry has no achieved frequency")?;
            let constraint = timing["constraint"]
                .as_f64()
                .ok_or("nextpnr fmax entry has no constraint frequency")?;
            Ok(ClockTiming {
                name: name.clone(),
                achieved,
                constraint,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let critical_paths = report["critical_paths"]
        .as_array()
        .ok_or("nextpnr report has no critical_paths array")?;
    let global_names = fabric_clocks
        .iter()
        .map(|timing| timing.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut domains = BTreeMap::<&str, BTreeSet<BoardClockDomain>>::new();
    for path in critical_paths {
        let from_clock = path["from"].as_str().and_then(posedge_clock);
        let to_clock = path["to"].as_str().and_then(posedge_clock);
        let segments = path["path"]
            .as_array()
            .ok_or("nextpnr critical path has no segment array")?;

        let source_domain = segments.first().and_then(|segment| {
            // In nextpnr 0.6 a packed CLK endpoint can carry another cell's
            // name. The Q endpoint of the clk-to-q arc identifies the actual
            // launch register, including on newer report formats.
            let endpoint = if segment["type"] == "clk-to-q" {
                "to"
            } else {
                "from"
            };
            segment[endpoint]["cell"]
                .as_str()
                .and_then(mapped_cell_domain)
        });
        let destination_domain = segments
            .last()
            .and_then(|segment| segment["to"]["cell"].as_str().and_then(mapped_cell_domain));
        if let (Some(clock), Some(domain)) = (from_clock, source_domain)
            && global_names.contains(clock)
        {
            domains.entry(clock).or_default().insert(domain);
        }
        if let (Some(clock), Some(domain)) = (to_clock, destination_domain)
            && global_names.contains(clock)
        {
            domains.entry(clock).or_default().insert(domain);
        }
    }

    let select = |wanted: BoardClockDomain| -> Result<ClockTiming, Box<dyn Error>> {
        let mut candidates = fabric_clocks
            .iter()
            .filter(|timing| {
                domains
                    .get(timing.name.as_str())
                    .is_some_and(|identified| identified.len() == 1 && identified.contains(&wanted))
            })
            .map(|timing| ClockTiming {
                name: timing.name.clone(),
                achieved: timing.achieved,
                constraint: timing.constraint,
            });
        let selected = candidates
            .next()
            .ok_or_else(|| format!("nextpnr report does not identify a {wanted:?} global clock"))?;
        if candidates.next().is_some() {
            return Err(
                format!("nextpnr report identifies multiple {wanted:?} global clocks").into(),
            );
        }
        Ok(selected)
    };
    let cpu = select(BoardClockDomain::Cpu)?;
    let memory = select(BoardClockDomain::Memory)?;
    let divide_ratio = cpu.constraint / memory.constraint;
    if !(1.99..=2.01).contains(&divide_ratio) {
        return Err(format!(
            "board CPU/memory clock constraints are not divide-by-two related: {:.2}/{:.2} MHz",
            cpu.constraint, memory.constraint
        )
        .into());
    }
    Ok((cpu, memory))
}

fn validate_board_report_value(
    report: &Value,
    expected: BoardClockRelation,
) -> Result<String, Box<dyn Error>> {
    let (cpu, memory) = board_clock_timings(report)?;
    let cpu_clock = cpu.name.as_str();
    let memory_clock = memory.name.as_str();
    let cpu_achieved = cpu.achieved;
    let cpu_constraint = cpu.constraint;
    let memory_achieved = memory.achieved;
    let memory_constraint = memory.constraint;
    if (cpu_constraint - expected.cpu_mhz).abs() > 0.05
        || (memory_constraint - expected.memory_mhz).abs() > 0.05
    {
        return Err(format!(
            "nextpnr constraints do not match the validated board PLL: CPU {cpu_constraint:.3}/{:.3} MHz, memory {memory_constraint:.3}/{:.3} MHz",
            expected.cpu_mhz, expected.memory_mhz
        )
        .into());
    }
    let interior_failure =
        (cpu_achieved < cpu_constraint || memory_achieved < memory_constraint).then(|| {
            format!(
                "interior timing failed: CPU {cpu_achieved:.2}/{cpu_constraint:.2} MHz, memory {memory_achieved:.2}/{memory_constraint:.2} MHz"
            )
        });

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
    let boundary_failure = (cpu_to_memory > cpu_period || memory_to_cpu > cpu_period).then(|| {
        format!(
            "related-clock boundary failed: CPU->memory {cpu_to_memory:.3}/{cpu_period:.3} ns, memory->CPU {memory_to_cpu:.3}/{cpu_period:.3} ns"
        )
    });
    match (interior_failure, boundary_failure) {
        (Some(interior), Some(boundary)) => {
            return Err(format!("multiple timing failures: {interior}; {boundary}").into());
        }
        (Some(interior), None) => return Err(interior.into()),
        (None, Some(boundary)) => return Err(boundary.into()),
        (None, None) => {}
    }
    Ok(format!(
        "Board timing valid: CPU {cpu_achieved:.2}/{cpu_constraint:.2} MHz, memory {memory_achieved:.2}/{memory_constraint:.2} MHz; CPU->memory {cpu_to_memory:.3}/{cpu_period:.3} ns, memory->CPU {memory_to_cpu:.3}/{cpu_period:.3} ns"
    ))
}

fn validate_board_report(project: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    let relation = validated_board_clock_relation(&board_pll(project)?)?;
    let report: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    println!("{}", validate_board_report_value(&report, relation)?);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut arguments = env::args().skip(1).collect::<Vec<_>>();
    let nextpnr_json = arguments.iter().any(|arg| arg == "--nextpnr-json");
    arguments.retain(|arg| arg != "--nextpnr-json");
    let mut arguments = arguments.into_iter();
    let top = arguments.next().unwrap_or_else(|| "Core".to_owned());
    if top == "--validate-board-report" {
        let report = arguments
            .next()
            .ok_or("--validate-board-report requires a nextpnr report path")?;
        if arguments.next().is_some() {
            return Err("--validate-board-report accepts exactly one path".into());
        }
        return validate_board_report(project, Path::new(&report));
    }
    let feedback_arguments = arguments.collect::<Vec<_>>();
    if feedback_arguments.len() % 2 != 0 {
        return Err("each physical feedback step requires REPORT and PLACED artifact paths".into());
    }
    let (output_name, timing_goal_mhz) = match top.as_str() {
        "Core" => ("riscv-125.nextpnr.json", 125),
        "Top" => ("riscv-top-125.nextpnr.json", 125),
        "HardwareTestTop" => ("riscv-hardware-test-125.nextpnr.json", 125),
        "HardwareStaticTestTop" => ("riscv-hardware-static-test-125.nextpnr.json", 125),
        "HardwareCached2KTestTop" => ("riscv-hardware-cached-2k-test-125.nextpnr.json", 125),
        "HardwareCached4KTestTop" => ("riscv-hardware-cached-4k-test-125.nextpnr.json", 125),
        "HardwareCached8KTestTop" => ("riscv-hardware-cached-8k-test-125.nextpnr.json", 125),
        // Keep the standalone ALU diagnostics at their intentionally harder
        // 150 MHz target even though the integrated CPU now targets 125 MHz.
        "FullAluTimingTop" => ("alu-full-single-cycle-150.nextpnr.json", 150),
        "FunnelShiftTimingTop" => ("funnel-shift-single-cycle-150.nextpnr.json", 150),
        _ => {
            return Err(format!(
                "expected Core, Top, HardwareTestTop, HardwareStaticTestTop, HardwareCached2KTestTop, HardwareCached4KTestTop, HardwareCached8KTestTop, FullAluTimingTop, or FunnelShiftTimingTop, received {top}"
            )
            .into());
        }
    };
    let mut source = verify_riscv::core_source(project)?;
    for file in [
        "AluTimingTop.veryl",
        "Top.veryl",
        "InstructionBuffer.veryl",
        "InstructionCache.veryl",
        "HardwareCachedTestTop.veryl",
        "HardwareMemory.veryl",
        "JtagDebugTransport.veryl",
        "HardwareTestTop.veryl",
        "HardwareStaticTestTop.veryl",
    ] {
        source.push('\n');
        source.push_str(&fs::read_to_string(project.join("src").join(file))?);
    }
    eprintln!("nextpnr-riscv: analyze/lower {top}");
    let design = analyze_and_lower(&source, "rica_riscv", &top)?;
    eprintln!("nextpnr-riscv: synthesize {top}");
    let synthesized = synthesize(&design)?;
    for report in &synthesized.reports {
        println!("Struo {}: {}", report.pass, report.message);
    }
    eprintln!("nextpnr-riscv: map {top}");
    let mut mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz,
            ..MappingOptions::default()
        },
    )?;
    if !mapped.retiming().equivalence_signed_off {
        return Err("Struo mapping equivalence was not signed off".into());
    }
    let is_board = matches!(
        top.as_str(),
        "HardwareTestTop"
            | "HardwareStaticTestTop"
            | "HardwareCached2KTestTop"
            | "HardwareCached4KTestTop"
            | "HardwareCached8KTestTop"
    );
    if is_board {
        mapped.bind_jtagg(&JtaggBinding::with_prefix("jtag"))?;
        let pll = board_pll(project)?;
        mapped.bind_pll(&pll)?;
    }
    for [report, placed] in feedback_arguments.as_chunks::<2>().0 {
        let feedback = PhysicalFeedback::from_nextpnr_files(Path::new(report), Path::new(placed))?;
        mapped = mapped.apply_physical_feedback(&feedback);
    }
    if is_board {
        // Mapping may legitimately absorb a self-hold into D on individual
        // bits. Constrain only FFs that actually retain a routed CE pin;
        // selecting a broad source-level vector would reject mixed-CE groups.
        let mut groups = BTreeMap::<String, (usize, usize)>::new();
        let constraints = mapped
            .cells()
            .iter()
            .filter_map(|cell| {
                let Ecp5Cell::FlipFlop { name, enable, .. } = cell else {
                    return None;
                };
                let group = name.split_once('[').map_or(name.as_str(), |entry| entry.0);
                let limit = csr_register_enable_limit(name)?;
                let counts = groups.entry(group.to_owned()).or_default();
                counts.1 += 1;
                if matches!(enable, Some(enable) if matches!(enable.signal, Bit::Wire(_))) {
                    counts.0 += 1;
                    Some(RegisterEnableFanoutConstraint::new(name, limit))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        println!("CSR mapped CE groups (eligible/total): {groups:?}");
        let report = mapped.apply_register_enable_fanout_constraints(&constraints)?;
        println!(
            "CSR clock-enable fanout: {} FFs, {} branches",
            report.rewired_registers, report.inserted_branches
        );
    }

    let output_name = if nextpnr_json {
        output_name.to_owned()
    } else {
        output_name.replace(".nextpnr.json", ".stnet")
    };
    let output = project.join("build").join(output_name);
    fs::create_dir_all(output.parent().unwrap())?;
    if nextpnr_json {
        fs::write(&output, mapped.to_nextpnr_json()?)?;
    } else {
        mapped.write_artifact(&output)?;
    }
    println!(
        "Struo mapped artifact: {} cells, retimed={}, data={} -> {} ps, overall={} -> {} ps, LUT depth={} -> {}, {}",
        mapped.cells().len(),
        mapped.retiming().applied,
        mapped.retiming().original_period_ps,
        mapped.retiming().selected_period_ps,
        mapped.retiming().original_overall_period_ps,
        mapped.retiming().selected_overall_period_ps,
        mapped.retiming().original_lut_depth,
        mapped.retiming().selected_lut_depth,
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BoardClockDomain, BoardClockRelation, board_pll, csr_register_enable_limit,
        mapped_cell_domain, validate_board_report_value, validate_pll_phase_detector,
        validated_board_clock_relation,
    };
    use serde_json::{Value, json};
    use std::path::Path;

    fn path(from_clock: &str, to_clock: &str, from_cell: &str, to_cell: &str, delay: f64) -> Value {
        json!({
            "from": format!("posedge {from_clock}"),
            "to": format!("posedge {to_clock}"),
            "path": [{
                "delay": delay,
                "from": { "cell": from_cell },
                "to": { "cell": to_cell }
            }]
        })
    }

    fn board_report(include_jtag_clocks: bool, memory_to_cpu_delay: f64) -> Value {
        let cpu = "$glbnet$cpu";
        let memory = "$glbnet$memory";
        let mut fmax = json!({
            cpu: { "achieved": 130.0, "constraint": 125.0 },
            memory: { "achieved": 80.0, "constraint": 62.5 }
        });
        let mut paths = vec![
            path(cpu, cpu, "ff_core.source_q", "ff_core.sink_q", 7.0),
            path(
                memory,
                memory,
                "ff_hardware_memory.source_q",
                "bram_hardware_memory.memory_0",
                9.0,
            ),
            path(
                cpu,
                memory,
                "ff_core.request_q",
                "ff_hardware_memory.request_q",
                7.5,
            ),
            path(
                memory,
                cpu,
                "ff_hardware_memory.response_q",
                "ff_core.response_q",
                memory_to_cpu_delay,
            ),
        ];
        if include_jtag_clocks {
            fmax["$glbnet$jtag_shift"] = json!({ "achieved": 600.0, "constraint": 125.0 });
            fmax["$glbnet$jtag_update"] = json!({ "achieved": 700.0, "constraint": 125.0 });
            paths.push(path(
                "$glbnet$jtag_shift",
                "$glbnet$jtag_shift",
                "ff_jtag_debug_transport.shift_q",
                "ff_jtag_debug_transport.shift_q",
                1.0,
            ));
            paths.push(path(
                "$glbnet$jtag_update",
                "$glbnet$jtag_update",
                "ff_jtag_debug_transport.update_q",
                "ff_jtag_debug_transport.update_q",
                1.0,
            ));
        }
        json!({ "fmax": fmax, "critical_paths": paths })
    }

    fn synthetic_relation() -> BoardClockRelation {
        BoardClockRelation {
            cpu_mhz: 125.0,
            memory_mhz: 62.5,
        }
    }

    #[test]
    fn board_pll_keeps_phase_detector_in_specified_range() {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let relation = validated_board_clock_relation(&board_pll(project).unwrap()).unwrap();
        assert_eq!(relation.cpu_mhz, 124.8);
        assert_eq!(relation.memory_mhz, 62.4);
        let mut pll = board_pll(project).unwrap();
        pll.parameters.insert("CLKI_DIV".to_owned(), "2".to_owned());
        let error = validate_pll_phase_detector(&pll).unwrap_err();
        assert!(error.to_string().contains("phase detector is 6 MHz"));
    }

    #[test]
    fn board_report_guard_rejects_unchecked_pll_phase() {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut pll = board_pll(project).unwrap();
        pll.parameters
            .insert("CLKOS2_CPHASE".to_owned(), "1".to_owned());
        assert!(validated_board_clock_relation(&pll).is_err());
    }

    #[test]
    fn nextpnr_uses_the_same_csr_ce_limits_as_the_texo_flow() {
        assert_eq!(
            csr_register_enable_limit("ff_board.core.csr_file.mcause_q[63]"),
            Some(4)
        );
        assert_eq!(
            csr_register_enable_limit("ff_board.core.csr_file.event_counters.low_q[3][31]"),
            Some(8)
        );
        assert_eq!(
            csr_register_enable_limit("ff_core.csr_file.mcause_q[63]"),
            Some(4)
        );
        assert_eq!(
            csr_register_enable_limit("ff_core.csr_file.event_counters.low_q[3][31]"),
            Some(8)
        );
        assert_eq!(
            csr_register_enable_limit("ff_core.branch_predictor.speculative_history_q[0]"),
            None
        );
    }

    #[test]
    fn two_clock_board_report_keeps_existing_validation_behavior() {
        let message =
            validate_board_report_value(&board_report(false, 7.75), synthetic_relation()).unwrap();
        assert_eq!(
            message,
            "Board timing valid: CPU 130.00/125.00 MHz, memory 80.00/62.50 MHz; CPU->memory 7.500/8.000 ns, memory->CPU 7.750/8.000 ns"
        );
    }

    #[test]
    fn packed_clock_pin_alias_does_not_misidentify_the_launch_domain() {
        let mut report = board_report(false, 7.0);
        for path in report["critical_paths"].as_array_mut().unwrap() {
            let segments = path["path"].as_array_mut().unwrap();
            let launch = segments[0]["from"].clone();
            segments.insert(
                0,
                json!({
                    "type": "clk-to-q", "delay": 0.3,
                    "from": { "cell": "ff_core.unrelated_packed_clock_pin", "port": "CLK" },
                    "to": launch,
                }),
            );
        }
        assert!(validate_board_report_value(&report, synthetic_relation()).is_ok());
        report["fmax"]["$glbnet$cpu"]["achieved"] = json!(90.0);
        let error = validate_board_report_value(&report, synthetic_relation())
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("interior timing failed:"));
    }

    #[test]
    fn four_clock_board_report_ignores_jtag_clocks() {
        let error = validate_board_report_value(&board_report(true, 8.25), synthetic_relation())
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "related-clock boundary failed: CPU->memory 7.500/8.000 ns, memory->CPU 8.250/8.000 ns"
        );
    }

    #[test]
    fn board_report_guard_reports_interior_and_boundary_failures_together() {
        let mut report = board_report(true, 8.25);
        report["fmax"]["$glbnet$cpu"]["achieved"] = json!(90.0);
        let error = validate_board_report_value(&report, synthetic_relation())
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "multiple timing failures: interior timing failed: CPU 90.00/125.00 MHz, memory 80.00/62.50 MHz; related-clock boundary failed: CPU->memory 7.500/8.000 ns, memory->CPU 8.250/8.000 ns"
        );
    }

    #[test]
    fn board_report_guard_rejects_a_different_two_to_one_frequency_pair() {
        let mut report = board_report(false, 4.0);
        for timing in report["fmax"].as_object_mut().unwrap().values_mut() {
            let constraint = timing["constraint"].as_f64().unwrap();
            if constraint == 125.0 {
                timing["constraint"] = json!(100.0);
            } else {
                timing["constraint"] = json!(50.0);
            }
        }
        let error = validate_board_report_value(&report, synthetic_relation())
            .unwrap_err()
            .to_string();
        assert!(error.contains("do not match the validated board PLL"));
    }

    #[test]
    fn retimed_endpoints_still_identify_the_cpu_clock() {
        let mut report = board_report(true, 7.75);
        for path in report["critical_paths"].as_array_mut().unwrap() {
            for segment in path["path"].as_array_mut().unwrap() {
                for endpoint in ["from", "to"] {
                    let Some(cell) = segment[endpoint]["cell"].as_str() else {
                        continue;
                    };
                    if cell.starts_with("ff_core.") {
                        segment[endpoint]["cell"] = Value::String(format!("retime_{cell}"));
                    }
                }
            }
        }
        assert!(validate_board_report_value(&report, synthetic_relation()).is_ok());
        assert_eq!(
            mapped_cell_domain("retime_lut_core.branch_redirect"),
            Some(BoardClockDomain::Cpu)
        );
        assert_eq!(
            mapped_cell_domain("retime_bram_hardware_memory.bank_0"),
            Some(BoardClockDomain::Memory)
        );
        assert_eq!(mapped_cell_domain("retime_ff_score.value_q"), None);
        assert_eq!(
            mapped_cell_domain("retime_ff_board.core.cm_pc_q[0]"),
            Some(BoardClockDomain::Cpu)
        );
        assert_eq!(
            mapped_cell_domain("bram_board.hardware_memory.bank_0"),
            Some(BoardClockDomain::Memory)
        );
    }

    #[test]
    fn cache_only_critical_endpoints_identify_the_cpu_clock() {
        let mut report = board_report(false, 7.75);
        for path in report["critical_paths"].as_array_mut().unwrap() {
            for segment in path["path"].as_array_mut().unwrap() {
                for endpoint in ["from", "to"] {
                    if segment[endpoint]["cell"]
                        .as_str()
                        .is_some_and(|cell| cell.starts_with("ff_core."))
                    {
                        segment[endpoint]["cell"] = Value::String(
                            "retime_bram_board.g_cache.instruction_cache.g_bank_0.memory"
                                .to_owned(),
                        );
                    }
                }
            }
        }
        assert!(validate_board_report_value(&report, synthetic_relation()).is_ok());
        assert_eq!(
            mapped_cell_domain("ff_board.g_buffer.instruction_buffer.response_line_q[0]"),
            Some(BoardClockDomain::Cpu)
        );
    }
}
