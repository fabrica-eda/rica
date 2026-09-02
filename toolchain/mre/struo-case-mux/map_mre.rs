//! Maps the case-mux MRE without invoking Struo's post-map simulator.

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;

use struo::ir::{NetId, Node, NodeKind};
use struo::rtl::{ExprId, ExprKind, Module};
use struo::target::ecp5::Ecp5Cell;
use struo::{
    MappingOptions, SynthesisOptions, analyze_and_lower, map_to_ecp5_with_options,
    synthesize_with_options,
};

#[derive(Clone, Copy, Default)]
struct ExprStats {
    nodes: usize,
    muxes: usize,
    mux_depth: usize,
}

fn net_stats<'a>(
    net: NetId,
    drivers: &HashMap<NetId, &'a Node>,
    memo: &mut HashMap<NetId, ExprStats>,
) -> ExprStats {
    if let Some(stats) = memo.get(&net) {
        return *stats;
    }
    let node = drivers[&net];
    let child_stats: Vec<_> = node
        .inputs()
        .iter()
        .map(|input| net_stats(*input, drivers, memo))
        .collect();
    let is_mux = matches!(node.kind(), NodeKind::Mux);
    let is_logic = matches!(
        node.kind(),
        NodeKind::And | NodeKind::Or | NodeKind::Xor | NodeKind::Not | NodeKind::Mux
    );
    let stats = ExprStats {
        nodes: usize::from(is_logic)
            + child_stats
                .iter()
                .map(|stats| stats.nodes)
                .max()
                .unwrap_or_default(),
        muxes: usize::from(is_mux)
            + child_stats
                .iter()
                .map(|stats| stats.muxes)
                .max()
                .unwrap_or_default(),
        mux_depth: usize::from(is_mux)
            + child_stats
                .iter()
                .map(|stats| stats.mux_depth)
                .max()
                .unwrap_or_default(),
    };
    memo.insert(net, stats);
    stats
}

fn expr_stats(module: &Module, id: ExprId, memo: &mut HashMap<ExprId, ExprStats>) -> ExprStats {
    if let Some(stats) = memo.get(&id) {
        return *stats;
    }
    let expression = &module.expressions()[id.index() as usize];
    let children: Vec<ExprId> = match expression.kind() {
        ExprKind::Signal(_) | ExprKind::Constant(_) => Vec::new(),
        ExprKind::Unary { input, .. } => vec![*input],
        ExprKind::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
        ExprKind::Mux {
            condition,
            then_expr,
            else_expr,
        } => vec![*condition, *then_expr, *else_expr],
        ExprKind::Concat(parts) => parts.clone(),
        ExprKind::Slice { input, .. } => vec![*input],
    };
    let child_stats: Vec<_> = children
        .into_iter()
        .map(|child| expr_stats(module, child, memo))
        .collect();
    let is_mux = matches!(expression.kind(), ExprKind::Mux { .. });
    let stats = ExprStats {
        nodes: 1 + child_stats.iter().map(|stats| stats.nodes).sum::<usize>(),
        muxes: usize::from(is_mux) + child_stats.iter().map(|stats| stats.muxes).sum::<usize>(),
        mux_depth: usize::from(is_mux)
            + child_stats
                .iter()
                .map(|stats| stats.mux_depth)
                .max()
                .unwrap_or_default(),
    };
    memo.insert(id, stats);
    stats
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let source_path = args.next().ok_or("missing Veryl source path")?;
    let top = args.next().ok_or("missing top module")?;
    let output = args.next().ok_or("missing output JSON path")?;
    let timing_goal_mhz = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(125);
    let source = fs::read_to_string(source_path)?;
    let design = analyze_and_lower(&source, "mux_mre", &top)?;
    let module = design.top_module().ok_or("missing top module")?;
    let signal_names: HashMap<_, _> = module
        .signals()
        .iter()
        .map(|signal| (signal.id(), signal.name()))
        .collect();
    let mut memo = HashMap::new();
    for register in module.registers() {
        let target = signal_names[&register.target];
        if target == "result_q" {
            let stats = expr_stats(module, register.next, &mut memo);
            println!(
                "LOWERED target={target} nodes={} muxes={} mux_depth={}",
                stats.nodes, stats.muxes, stats.mux_depth
            );
        }
    }
    for assignment in module.assignments() {
        let target = signal_names[&assignment.target.signal];
        if target == "selected" {
            let stats = expr_stats(module, assignment.value, &mut memo);
            println!(
                "LOWERED_ASSIGN target={target}[{}+:{}] nodes={} muxes={} mux_depth={}",
                assignment.target.lsb,
                assignment.target.width.get(),
                stats.nodes,
                stats.muxes,
                stats.mux_depth
            );
        }
    }
    let synthesized = synthesize_with_options(
        &design,
        SynthesisOptions {
            relax_qualified_register_enables: true,
            ..SynthesisOptions::default()
        },
    )?;
    let boolean_muxes = synthesized
        .netlist
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind(), NodeKind::Mux))
        .count();
    println!(
        "SYNTH nodes={} mux_nodes={} registers={}",
        synthesized.netlist.nodes().len(),
        boolean_muxes,
        synthesized.netlist.registers().len()
    );
    let drivers: HashMap<_, _> = synthesized
        .netlist
        .nodes()
        .iter()
        .map(|node| (node.output(), node))
        .collect();
    let mut net_memo = HashMap::new();
    if let Some(register) = synthesized
        .netlist
        .registers()
        .iter()
        .find(|register| register.name() == "result_q[0]")
    {
        let stats = net_stats(register.data(), &drivers, &mut net_memo);
        println!(
            "SYNTH_PATH target={} logic_depth={} mux_depth={}",
            register.name(),
            stats.nodes,
            stats.mux_depth
        );
    }
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz,
            ..MappingOptions::default()
        },
    )?;
    let mut lut4 = 0;
    let mut pfu = 0;
    let mut l6 = 0;
    let mut ff = 0;
    for cell in mapped.cells() {
        match cell {
            Ecp5Cell::Lut4 { .. } => lut4 += 1,
            Ecp5Cell::PfuMux { .. } => pfu += 1,
            Ecp5Cell::L6Mux21 { .. } => l6 += 1,
            Ecp5Cell::FlipFlop { .. } => ff += 1,
            _ => {}
        }
    }
    let timing = mapped.retiming();
    println!(
        "MAPPED top={top} cells={} lut4={lut4} pfu={pfu} l6={l6} ff={ff} retimed={} data={}=>{} overall={}=>{} depth={}=>{}",
        mapped.cells().len(),
        timing.applied,
        timing.original_period_ps,
        timing.selected_period_ps,
        timing.original_overall_period_ps,
        timing.selected_overall_period_ps,
        timing.original_lut_depth,
        timing.selected_lut_depth,
    );
    fs::write(output, mapped.to_nextpnr_json()?)?;
    Ok(())
}
