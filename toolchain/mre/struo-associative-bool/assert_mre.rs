//! Maps the associative-Boolean MRE and rejects left-associated quality regressions.

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;

use struo::ir::{NetId, Node, NodeKind};
use struo::{MappingOptions, analyze_and_lower, map_to_ecp5_with_options, synthesize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Metrics {
    synth_depth: usize,
    mapped_depth: usize,
    period_ps: u32,
    cells: usize,
}

fn logic_depth(
    net: NetId,
    drivers: &HashMap<NetId, &Node>,
    memo: &mut HashMap<NetId, usize>,
) -> usize {
    if let Some(depth) = memo.get(&net) {
        return *depth;
    }
    let node = drivers[&net];
    let is_logic = matches!(
        node.kind(),
        NodeKind::And | NodeKind::Or | NodeKind::Xor | NodeKind::Not | NodeKind::Mux
    );
    let depth = usize::from(is_logic)
        + node
            .inputs()
            .iter()
            .map(|input| logic_depth(*input, drivers, memo))
            .max()
            .unwrap_or_default();
    memo.insert(net, depth);
    depth
}

fn measure(source: &str, top: &str) -> Result<Metrics, Box<dyn Error>> {
    let design = analyze_and_lower(source, "associative_bool_mre", top)?;
    let synthesized = synthesize(&design)?;
    let drivers = synthesized
        .netlist
        .nodes()
        .iter()
        .map(|node| (node.output(), node))
        .collect::<HashMap<_, _>>();
    let result = synthesized
        .netlist
        .registers()
        .iter()
        .find(|register| register.name() == "result_q")
        .ok_or("missing result_q")?;
    let synth_depth = logic_depth(result.data(), &drivers, &mut HashMap::new());
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )?;
    Ok(Metrics {
        synth_depth,
        mapped_depth: mapped.retiming().original_lut_depth,
        period_ps: mapped.retiming().original_overall_period_ps,
        cells: mapped.cells().len(),
    })
}

fn require_equivalent_qor(
    operation: &str,
    linear: Metrics,
    balanced: Metrics,
) -> Result<(), Box<dyn Error>> {
    println!("{operation}: linear={linear:?}, explicit_balanced={balanced:?}");
    if linear != balanced {
        return Err(format!(
            "{operation} association changed QoR: linear={linear:?}, balanced={balanced:?}"
        )
        .into());
    }
    if linear.synth_depth > 4 || linear.mapped_depth > 2 || linear.period_ps > 2_000 {
        return Err(format!("{operation} did not retain the balanced QoR bound: {linear:?}").into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let source_path = env::args().nth(1).ok_or("missing Veryl source path")?;
    let source = fs::read_to_string(source_path)?;
    require_equivalent_qor(
        "AND",
        measure(&source, "LinearAnd16")?,
        measure(&source, "BalancedAnd16")?,
    )?;
    require_equivalent_qor(
        "OR",
        measure(&source, "LinearOr16")?,
        measure(&source, "BalancedOr16")?,
    )?;
    Ok(())
}
