//! Export physical primitive connectivity for initial-placement hints only.
use std::{error::Error, fs::File, path::Path};
use struo_target_ecp5::{JtaggBinding, MappingOptions, PllBinding, RegisterEnableFanoutConstraint};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: placement-graph PNR-INPUT.json OUTPUT.txcp".into());
    }
    let record: serde_json::Value = serde_json::from_reader(File::open(&args[1])?)?;
    let command: Vec<&str> = record["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let option = |name| command[command.iter().position(|s| *s == name).unwrap() + 1];
    let loaded = texo_cli::load_veryl_project(Path::new(command[2]), Some(option("--top")))?;
    let synthesized = struo_synth::synthesize(&loaded.design)?;
    let mut mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: option("--synthesis-goal-mhz").parse()?,
            retiming: !command.contains(&"--no-retiming"),
            ..Default::default()
        },
    )?;
    mapped.bind_jtagg(&JtaggBinding::with_prefix(option("--jtagg-prefix")))?;
    let pll: PllBinding = serde_json::from_reader(File::open(option("--pll-binding"))?)?;
    mapped.bind_pll(&pll)?;
    let mut constraints = Vec::new();
    for pair in command.windows(2) {
        if pair[0] == "--register-enable-fanout" {
            let (name, limit) = pair[1].rsplit_once('=').unwrap();
            constraints.push(RegisterEnableFanoutConstraint::new(name, limit.parse()?));
        }
    }
    mapped.apply_register_enable_fanout_constraints(&constraints)?;
    println!("Mapping retiming: {:?}", mapped.retiming());
    assert!(mapped.retiming().equivalence_signed_off);
    let imported = texo_struo::import_ecp5(&mapped)?;
    let design = imported.design();
    let mut cells = Vec::new();
    for (&id, metadata) in imported.metadata() {
        let cell = &design.cells()[id.0];
        let mut inputs = Vec::new();
        for &pin_id in cell.pins() {
            let pin = &design.pins()[pin_id.0];
            if let Some(net) = pin.net() {
                let net = &design.nets()[net.0];
                if net.driver != pin_id {
                    let driver = &design.pins()[net.driver.0];
                    inputs.push(serde_json::json!({
                        "pin": pin.name, "driver": driver.cell.0, "driver_pin": driver.name,
                        "net": net.name, "net_id": pin.net().unwrap().0,
                    }));
                }
            }
        }
        cells.push(serde_json::json!({
            "id": id.0, "name": cell.name, "kind": format!("{:?}", cell.kind),
            "metadata": format!("{metadata:?}"), "absorbed": imported.absorbed_inputs().get(&id),
            "inputs": inputs,
        }));
    }
    assert_eq!(cells.len(), design.cells().len());
    let output = Path::new(&args[2]);
    if output.extension().and_then(|s| s.to_str()) != Some("txcp") {
        return Err("graph output must be atomic CBOR+Zstd .txcp".into());
    }
    texo_cli::write_checkpoint_binary(output, &cells)?;

    println!(
        "Exported {} cells and {} nets; no timing evidence transferred",
        cells.len(),
        design.nets().len()
    );
    Ok(())
}
