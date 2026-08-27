use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::Instant;
use std::{cmp, collections::HashMap};

use celox::{Simulator, SimulatorBuilder};
use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{Bit, Ecp5Cell, MappingOptions, map_to_ecp5_with_options};

#[derive(Clone, Copy)]
struct Expected {
    pc: u32,
    instr: u32,
    rd: u8,
    data: u32,
    mem_write: Option<(u32, u32)>,
}

fn i_type(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((imm as u32 & 0x0fff) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | u32::from(opcode)
}

fn r_type(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8) -> u32 {
    (u32::from(funct7) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | 0x33
}

fn s_type(imm: i32, rs2: u8, rs1: u8, funct3: u8) -> u32 {
    let imm = imm as u32 & 0x0fff;
    ((imm >> 5) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | ((imm & 0x1f) << 7)
        | 0x23
}

fn b_type(imm: i32, rs2: u8, rs1: u8, funct3: u8) -> u32 {
    let imm = imm as u32 & 0x1fff;
    (((imm >> 12) & 1) << 31)
        | (((imm >> 5) & 0x3f) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (((imm >> 1) & 0x0f) << 8)
        | (((imm >> 11) & 1) << 7)
        | 0x63
}

fn u_type(imm20: u32, rd: u8, opcode: u8) -> u32 {
    ((imm20 & 0x000f_ffff) << 12) | (u32::from(rd) << 7) | u32::from(opcode)
}

fn j_type(imm: i32, rd: u8) -> u32 {
    let imm = imm as u32 & 0x001f_ffff;
    (((imm >> 20) & 1) << 31)
        | (((imm >> 1) & 0x03ff) << 21)
        | (((imm >> 11) & 1) << 20)
        | (((imm >> 12) & 0xff) << 12)
        | (u32::from(rd) << 7)
        | 0x6f
}

fn program() -> Vec<Expected> {
    vec![
        Expected {
            pc: 0,
            instr: i_type(5, 0, 0, 1, 0x13),
            rd: 1,
            data: 5,
            mem_write: None,
        },
        Expected {
            pc: 4,
            instr: i_type(7, 0, 0, 2, 0x13),
            rd: 2,
            data: 7,
            mem_write: None,
        },
        Expected {
            pc: 8,
            instr: r_type(0, 2, 1, 0, 3),
            rd: 3,
            data: 12,
            mem_write: None,
        },
        Expected {
            pc: 12,
            instr: r_type(0x20, 1, 3, 0, 4),
            rd: 4,
            data: 7,
            mem_write: None,
        },
        Expected {
            pc: 16,
            instr: r_type(0, 4, 3, 4, 5),
            rd: 5,
            data: 11,
            mem_write: None,
        },
        Expected {
            pc: 20,
            instr: i_type(0x10, 5, 6, 6, 0x13),
            rd: 6,
            data: 27,
            mem_write: None,
        },
        Expected {
            pc: 24,
            instr: i_type(0x0f, 6, 7, 7, 0x13),
            rd: 7,
            data: 11,
            mem_write: None,
        },
        Expected {
            pc: 28,
            instr: s_type(0, 7, 0, 2),
            rd: 0,
            data: 0,
            mem_write: Some((0, 11)),
        },
        Expected {
            pc: 32,
            instr: i_type(0, 0, 2, 8, 0x03),
            rd: 8,
            data: 11,
            mem_write: None,
        },
        Expected {
            pc: 36,
            instr: b_type(8, 7, 8, 0),
            rd: 0,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 44,
            instr: u_type(1, 9, 0x17),
            rd: 9,
            data: 0x102c,
            mem_write: None,
        },
        Expected {
            pc: 48,
            instr: j_type(8, 10),
            rd: 10,
            data: 52,
            mem_write: None,
        },
        Expected {
            pc: 56,
            instr: i_type(64, 0, 0, 11, 0x13),
            rd: 11,
            data: 64,
            mem_write: None,
        },
        Expected {
            pc: 60,
            instr: i_type(0, 11, 0, 12, 0x67),
            rd: 12,
            data: 64,
            mem_write: None,
        },
        Expected {
            pc: 64,
            instr: i_type(0x55, 0, 0, 31, 0x13),
            rd: 31,
            data: 0x55,
            mem_write: None,
        },
        Expected {
            pc: 68,
            instr: 0x0000_0073,
            rd: 0,
            data: 0,
            mem_write: None,
        },
    ]
}

macro_rules! define_verifier {
($function:ident, $read_value:ident, $simulator_type:path) => {
fn $read_value(simulator: &mut $simulator_type, name: &str) -> u32 {
    simulator
        .get(simulator.signal(name))
        .to_u32_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn $function(
    simulator: &mut $simulator_type,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let instruction = simulator.signal("instr");
    let data_rdata = simulator.signal("data_rdata");
    let expected = program();

    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(instruction, 0x0000_0013_u32);
        io.set(data_rdata, 0_u32);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| io.set(reset, 1_u8))?;

    let mut memory_word = 0_u32;
    let mut committed = 0_usize;
    let mut commit_cycles = Vec::new();
    let mut elapsed_cycles = 0_usize;
    for cycle in 0..64 {
        let fetch_pc = $read_value(simulator, "pc");
        let fetched_instruction = expected
            .iter()
            .find(|entry| entry.pc == fetch_pc)
            .map_or(0x0000_0013, |entry| entry.instr);
        let memory_read_data = if $read_value(simulator, "data_addr") == 0 {
            memory_word
        } else {
            0
        };
        let store = ($read_value(simulator, "data_we") != 0).then(|| {
            (
                $read_value(simulator, "data_addr"),
                $read_value(simulator, "data_wdata"),
            )
        });
        simulator.modify(|io| {
            io.set(instruction, fetched_instruction);
            io.set(data_rdata, memory_read_data);
        })?;
        simulator.tick(clock)?;
        elapsed_cycles = cycle + 1;

        if let Some((address, data)) = store {
            if address != 0 {
                return Err(format!("{label}: store used unsupported address 0x{address:08x}").into());
            }
            memory_word = data;
        }

        if $read_value(simulator, "commit_valid") == 0 {
            continue;
        }
        let Some(entry) = expected.get(committed) else {
            return Err(format!("{label}: retired more instructions than expected").into());
        };
        for (signal, expected_value) in [
            ("commit_pc", entry.pc),
            ("commit_instr", entry.instr),
            ("commit_rd", u32::from(entry.rd)),
            ("commit_data", entry.data),
            ("commit_mem_we", u32::from(entry.mem_write.is_some())),
        ] {
            let observed = $read_value(simulator, signal);
            if observed != expected_value {
                return Err(format!(
                    "{label}: {signal} mismatch after PC 0x{:08x}: expected 0x{expected_value:08x}, got 0x{observed:08x}",
                    entry.pc
                )
                .into());
            }
        }
        if let Some((address, data)) = entry.mem_write {
            let observed_address = $read_value(simulator, "commit_mem_addr");
            let observed_data = $read_value(simulator, "commit_mem_wdata");
            if (observed_address, observed_data) != (address, data) {
                return Err(format!(
                    "{label}: committed store mismatch at PC 0x{:08x}: expected [0x{address:08x}] = 0x{data:08x}, got [0x{observed_address:08x}] = 0x{observed_data:08x}",
                    entry.pc
                )
                .into());
            }
        }
        commit_cycles.push(cycle);
        committed += 1;
        if committed == expected.len() {
            break;
        }
    }

    if committed != expected.len() {
        return Err(format!(
            "{label}: retired {committed} instructions in {elapsed_cycles} clocks, expected {}",
            expected.len()
        )
        .into());
    }
    if commit_cycles[..8]
        .windows(2)
        .any(|cycles| cycles[1] != cycles[0] + 1)
    {
        return Err(format!("{label}: dependent ALU forwarding did not sustain one commit per clock").into());
    }
    if elapsed_cycles > 36 {
        return Err(format!(
            "{label}: architectural trace took {elapsed_cycles} clocks, expected at most 36"
        )
        .into());
    }
    if $read_value(simulator, "halted") != 1 || $read_value(simulator, "debug_x31") != 0x55 {
        return Err(format!("{label}: final architectural signature is invalid").into());
    }
    println!(
        "{label}: {} instructions in {elapsed_cycles} clocks ({:.3} CPI); dependent ALU forwarding CPI 1; signature x31=0x00000055",
        expected.len(),
        elapsed_cycles as f64 / expected.len() as f64,
    );
    Ok(())
}
};
}

define_verifier!(verify_rtl, value_rtl, Simulator);
define_verifier!(verify_post_map, value_post_map, celox03::Simulator);

fn verify_register_file(project: &Path) -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(project.join("src/RegisterFile.veryl"))?;
    let mut simulator = SimulatorBuilder::new(&source, "RegisterFile").build_native()?;
    let clock = simulator.event("clk");
    let write_enable = simulator.signal("write_enable");
    let write_address = simulator.signal("write_address");
    let write_data = simulator.signal("write_data");
    let read_address_a = simulator.signal("read_address_a");
    let read_address_b = simulator.signal("read_address_b");

    simulator.modify(|io| {
        io.set(write_enable, 1_u8);
        io.set(write_address, 5_u8);
        io.set(write_data, 0x1234_5678_u32);
        io.set(read_address_a, 0_u8);
        io.set(read_address_b, 0_u8);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| {
        io.set(write_address, 7_u8);
        io.set(write_data, 0xcafe_babe_u32);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| {
        io.set(write_enable, 0_u8);
        io.set(read_address_a, 5_u8);
        io.set(read_address_b, 7_u8);
    })?;
    simulator.tick(clock)?;

    for (signal, expected) in [("read_data_a", 0x1234_5678), ("read_data_b", 0xcafe_babe)] {
        let observed = simulator
            .get(simulator.signal(signal))
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
        if observed != expected {
            return Err(format!(
                "register file {signal} mismatch: expected 0x{expected:08x}, got 0x{observed:08x}"
            )
            .into());
        }
    }

    simulator.modify(|io| {
        io.set(write_enable, 1_u8);
        io.set(write_address, 0_u8);
        io.set(write_data, u32::MAX);
        io.set(read_address_a, 0_u8);
        io.set(read_address_b, 0_u8);
    })?;
    simulator.tick(clock)?;
    for signal in ["read_data_a", "read_data_b"] {
        let observed = simulator
            .get(simulator.signal(signal))
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
        if observed != 0 {
            return Err(format!("register file {signal} did not mask x0").into());
        }
    }

    let design = analyze_and_lower(&source, "rica_riscv", "RegisterFile")?;
    let synthesized = synthesize(&design)?;
    if synthesized.netlist.memories().len() != 2 {
        return Err(format!(
            "register file inferred {} memories; expected two replicated 1R1W banks",
            synthesized.netlist.memories().len()
        )
        .into());
    }
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 150,
            ..MappingOptions::default()
        },
    )?;
    let block_rams = mapped
        .cells()
        .iter()
        .filter(|cell| matches!(cell, Ecp5Cell::BlockRam { .. }))
        .count();
    if block_rams != 4 {
        return Err(
            format!("register file mapped to {block_rams} DP16KD cells; expected four").into(),
        );
    }
    println!(
        "Register file @ 150 MHz: dual-read behavior passed; 2 logical memories -> {block_rams} DP16KD cells, data={} -> {} ps, overall={} -> {} ps",
        mapped.retiming().original_period_ps,
        mapped.retiming().selected_period_ps,
        mapped.retiming().original_overall_period_ps,
        mapped.retiming().selected_overall_period_ps,
    );
    Ok(())
}

fn mapped_critical_sinks(cells: &[Ecp5Cell]) -> Vec<(usize, String)> {
    fn bit_depth(bit: Bit, depths: &HashMap<u32, usize>) -> usize {
        match bit {
            Bit::Wire(wire) => depths.get(&wire).copied().unwrap_or(0),
            _ => 0,
        }
    }

    let mut depths = HashMap::<u32, usize>::new();
    let mut sinks = Vec::<(usize, String)>::new();
    for cell in cells {
        match cell {
            Ecp5Cell::Lut4 { inputs, output, .. } => {
                let depth = inputs
                    .iter()
                    .map(|bit| bit_depth(*bit, &depths))
                    .max()
                    .unwrap_or(0)
                    + 1;
                depths.insert(*output, depth);
            }
            Ecp5Cell::Ccu2c {
                name,
                inputs,
                carry_in,
                sums,
                carry_out,
                ..
            } => {
                let input_depth = inputs
                    .iter()
                    .flatten()
                    .map(|bit| bit_depth(*bit, &depths))
                    .max()
                    .unwrap_or(0);
                let depth = cmp::max(input_depth, bit_depth(*carry_in, &depths)) + 1;
                for output in sums.iter().chain(std::iter::once(carry_out)) {
                    depths.insert(*output, depth);
                }
                sinks.push((depth, format!("{name} carry output")));
            }
            Ecp5Cell::FlipFlop {
                name, data, output, ..
            } => {
                sinks.push((bit_depth(*data, &depths), name.clone()));
                depths.insert(*output, 0);
            }
            Ecp5Cell::BlockRam { read_data, .. } => {
                for output in read_data {
                    depths.insert(*output, 0);
                }
            }
            _ => {}
        }
    }
    sinks.sort_by(|lhs, rhs| rhs.cmp(lhs));
    sinks.truncate(12);
    sinks
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let map_only = arguments.iter().any(|argument| argument == "--map-only");
    let post_map = arguments.iter().any(|argument| argument == "--post-map");
    let register_file_only = arguments
        .iter()
        .any(|argument| argument == "--regfile-only");
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    verify_register_file(project)?;
    if register_file_only {
        return Ok(());
    }
    let core_source = [
        "PipelineInterfaces.veryl",
        "RegisterFile.veryl",
        "Decoder.veryl",
        "ExecuteUnit.veryl",
        "Core.veryl",
    ]
    .into_iter()
    .map(|file| fs::read_to_string(project.join("src").join(file)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let mut rtl = SimulatorBuilder::new(&core_source, "Core").build_native()?;
    verify_rtl(&mut rtl, "Celox RTL simulation")?;

    let design = analyze_and_lower(&core_source, "rica_riscv", "Core")?;
    let synthesized = synthesize(&design)?;
    let map_started = Instant::now();
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 150,
            ..MappingOptions::default()
        },
    )?;
    let map_elapsed = map_started.elapsed();
    if !mapped.retiming().equivalence_signed_off {
        return Err("Struo mapping equivalence was not signed off".into());
    }

    println!(
        "Struo ECP5 map @ 150 MHz: {} nodes, {} registers, {} cells, data={} -> {} ps, overall={} -> {} ps, LUT depth={} -> {}, critical registers={} -> {}, retimed={}, map={map_elapsed:?}",
        synthesized.netlist.nodes().len(),
        synthesized.netlist.registers().len(),
        mapped.cells().len(),
        mapped.retiming().original_period_ps,
        mapped.retiming().selected_period_ps,
        mapped.retiming().original_overall_period_ps,
        mapped.retiming().selected_overall_period_ps,
        mapped.retiming().original_lut_depth,
        mapped.retiming().selected_lut_depth,
        mapped.retiming().original_critical_registers,
        mapped.retiming().selected_critical_registers,
        mapped.retiming().applied,
    );
    println!(
        "Deepest mapped sinks: {:?}",
        mapped_critical_sinks(mapped.cells())
    );

    if map_only || !post_map {
        if !post_map {
            println!(
                "Post-map simulation skipped; pass --post-map to run the expensive scalar-core regression"
            );
        }
        return Ok(());
    }

    let mut post_map = ecp5_simulator(&mapped)?.build_native()?;
    verify_post_map(&mut post_map, "Celox post-map simulation")?;
    println!(
        "Struo: {} nodes, {} registers, {} ECP5 cells; equivalence passed",
        synthesized.netlist.nodes().len(),
        synthesized.netlist.registers().len(),
        mapped.cells().len()
    );
    Ok(())
}
