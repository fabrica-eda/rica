//! Performance diagnostics with atomic coincident CPU/memory edges.
//!
//! Celox's timed scheduler evaluates all triggered FF domains before publishing
//! their updates. The manual `Simulator::tick` API cannot express this grouping.
use std::error::Error;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use celox::Simulation;

use crate::{ElfImage, IsaExtensions};

fn value(sim: &mut Simulation, name: &str) -> u64 {
    sim.get(if let Some((instances, leaf)) = name.rsplit_once('.') {
        sim.child_signal(
            &instances
                .split('.')
                .map(|name| (name, 0))
                .collect::<Vec<_>>(),
            leaf,
        )
    } else {
        sim.signal(name)
    })
    .to_u64_digits()
    .first()
    .copied()
    .unwrap_or(0)
}

fn edge(sim: &mut Simulation, clocks: &[&str]) -> Result<(), Box<dyn Error>> {
    let time = sim.time();
    for clock in clocks {
        sim.schedule(clock, time + 1, 0)?;
        sim.schedule(clock, time + 2, 1)?;
    }
    sim.run_until(time + 2)?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct TraceConfig<'a> {
    pub path: &'a Path,
    pub start_cycle: usize,
    pub end_cycle: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionConfig<'a> {
    pub use_dsp: bool,
    pub early_bitmanip: bool,
    pub early_taken_fetch: bool,
    pub early_divide: bool,
    pub machine_interrupts: bool,
    pub trace: Option<TraceConfig<'a>>,
}

// Sample settled state immediately before the named CPU edge. Even-numbered
// cycles also contain a memory edge; both domains still read pre-edge values.
const TRACE_SIGNALS: &[&str] = &[
    "pc",
    "instruction_valid",
    "core.fetch_valid_q",
    "core.fetch_lookahead_valid_q",
    "core.fetch_tail_valid_q",
    "core.predictor_wait",
    "core.predictor_request_valid",
    "core.predictor_request_ready",
    "core.predictor_response_valid",
    "core.predictor_response_accept",
    "core.predictor_response_taken",
    "core.predictor_frontend_redirect",
    "core.fetch_queue_advance",
    "core.rx_valid_q",
    "core.rx_pc_q",
    "core.rx_execution_hazard_q",
    "core.ex_valid_q",
    "core.ex_pc_q",
    "core.ex_is_load_q",
    "core.cm_valid_q",
    "core.cm_pc_q",
    "core.cm_is_load_q",
    "core.cm_retire_event",
    "core.wb_valid_q",
    "core.execution_hazard",
    "core.memory_wait",
    "core.muldiv_wait",
    "core.backend_wait",
    "core.redirect_pending_q",
    "core.branch_redirect_valid",
    "data_addr",
    "data_we",
    "data_request_sequence",
    "data_response_sequence",
    "core_data_response_sequence",
    "data_rdata",
    "core_data_rdata",
    "hardware_memory.data_request_sequence_q",
    "hardware_memory.data_request_handled_q",
    "hardware_memory.data_response_sequence_q",
    "hardware_memory.data_address_q",
    "hardware_memory.data_read_mux",
    "core.csr_file.mcycle_value",
    "core.csr_file.minstret_value",
];

/// This explicit diagnostic compiles a timed model per ELF. Ordinary catalog
/// verification retains its cached native images and both serialized orders.
pub fn run(
    project: &Path,
    elf: &Path,
    predictor_config: u64,
    memory: (u64, u64),
    isa: IsaExtensions,
    execution: ExecutionConfig<'_>,
    max_cycles: usize,
) -> Result<(), Box<dyn Error>> {
    let mut diagnostic = execution
        .trace
        .map(|trace| -> Result<_, Box<dyn Error>> {
            if trace.start_cycle == 0 || trace.end_cycle < trace.start_cycle {
                return Err("trace requires 1 <= start cycle <= end cycle".into());
            }
            let mut output = BufWriter::new(fs::File::create_new(trace.path)?);
            writeln!(output, "cycle,time,mem_rising,{}", TRACE_SIGNALS.join(","))?;
            Ok((trace, output))
        })
        .transpose()?;
    let image = ElfImage::load(elf)?;
    let mut source = crate::core_source(project)?;
    for name in [
        "InstructionBuffer",
        "InstructionCache",
        "HardwareMemory",
        "HardwareCoreTestTop",
    ] {
        source.push_str(&fs::read_to_string(
            project.join(format!("src/{name}.veryl")),
        )?);
    }
    let mut sim = Simulation::builder(&source, "HardwareCoreTestTop")
        .param("PREDICTOR_MODE", predictor_config / 2)
        .param("ICACHE_INDEX_BITS", memory.0)
        .param("FAST_LSU", memory.1)
        .param("ENABLE_M", u64::from(isa.m))
        .param("ENABLE_ZBA", u64::from(isa.zba))
        .param("ENABLE_ZBB", u64::from(isa.zbb))
        .param("ENABLE_ZBS", u64::from(isa.zbs))
        .param("M_USE_DSP", u64::from(execution.use_dsp))
        .param("EARLY_BITMANIP", u64::from(execution.early_bitmanip))
        .param("EARLY_TAKEN_FETCH", u64::from(execution.early_taken_fetch))
        .param("EARLY_DIVIDE", u64::from(execution.early_divide))
        .param("ENABLE_INTERRUPTS", u64::from(execution.machine_interrupts))
        .build()?;
    let reset = sim.signal("rst_n");
    let run = sim.signal("run");
    let debug_write = sim.signal("debug_write");
    let debug_address = sim.signal("debug_address");
    let debug_data = sim.signal("debug_data");
    sim.modify(|io| {
        io.set(reset, 0_u8);
        io.set(run, 0_u8);
        io.set(debug_write, 0_u8);
        io.set(debug_address, 0_u32);
        io.set(debug_data, 0_u8);
    })?;
    edge(&mut sim, &["clk_cpu", "clk_mem"])?;
    sim.modify(|io| io.set(reset, 1_u8))?;
    for (address, byte) in image.load_bytes() {
        if address >= 0x1000 && !(0x8000_0000..0x8003_f000).contains(&address) {
            return Err(format!("hardware RTL address {address:#x} is out of range").into());
        }
        sim.modify(|io| {
            io.set(debug_write, 1_u8);
            io.set(debug_address, address as u32);
            io.set(debug_data, byte);
        })?;
        edge(&mut sim, &["clk_mem"])?;
    }
    sim.modify(|io| io.set(debug_write, 0_u8))?;
    edge(&mut sim, &["clk_mem"])?;
    sim.modify(|io| io.set(run, 1_u8))?;
    let mut commits = 0;
    let mut memory_wait = 0;
    let mut muldiv_wait = 0;
    let mut mailbox_low = 0;
    let mut trace = std::collections::VecDeque::new();
    for cycle in 1..=max_cycles {
        if let Some((config, output)) = &mut diagnostic
            && (config.start_cycle..=config.end_cycle).contains(&cycle)
        {
            write!(
                output,
                "{cycle},{},{}",
                sim.time(),
                u8::from(cycle % 2 == 0)
            )?;
            for signal in TRACE_SIGNALS {
                write!(output, ",{:x}", value(&mut sim, signal))?;
            }
            writeln!(output)?;
        }
        memory_wait += usize::from(value(&mut sim, "core.memory_wait") != 0);
        muldiv_wait += usize::from(value(&mut sim, "core.muldiv_wait") != 0);
        let store = value(&mut sim, "data_we") != 0
            && value(&mut sim, "data_request_sequence")
                == value(&mut sim, "data_response_sequence");
        let mut mailbox = None;
        if store {
            let address = value(&mut sim, "data_addr");
            let data = value(&mut sim, "data_wdata");
            let mask = value(&mut sim, "data_wmask");
            if address == image.tohost && mask == 0xff {
                mailbox = Some(data);
            }
            if address == image.tohost && mask == 0x0f {
                mailbox_low = data & 0xffff_ffff;
            }
            if address == image.tohost + 4 && mask == 0x0f && data & 0xffff_ffff == 0 {
                mailbox = Some(mailbox_low);
            }
        }
        let clocks: &[&str] = if cycle % 2 == 0 {
            &["clk_cpu", "clk_mem"]
        } else {
            &["clk_cpu"]
        };
        edge(&mut sim, clocks)?;
        if value(&mut sim, "commit_valid") != 0 {
            commits += 1;
            trace.push_back((
                value(&mut sim, "commit_pc"),
                value(&mut sim, "commit_instr"),
            ));
            if trace.len() > 20 {
                trace.pop_front();
            }
        }
        if let Some(result) = mailbox.filter(|value| *value != 0) {
            if let Some((_, output)) = &mut diagnostic {
                output.flush()?;
            }
            if result != 1 {
                return Err(format!(
                    "timed model {}: tohost={result}, trace={trace:x?}",
                    elf.display()
                )
                .into());
            }
            let report = serde_json::json!({
                "elf": elf, "clock_order": "simultaneous", "cycles": cycle,
                "commits": commits, "memory_wait_pre_edge": memory_wait,
                "muldiv_wait_pre_edge": muldiv_wait,
                "mcycle": value(&mut sim, "core.csr_file.mcycle_value"),
                "minstret": value(&mut sim, "core.csr_file.minstret_value"),
            });
            println!("Hardware-memory timed RTL: {} PASS", elf.display());
            println!("TIMED_METRIC {report}");
            return Ok(());
        }
    }
    if let Some((_, output)) = &mut diagnostic {
        output.flush()?;
    }
    Err(format!(
        "timed model {} timed out after {commits} commits: {trace:x?}",
        elf.display()
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_edges_read_old_state_independent_of_registration_order() {
        let source = r#"
            module Edges(clk_a: input '_ clock, clk_b: input '_ clock, rst_n: input '_ reset,
                         a: output '_ logic<8>, b: output '_ logic<8>) {
                always_ff(clk_a, rst_n) { if_reset { a = 8'd0; } else { a = a + 8'd1; } }
                always_ff(clk_b, rst_n) { if_reset { b = 8'd0; } else { b = a; } }
            }
        "#;
        for clocks in [["clk_a", "clk_b"], ["clk_b", "clk_a"]] {
            let mut sim = Simulation::builder(source, "Edges").build().unwrap();
            let reset = sim.signal("rst_n");
            sim.modify(|io| io.set(reset, 0_u8)).unwrap();
            edge(&mut sim, &clocks).unwrap();
            sim.modify(|io| io.set(reset, 1_u8)).unwrap();
            for count in 1..=4 {
                edge(&mut sim, &clocks).unwrap();
                assert_eq!(value(&mut sim, "a"), count);
                assert_eq!(value(&mut sim, "b"), count - 1);
            }
            edge(&mut sim, &["clk_a"]).unwrap();
            assert_eq!(value(&mut sim, "a"), 5);
            assert_eq!(value(&mut sim, "b"), 3);
        }
    }
}
