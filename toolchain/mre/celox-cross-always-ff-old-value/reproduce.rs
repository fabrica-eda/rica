//! Cross-`always_ff` old-value semantics reproducer.
//!
//! The source is read from disk so that this MRE exercises Celox and Struo's
//! public Veryl APIs without embedding generated SystemVerilog.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use celox::{OptLevel, SimBackend, Simulator, SimulatorBuilder};
use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{MappingOptions, map_to_ecp5_with_options};

const TOP: &str = "CrossAlwaysFfOldValue";
const OLD_VALUE: u32 = 4;
const NEW_VALUE: u32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Observation {
    current: u32,
    captured: u32,
    current_metadata: u32,
    captured_metadata: u32,
}

fn value<B: SimBackend>(simulator: &mut Simulator<B>, name: &str) -> u32 {
    simulator
        .get(simulator.signal(name))
        .to_u32_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn run_current<B: SimBackend>(mut simulator: Simulator<B>) -> Result<Observation, Box<dyn Error>> {
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let capture = simulator.signal("raw_response_capture");
    let replace = simulator.signal("request_fire");
    let replacement = simulator.signal("request_fallthrough");

    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(capture, 0_u8);
        io.set(replace, 0_u8);
        io.set(replacement, NEW_VALUE);
    })?;
    simulator.tick(clock)?;

    let reset_current = value(&mut simulator, "current_value");
    let reset_metadata = value(&mut simulator, "current_metadata");
    if reset_current != OLD_VALUE || reset_metadata != 0 {
        return Err(
            format!("reset produced current={reset_current}, metadata={reset_metadata}").into(),
        );
    }

    simulator.modify(|io| {
        io.set(reset, 1_u8);
        io.set(capture, 1_u8);
        io.set(replace, 1_u8);
    })?;
    simulator.tick(clock)?;

    Ok(Observation {
        current: value(&mut simulator, "current_value"),
        captured: value(&mut simulator, "captured_value"),
        current_metadata: value(&mut simulator, "current_metadata"),
        captured_metadata: value(&mut simulator, "captured_metadata"),
    })
}

fn value_03(simulator: &mut celox03::Simulator, name: &str) -> u32 {
    simulator
        .get(simulator.signal(name))
        .to_u32_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn run_mapped(mut simulator: celox03::Simulator) -> Result<Observation, Box<dyn Error>> {
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let capture = simulator.signal("raw_response_capture");
    let replace = simulator.signal("request_fire");
    let replacement = simulator.signal("request_fallthrough");

    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(capture, 0_u8);
        io.set(replace, 0_u8);
        io.set(replacement, NEW_VALUE);
    })?;
    simulator.tick(clock)?;

    let reset_current = value_03(&mut simulator, "current_value");
    let reset_metadata = value_03(&mut simulator, "current_metadata");
    if reset_current != OLD_VALUE || reset_metadata != 0 {
        return Err(format!(
            "mapped reset produced current={reset_current}, metadata={reset_metadata}"
        )
        .into());
    }

    simulator.modify(|io| {
        io.set(reset, 1_u8);
        io.set(capture, 1_u8);
        io.set(replace, 1_u8);
    })?;
    simulator.tick(clock)?;

    Ok(Observation {
        current: value_03(&mut simulator, "current_value"),
        captured: value_03(&mut simulator, "captured_value"),
        current_metadata: value_03(&mut simulator, "current_metadata"),
        captured_metadata: value_03(&mut simulator, "captured_metadata"),
    })
}

fn source_path() -> Result<PathBuf, Box<dyn Error>> {
    match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../toolchain/mre/celox-cross-always-ff-old-value/src/CrossAlwaysFfOldValue.veryl",
        )),
        [path] => Ok(PathBuf::from(path)),
        _ => Err("usage: celox-cross-always-ff-old-value [SOURCE.veryl]".into()),
    }
}

fn report(name: &str, observation: Observation) -> Result<bool, Box<dyn Error>> {
    let expected = Observation {
        current: NEW_VALUE,
        captured: OLD_VALUE,
        current_metadata: 1,
        captured_metadata: 0,
    };
    let status = if observation == expected {
        "PASS"
    } else {
        "MISMATCH"
    };
    println!(
        "{name}: {status}: current={}, captured={}, current_metadata={}, captured_metadata={}",
        observation.current,
        observation.captured,
        observation.current_metadata,
        observation.captured_metadata,
    );
    if observation.current != NEW_VALUE {
        return Err(format!("{name} did not install the replacement: {observation:?}").into());
    }
    Ok(observation == expected)
}

fn main() -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(source_path()?)?;

    let native = SimulatorBuilder::new(&source, TOP)
        .opt_level(OptLevel::O0)
        .build_native()?;
    let native_ok = report("Celox source/native/O0", run_current(native)?)?;

    let wasm = SimulatorBuilder::new(&source, TOP)
        .opt_level(OptLevel::O0)
        .build_wasm()?;
    let wasm_ok = report("Celox source/WASM/O0", run_current(wasm)?)?;

    let design = analyze_and_lower(&source, "cross_always_ff_old_value", TOP)?;
    let synthesized = synthesize(&design)?;
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )?;
    let mapped_native = ecp5_simulator(&mapped)?.build_native()?;
    let mapped_ok = report(
        "Struo ECP5 mapped/Celox 0.3.1 native",
        run_mapped(mapped_native)?,
    )?;

    if !(native_ok && wasm_ok && mapped_ok) {
        return Err("one or more backends violated cross-always_ff old-value semantics".into());
    }

    Ok(())
}
