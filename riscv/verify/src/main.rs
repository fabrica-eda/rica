use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{cmp, collections::HashMap};

use celox::{BigUint, NativeProgramImage, SimulatorBuilder};
use object::{Object, ObjectSymbol};
use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{Bit, Ecp5Cell, MappingOptions, map_to_ecp5_with_options};
use verify_riscv::ElfImage;

#[derive(Clone, Copy)]
struct Expected {
    pc: u64,
    instr: u32,
    rd: u8,
    data: u64,
    mem_write: Option<(u64, u64, u8)>,
}

fn i_type(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((imm as u32 & 0x0fff) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | u32::from(opcode)
}

fn r_type(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8) -> u32 {
    r_type_opcode(funct7, rs2, rs1, funct3, rd, 0x33)
}

fn r_type_opcode(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (u32::from(funct7) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | u32::from(opcode)
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
            mem_write: Some((0, 11, 0x0f)),
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
            instr: u_type(0x80000, 13, 0x37),
            rd: 13,
            data: 0xffff_ffff_8000_0000,
            mem_write: None,
        },
        Expected {
            pc: 68,
            instr: i_type(-1, 13, 0, 14, 0x1b),
            rd: 14,
            data: 0x0000_0000_7fff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 72,
            instr: r_type_opcode(0, 14, 14, 0, 15, 0x3b),
            rd: 15,
            data: 0xffff_ffff_ffff_fffe,
            mem_write: None,
        },
        Expected {
            pc: 76,
            instr: r_type_opcode(0x20, 14, 15, 0, 16, 0x3b),
            rd: 16,
            data: 0x0000_0000_7fff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 80,
            instr: i_type(33, 16, 1, 17, 0x13),
            rd: 17,
            data: 0xffff_fffe_0000_0000,
            mem_write: None,
        },
        Expected {
            pc: 84,
            instr: i_type(32, 17, 5, 18, 0x13),
            rd: 18,
            data: 0x0000_0000_ffff_fffe,
            mem_write: None,
        },
        Expected {
            pc: 88,
            instr: i_type(0x420, 17, 5, 19, 0x13),
            rd: 19,
            data: 0xffff_ffff_ffff_fffe,
            mem_write: None,
        },
        Expected {
            pc: 92,
            instr: i_type(1, 16, 1, 20, 0x1b),
            rd: 20,
            data: 0xffff_ffff_ffff_fffe,
            mem_write: None,
        },
        Expected {
            pc: 96,
            instr: i_type(31, 20, 5, 21, 0x1b),
            rd: 21,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 100,
            instr: i_type(0x41f, 20, 5, 22, 0x1b),
            rd: 22,
            data: 0xffff_ffff_ffff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 104,
            instr: i_type(63, 0, 0, 23, 0x13),
            rd: 23,
            data: 63,
            mem_write: None,
        },
        Expected {
            pc: 108,
            instr: r_type(0, 23, 21, 1, 24),
            rd: 24,
            data: 0x8000_0000_0000_0000,
            mem_write: None,
        },
        Expected {
            pc: 112,
            instr: r_type(0, 23, 24, 5, 25),
            rd: 25,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 116,
            instr: r_type(0x20, 23, 24, 5, 26),
            rd: 26,
            data: 0xffff_ffff_ffff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 120,
            instr: r_type_opcode(0, 23, 21, 1, 27, 0x3b),
            rd: 27,
            data: 0xffff_ffff_8000_0000,
            mem_write: None,
        },
        Expected {
            pc: 124,
            instr: r_type_opcode(0, 23, 27, 5, 28, 0x3b),
            rd: 28,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 128,
            instr: r_type_opcode(0x20, 23, 27, 5, 29, 0x3b),
            rd: 29,
            data: 0xffff_ffff_ffff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 132,
            instr: i_type(1, 1, 1, 30, 0x13),
            rd: 30,
            data: 10,
            mem_write: None,
        },
        Expected {
            pc: 136,
            instr: i_type(1, 2, 5, 30, 0x13),
            rd: 30,
            data: 3,
            mem_write: None,
        },
        Expected {
            pc: 140,
            instr: i_type(0x401, 13, 5, 30, 0x13),
            rd: 30,
            data: 0xffff_ffff_c000_0000,
            mem_write: None,
        },
        Expected {
            pc: 144,
            instr: r_type(0, 1, 13, 2, 30),
            rd: 30,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 148,
            instr: r_type(0, 1, 13, 3, 30),
            rd: 30,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 152,
            instr: i_type(0, 13, 2, 30, 0x13),
            rd: 30,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 156,
            instr: i_type(-1, 0, 3, 30, 0x13),
            rd: 30,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 160,
            instr: b_type(8, 1, 13, 4),
            rd: 0,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 168,
            instr: b_type(8, 13, 1, 5),
            rd: 0,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 176,
            instr: b_type(8, 13, 1, 6),
            rd: 0,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 184,
            instr: b_type(8, 1, 13, 7),
            rd: 0,
            data: 0,
            mem_write: None,
        },
        Expected {
            pc: 192,
            instr: s_type(0, 22, 0, 0),
            rd: 0,
            data: 0,
            mem_write: Some((0, 0xffff_ffff_ffff_ffff, 0x01)),
        },
        Expected {
            pc: 196,
            instr: i_type(0, 0, 0, 30, 0x03),
            rd: 30,
            data: 0xffff_ffff_ffff_ffff,
            mem_write: None,
        },
        Expected {
            pc: 200,
            instr: i_type(0, 0, 4, 30, 0x03),
            rd: 30,
            data: 0x0000_0000_0000_00ff,
            mem_write: None,
        },
        Expected {
            pc: 204,
            instr: s_type(0, 15, 0, 1),
            rd: 0,
            data: 0,
            mem_write: Some((0, 0xffff_ffff_ffff_fffe, 0x03)),
        },
        Expected {
            pc: 208,
            instr: i_type(0, 0, 1, 30, 0x03),
            rd: 30,
            data: 0xffff_ffff_ffff_fffe,
            mem_write: None,
        },
        Expected {
            pc: 212,
            instr: i_type(0, 0, 5, 30, 0x03),
            rd: 30,
            data: 0x0000_0000_0000_fffe,
            mem_write: None,
        },
        Expected {
            pc: 216,
            instr: s_type(0, 13, 0, 2),
            rd: 0,
            data: 0,
            mem_write: Some((0, 0xffff_ffff_8000_0000, 0x0f)),
        },
        Expected {
            pc: 220,
            instr: i_type(0, 0, 2, 30, 0x03),
            rd: 30,
            data: 0xffff_ffff_8000_0000,
            mem_write: None,
        },
        Expected {
            pc: 224,
            instr: i_type(0, 0, 6, 30, 0x03),
            rd: 30,
            data: 0x0000_0000_8000_0000,
            mem_write: None,
        },
        Expected {
            pc: 228,
            instr: s_type(0, 17, 0, 3),
            rd: 0,
            data: 0,
            mem_write: Some((0, 0xffff_fffe_0000_0000, 0xff)),
        },
        Expected {
            pc: 232,
            instr: i_type(0, 0, 3, 30, 0x03),
            rd: 30,
            data: 0xffff_fffe_0000_0000,
            mem_write: None,
        },
        Expected {
            pc: 236,
            instr: i_type(1, 0, 0, 30, 0x13),
            rd: 30,
            data: 1,
            mem_write: None,
        },
        Expected {
            pc: 240,
            instr: i_type(5, 30, 1, 30, 0x13),
            rd: 30,
            data: 32,
            mem_write: None,
        },
        Expected {
            pc: 244,
            instr: i_type(1, 30, 0, 30, 0x13),
            rd: 30,
            data: 33,
            mem_write: None,
        },
        Expected {
            pc: 248,
            instr: i_type(0x55, 0, 0, 31, 0x13),
            rd: 31,
            data: 0x55,
            mem_write: None,
        },
        Expected {
            pc: 252,
            instr: j_type(0, 0),
            rd: 0,
            data: 0,
            mem_write: None,
        },
    ]
}

macro_rules! define_verifier {
($function:ident, $read_value:ident, $simulator_type:path) => {
fn $read_value(simulator: &mut $simulator_type, name: &str) -> u64 {
    simulator
        .get(simulator.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn $function(
    simulator: &mut $simulator_type,
    label: &str,
    expected_trace_cycles: Option<usize>,
) -> Result<(), Box<dyn Error>> {
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let instruction = simulator.signal("instr");
    let instruction_compressed = simulator.signal("instr_is_compressed");
    let instruction_valid = simulator.signal("instr_valid");
    let data_rdata = simulator.signal("data_rdata");
    let data_response_sequence = simulator.signal("data_response_sequence");
    let counter_freeze = simulator.signal("counter_freeze");
    let debug_counter_index = simulator.signal("debug_counter_index");
    let expected = program();

    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(instruction, 0x0000_0013_u32);
        io.set(instruction_compressed, 0_u8);
        io.set(instruction_valid, 1_u8);
        io.set(data_rdata, 0_u64);
        io.set(data_response_sequence, 0_u8);
        io.set(counter_freeze, 0_u8);
        io.set(debug_counter_index, 0_u8);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| io.set(reset, 1_u8))?;

    let mut memory_word = 0_u64;
    let mut committed = 0_usize;
    let mut commit_cycles = Vec::new();
    let mut elapsed_cycles = 0_usize;
    // Compact BTB storage is scrubbed on reset, including in the native model.
    let trace_limit = if expected_trace_cycles.is_some() { 140 } else { 256 };
    for cycle in 0..trace_limit {
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
                $read_value(simulator, "data_wmask") as u8,
            )
        });
        simulator.modify(|io| {
            io.set(instruction, fetched_instruction);
            io.set(
                instruction_compressed,
                u8::from(fetched_instruction & 0b11 != 0b11),
            );
            io.set(data_rdata, memory_read_data);
        })?;
        simulator.tick(clock)?;
        elapsed_cycles = cycle + 1;

        if let Some((address, data, mask)) = store {
            if address != 0 {
                return Err(format!("{label}: store used unsupported address 0x{address:016x}").into());
            }
            for byte in 0..8 {
                if mask & (1 << byte) != 0 {
                    let byte_mask = 0xff_u64 << (byte * 8);
                    memory_word = (memory_word & !byte_mask) | (data & byte_mask);
                }
            }
        }

        if $read_value(simulator, "commit_valid") == 0 {
            continue;
        }
        let Some(entry) = expected.get(committed) else {
            return Err(format!("{label}: retired more instructions than expected").into());
        };
        for (signal, expected_value) in [
            ("commit_pc", entry.pc),
            ("commit_instr", u64::from(entry.instr)),
            ("commit_rd", u64::from(entry.rd)),
            ("commit_data", entry.data),
            ("commit_mem_we", u64::from(entry.mem_write.is_some())),
        ] {
            let observed = $read_value(simulator, signal);
            if observed != expected_value {
                return Err(format!(
                    "{label}: {signal} mismatch after PC 0x{:016x}: expected 0x{expected_value:016x}, got 0x{observed:016x}",
                    entry.pc,
                )
                .into());
            }
        }
        if let Some((address, data, mask)) = entry.mem_write {
            let observed_address = $read_value(simulator, "commit_mem_addr");
            let observed_data = $read_value(simulator, "commit_mem_wdata");
            let observed_mask = $read_value(simulator, "commit_mem_wmask");
            if (observed_address, observed_data, observed_mask) != (address, data, u64::from(mask)) {
                return Err(format!(
                    "{label}: committed store mismatch at PC 0x{:016x}: expected [0x{address:016x}]/0x{mask:02x} = 0x{data:016x}, got [0x{observed_address:016x}]/0x{observed_mask:02x} = 0x{observed_data:016x}",
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
    if commit_cycles[31..34]
        .windows(2)
        .any(|cycles| cycles[1] != cycles[0] + 1)
    {
        return Err(format!("{label}: pipelined shifts did not sustain one completion per clock").into());
    }
    if commit_cycles[54..56]
        .windows(2)
        .any(|cycles| cycles[1] != cycles[0] + 2)
    {
        return Err(format!("{label}: dependent shift-to-ALU latency was not exactly two clocks").into());
    }
    // Dynamic prediction includes its registered ingress and state repairs.
    // The static response is shorter and repairs only a wrong next PC.
    // Preserve the established 126/105-clock contracts for those predictors.
    // Experimental predictors still check every retirement and execution
    // latency above, without inheriting another frontend's cycle count.
    if let Some(expected) = expected_trace_cycles {
        if elapsed_cycles != expected {
            return Err(format!(
                "{label}: architectural trace took {elapsed_cycles} clocks, expected exactly {expected}"
            )
            .into());
        }
    }
    if $read_value(simulator, "halted") != 0 || $read_value(simulator, "debug_x31") != 0x55 {
        return Err(format!("{label}: final architectural signature is invalid").into());
    }
    let mut counters = [0_u64; 11];
    for (index, counter) in counters.iter_mut().enumerate() {
        simulator.modify(|io| io.set(debug_counter_index, index as u8))?;
        *counter = $read_value(simulator, "debug_counter_data");
    }
    let classified_cycles = counters[2..6].iter().sum::<u64>();
    if counters[0] == 0
        || counters[1] < expected.len() as u64
        || counters[1] > counters[0]
        || counters[2] == 0
        || counters[3] != 0
        || counters[4] == 0
        || counters[5] == 0
        || classified_cycles > counters[0]
        || counters[6] > counters[1]
        || counters[7] > counters[6]
        || counters[8] > counters[1]
        || counters[9] > counters[1]
        || counters[10] > counters[4]
    {
        return Err(format!(
            "{label}: invalid performance counters: mcycle={} minstret={} dependency={} memory={} redirect={} frontend={} conditional={} direction_wrong={} btb_miss={} ras_fallback={} repair={}",
            counters[0], counters[1], counters[2], counters[3], counters[4], counters[5],
            counters[6], counters[7], counters[8], counters[9], counters[10]
        )
        .into());
    }
    println!(
        "{label}: {} instructions in {elapsed_cycles} clocks ({:.3} CPI); two-stage shifts and comparisons sustain one completion per clock and dependent consumers wait once; counters={:?}; signature x31=0x0000000000000055",
        expected.len(),
        elapsed_cycles as f64 / expected.len() as f64,
        counters,
    );
    Ok(())
}
};
}

define_verifier!(
    verify_native,
    value_native,
    celox::Simulator<celox::NativeBackend>
);
define_verifier!(verify_post_map, value_post_map, celox::Simulator);

#[derive(Clone, Copy)]
enum CoincidentEdgeOrder {
    MemoryThenCpu,
    CpuThenMemory,
}

impl CoincidentEdgeOrder {
    fn label(self) -> &'static str {
        match self {
            Self::MemoryThenCpu => "mem->cpu",
            Self::CpuThenMemory => "cpu->mem",
        }
    }
}

fn verify_hardware_memory_elf(
    model: &NativeProgramImage,
    elf: &Path,
    label: &str,
    coincident_order: CoincidentEdgeOrder,
    platform: verify_riscv::platform::PlatformTiming,
    kernel_console: bool,
    max_cycles: usize,
) -> Result<(), Box<dyn Error>> {
    let image = ElfImage::load(elf)?;
    let elf_bytes = fs::read(elf)?;
    let object = object::File::parse(elf_bytes.as_slice())?;
    let failure_entries = object
        .symbols()
        .filter_map(|symbol| {
            let name = symbol.name().ok()?;
            matches!(
                name,
                "failedtest_x5_x4" | "failedtest_x8_x7" | "failedtest_x14_x13"
            )
            .then_some(symbol.address())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut loaded = Vec::<(u32, u8)>::new();
    for (address, byte) in image.load_bytes() {
        let in_trampoline = address < 0x1000;
        let in_act4 = (0x8000_0000..0x8003_f000).contains(&address);
        if !in_trampoline && !in_act4 {
            return Err(format!("hardware RTL address 0x{address:016x} is out of range").into());
        }
        loaded.push((u32::try_from(address)?, byte));
    }
    let loaded_bytes = loaded.len();

    let mut simulator =
        SimulatorBuilder::new("", "HardwareCoreTestTop").build_native_from_image(model.clone())?;
    let clk_cpu = simulator.event("clk_cpu");
    let clk_mem = simulator.event("clk_mem");
    let rst_n = simulator.signal("rst_n");
    let run = simulator.signal("run");
    let debug_write = simulator.signal("debug_write");
    let debug_address = simulator.signal("debug_address");
    let debug_data = simulator.signal("debug_data");
    simulator.modify(|io| {
        io.set(rst_n, 0_u8);
        io.set(run, 0_u8);
        io.set(debug_write, 0_u8);
        io.set(debug_address, 0_u32);
        io.set(debug_data, 0_u8);
    })?;
    simulator.tick(clk_cpu)?;
    simulator.tick(clk_mem)?;
    // Match HardwareTestTop: debug/system reset is released while run_q keeps
    // the CPU and instruction buffer reset during byte-wise BRAM loading.
    simulator.modify(|io| io.set(rst_n, 1_u8))?;
    for (address, byte) in loaded {
        simulator.modify(|io| {
            io.set(debug_write, 1_u8);
            io.set(debug_address, address);
            io.set(debug_data, byte);
        })?;
        simulator.tick(clk_mem)?;
    }
    simulator.modify(|io| io.set(debug_write, 0_u8))?;
    simulator.tick(clk_mem)?;
    simulator.modify(|io| io.set(run, 1_u8))?;

    let mut console = kernel_console.then(verify_riscv::console_host::KernelConsole::default);
    let mut console_pending = false;
    let mut console_sequence = 0_u8;
    let mut console_terminal = false;
    let console_request = simulator.signal("console_host_request_sequence");
    let console_pop = simulator.signal("console_host_pop");
    let console_push = simulator.signal("console_host_push");
    let console_data = simulator.signal("console_host_data");
    let mut trace = std::collections::VecDeque::with_capacity(20);
    let mut mailbox_low = 0_u64;
    let mut commits = 0_usize;
    let mut first_commit_cycle = None;
    let mut reset_entries = 0_usize;
    for cycle in 1..=max_cycles {
        if let Some(host) = &mut console {
            if console_pending
                && value_native(&mut simulator, "console_host_response_sequence")
                    == u64::from(console_sequence)
            {
                let response = value_native(&mut simulator, "console_host_response") as u16;
                host.accept(response)?;
                console_pending = false;
                if console_terminal && response & (1 << 8) == 0 {
                    host.validate()?;
                    println!(
                        "CONSOLE_TRANSCRIPT {}",
                        serde_json::json!({"elf": elf, "clock_order": coincident_order.label(), "text": host.transcript()?})
                    );
                    return Ok(());
                }
            }
            // Deliberately backpressure the 16-byte FIFO and hold each reply.
            if !console_pending && cycle % 256 == 0 {
                console_sequence ^= 1;
                let input = host.input();
                simulator.modify(|io| {
                    io.set(console_request, console_sequence);
                    io.set(console_pop, 1_u8);
                    io.set(console_push, u8::from(input.is_some()));
                    io.set(console_data, input.unwrap_or(0));
                })?;
                console_pending = true;
            }
        }
        // Manual tick handles one event at a time. These two serializations
        // stress request and response ordering; neither measures hardware
        // cycles. Use --timed-clocks for atomic coincident-edge diagnostics.
        if cycle % platform.memory_divisor() == 0
            && matches!(coincident_order, CoincidentEdgeOrder::MemoryThenCpu)
        {
            simulator.tick(clk_mem)?;
        }
        // A request is architecturally complete only when HardwareMemory has
        // returned the matching toggle. Sample immediately before the CPU edge
        // that consumes it: a preceding memory edge can produce the ACK and
        // the CPU edge can drop data_we again in the same serialized cycle.
        let store_acknowledged = value_native(&mut simulator, "data_we") != 0
            && value_native(&mut simulator, "data_store_retire_ready") != 0
            && value_native(&mut simulator, "data_response_sequence")
                == value_native(&mut simulator, "data_request_sequence");
        let store = store_acknowledged.then(|| {
            (
                value_native(&mut simulator, "data_addr"),
                value_native(&mut simulator, "data_wdata"),
                value_native(&mut simulator, "data_wmask"),
            )
        });
        simulator.tick(clk_cpu)?;
        if cycle % platform.memory_divisor() == 0
            && matches!(coincident_order, CoincidentEdgeOrder::CpuThenMemory)
        {
            simulator.tick(clk_mem)?;
        }
        if value_native(&mut simulator, "commit_valid") != 0 {
            first_commit_cycle.get_or_insert(cycle);
            commits += 1;
            let commit_pc = value_native(&mut simulator, "commit_pc");
            if failure_entries.contains(&commit_pc) {
                let recent = trace
                    .iter()
                    .map(|(pc, instr, rd, value)| {
                        format!("  pc={pc:#06x} instr={instr:#010x} rd=x{rd} data={value:#018x}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(format!(
                    "hardware-memory RTL {label} entered {commit_pc:#010x} at cycle {cycle}\n{recent}"
                )
                .into());
            }
            if commit_pc == 0 {
                reset_entries += 1;
                if reset_entries == 2 {
                    let recent = trace
                        .iter()
                        .map(|(pc, instr, rd, value)| {
                            format!(
                                "  pc={pc:#06x} instr={instr:#010x} rd=x{rd} data={value:#018x}"
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Err(format!(
                        "hardware-memory RTL {label} unexpectedly re-entered the reset trampoline at cycle {cycle}\n{recent}"
                    )
                    .into());
                }
            }
            if trace.len() == trace.capacity() {
                trace.pop_front();
            }
            trace.push_back((
                commit_pc,
                value_native(&mut simulator, "commit_instr"),
                value_native(&mut simulator, "commit_rd"),
                value_native(&mut simulator, "commit_data"),
            ));
        }
        let mailbox = store.and_then(|(address, data, mask)| {
            if address == image.tohost && mask == 0xff {
                Some(data)
            } else if address == image.tohost && mask == 0x0f {
                mailbox_low = data & 0xffff_ffff;
                None
            } else if address == image.tohost + 4 && mask == 0x0f && data & 0xffff_ffff == 0 {
                Some(mailbox_low)
            } else {
                None
            }
        });
        if let Some(result) = mailbox
            && result != 0
        {
            if result != 1 {
                let recent = trace
                    .iter()
                    .map(|(pc, instr, rd, value)| {
                        format!("  pc={pc:#06x} instr={instr:#010x} rd=x{rd} data={value:#018x}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(format!(
                    "hardware-memory RTL {label} failed: tohost={result} (test {}) at cycle {cycle}\n{recent}",
                    result >> 1
                )
                .into());
            }
            let first_commit =
                first_commit_cycle.ok_or("terminal store observed without a commit")?;
            println!(
                "Hardware-memory RTL: {label:<12} [{}] PASS; {loaded_bytes} debug bytes loaded, {commits} commits in {cycle} cycles; first commit={first_commit}, execution window={} cycles",
                coincident_order.label(),
                cycle - first_commit + 1,
            );
            if console.is_none() {
                return Ok(());
            }
            console_terminal = true;
        }
    }
    let recent = trace
        .iter()
        .map(|(pc, instr, rd, value)| {
            format!("  pc={pc:#06x} instr={instr:#010x} rd=x{rd} data={value:#018x}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "hardware-memory RTL {label} timed out after {commits} commits and {reset_entries} reset entries; pc={:#x}, instruction={:#x}, instruction_valid={}, memory_address={:#x}, memory_line={:#x}, memory_valid={}, request_sequence={}, response_sequence={}\n{recent}",
        value_native(&mut simulator, "pc"),
        value_native(&mut simulator, "instruction"),
        value_native(&mut simulator, "instruction_valid"),
        value_native(&mut simulator, "memory_instr_address"),
        value_native(&mut simulator, "memory_instr_line"),
        value_native(&mut simulator, "memory_instr_response_valid"),
        value_native(&mut simulator, "memory_instr_request_sequence"),
        value_native(&mut simulator, "memory_instr_response_sequence"),
    )
    .into())
}

struct HardwareModelConfig {
    predictor_config: u64,
    icache_index_bits: u64,
    fast_lsu: u64,
    enable_m: bool,
    use_dsp: bool,
    enable_zba: bool,
    enable_zbb: bool,
    enable_zbs: bool,
    early_bitmanip: bool,
    early_taken_fetch: bool,
    early_divide: bool,
    machine_interrupts: bool,
    strict_memory: bool,
    enable_a: bool,
    enable_u: bool,
    enable_s: bool,
    enable_sv39: bool,
    machine_console: bool,
    kernel_console: bool,
}

fn verify_hardware_memory_rtl(
    project: &Path,
    requested_elfs: &[PathBuf],
    config: HardwareModelConfig,
    max_cycles: usize,
) -> Result<(), Box<dyn Error>> {
    let HardwareModelConfig {
        predictor_config,
        icache_index_bits,
        fast_lsu,
        enable_m,
        use_dsp,
        enable_zba,
        enable_zbb,
        enable_zbs,
        early_bitmanip,
        early_taken_fetch,
        early_divide,
        machine_interrupts,
        strict_memory,
        enable_a,
        enable_u,
        enable_s,
        enable_sv39,
        machine_console,
        kernel_console,
    } = config;
    let platform = verify_riscv::platform::hardware_platform(project, enable_u)?;
    let hardware_memory = fs::read_to_string(project.join("src/HardwareMemory.veryl"))?;
    let source = [
        verify_riscv::core_source(project)?,
        fs::read_to_string(project.join("src/InstructionBuffer.veryl"))?,
        fs::read_to_string(project.join("src/InstructionCache.veryl"))?,
        hardware_memory,
        fs::read_to_string(project.join("src/HardwareCoreTestTop.veryl"))?,
    ]
    .join("\n");
    let model = SimulatorBuilder::new(&source, "HardwareCoreTestTop")
        .param("PMP_DATA_STAGES", platform.pmp_data_stages)
        .param("EARLY_ALU", u64::from(platform.early_alu))
        .param("DIRECT_LOAD_RETURN", u64::from(platform.direct_load_return))
        .param("MEMORY_HZ", platform.memory_hz)
        .param("PREDICTOR_MODE", predictor_config / 2)
        .param("ICACHE_INDEX_BITS", icache_index_bits)
        .param("FAST_LSU", fast_lsu)
        .param("ENABLE_M", u64::from(enable_m))
        .param("M_USE_DSP", u64::from(use_dsp))
        .param("ENABLE_ZBA", u64::from(enable_zba))
        .param("ENABLE_ZBB", u64::from(enable_zbb))
        .param("ENABLE_ZBS", u64::from(enable_zbs))
        .param("EARLY_BITMANIP", u64::from(early_bitmanip))
        .param("EARLY_TAKEN_FETCH", u64::from(early_taken_fetch))
        .param("EARLY_DIVIDE", u64::from(early_divide))
        .param("ENABLE_INTERRUPTS", u64::from(machine_interrupts))
        .param("STRICT_MEMORY", u64::from(strict_memory))
        .param("ENABLE_A", u64::from(enable_a))
        .param("ENABLE_U", u64::from(enable_u))
        .param("ENABLE_S", u64::from(enable_s))
        .param("ENABLE_SV39", u64::from(enable_sv39))
        .param("ENABLE_CONSOLE", u64::from(machine_console))
        .compile_native()?
        .into_program_image();
    if requested_elfs.is_empty() {
        for name in [
            "alu",
            "compare",
            "compressed",
            "control",
            "memory",
            "performance",
            "shift",
            "system",
        ] {
            let label = format!("{name}.elf");
            for order in [
                CoincidentEdgeOrder::MemoryThenCpu,
                CoincidentEdgeOrder::CpuThenMemory,
            ] {
                verify_hardware_memory_elf(
                    &model,
                    &project.join("build/rv64i-subset").join(&label),
                    &label,
                    order,
                    platform,
                    kernel_console,
                    max_cycles,
                )?;
            }
        }
    } else {
        for elf in requested_elfs {
            let label = elf.file_name().unwrap_or_default().to_string_lossy();
            for order in [
                CoincidentEdgeOrder::MemoryThenCpu,
                CoincidentEdgeOrder::CpuThenMemory,
            ] {
                verify_hardware_memory_elf(
                    &model,
                    elf,
                    &label,
                    order,
                    platform,
                    kernel_console,
                    max_cycles,
                )?;
            }
        }
    }
    Ok(())
}

fn verify_direct_jtag_transport(project: &Path) -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(project.join("src/JtagDebugTransport.veryl"))?;
    let mut simulator = SimulatorBuilder::new(&source, "JtagDebugTransport").build_native()?;
    let clk = simulator.event("clk");
    let jtag_tck = simulator.event("jtag_tck");
    let jtag_update = simulator.signal("jtag_update");
    let update =
        |simulator: &mut celox::Simulator<celox::NativeBackend>| -> Result<(), Box<dyn Error>> {
            for level in [1_u8, 0, 0] {
                simulator.modify(|io| io.set(jtag_update, level))?;
                simulator.tick(jtag_tck)?;
            }
            Ok(())
        };
    let rst_n = simulator.signal("rst_n");
    let jtag_rst_n = simulator.signal("jtag_rst_n");
    let jtag_tdi = simulator.signal("jtag_tdi");
    let jtag_shift = simulator.signal("jtag_shift");
    let jtag_ce1 = simulator.signal("jtag_ce1");
    let jtag_ce2 = simulator.signal("jtag_ce2");
    let response_bit = simulator.signal("response_bit");
    let status_bit = simulator.signal("status_bit");
    let command_valid = simulator.signal("command_valid");
    let command_frame_low = simulator.signal("command_frame_low");
    let command_frame_high = simulator.signal("command_frame_high");
    let jtag_tdo1 = simulator.signal("jtag_tdo1");
    let jtag_tdo2 = simulator.signal("jtag_tdo2");

    simulator.modify(|io| {
        io.set(rst_n, 0_u8);
        io.set(jtag_rst_n, 0_u8);
        io.set(jtag_tdi, 0_u8);
        io.set(jtag_update, 0_u8);
        io.set(jtag_shift, 0_u8);
        io.set(jtag_ce1, 0_u8);
        io.set(jtag_ce2, 0_u8);
        io.set(response_bit, 1_u8);
        io.set(status_bit, 1_u8);
    })?;
    simulator.tick(jtag_tck)?;
    simulator.tick(clk)?;
    simulator.modify(|io| {
        io.set(rst_n, 1_u8);
        io.set(jtag_rst_n, 1_u8);
    })?;
    // The real CPU clock runs throughout a 128-TCK scan. Complete the reset
    // alignment here before presenting the first mailbox update.
    for _ in 0..4 {
        simulator.tick(clk)?;
    }

    if simulator.get(jtag_tdo1) != BigUint::from(1_u8)
        || simulator.get(jtag_tdo2) != BigUint::from(1_u8)
    {
        return Err("direct JTAG stable response outputs are not transparent".into());
    }

    let mut frame = [0_u8; 16];
    frame[0] = 0xd5;
    frame[1] = 0x11;
    frame[2] = 0x6a;
    frame[3..7].copy_from_slice(&0x8001_2340_u32.to_le_bytes());
    frame[7..15].copy_from_slice(&0x8877_6655_4433_2211_u64.to_le_bytes());
    frame[15] = 0xa5;

    // At falling JTCK, JTAGG's JTDI is one cycle behind its controls.
    // The registered shift enable extends reception through the trailing
    // transition; UPDATE is sampled as data in the same clock domain.
    simulator.modify(|io| {
        io.set(jtag_shift, 1_u8);
        io.set(jtag_ce1, 1_u8);
    })?;
    let mut delayed_tdi = false;
    for physical_bit in 0..128 {
        simulator.modify(|io| io.set(jtag_tdi, u8::from(delayed_tdi)))?;
        simulator.tick(jtag_tck)?;
        delayed_tdi = frame[physical_bit / 8] & (1 << (physical_bit % 8)) != 0;
    }
    simulator.modify(|io| {
        io.set(jtag_tdi, u8::from(delayed_tdi));
        io.set(jtag_shift, 0_u8);
        io.set(jtag_ce1, 0_u8);
    })?;
    simulator.tick(jtag_tck)?;
    update(&mut simulator)?;

    let expected_low = BigUint::from(u64::from_le_bytes(frame[0..8].try_into()?));
    let expected_high = BigUint::from(u64::from_le_bytes(frame[8..16].try_into()?));
    let mut observed = None;
    for _ in 0..6 {
        simulator.tick(clk)?;
        if simulator.get(command_valid) != BigUint::from(0_u8) {
            observed = Some((
                simulator.get(command_frame_low),
                simulator.get(command_frame_high),
            ));
        }
    }
    if observed.as_ref() != Some(&(expected_low.clone(), expected_high.clone())) {
        return Err(format!(
            "direct JTAG mailbox mismatch: expected high:low 0x{expected_high:016x}:{expected_low:016x}, observed {observed:?}"
        )
        .into());
    }

    // A PLL-only CPU reset must align to the producer's retained toggle,
    // rather than replaying the most recent JTAG mailbox after relock.
    simulator.modify(|io| io.set(rst_n, 0_u8))?;
    simulator.tick(clk)?;
    simulator.modify(|io| io.set(rst_n, 1_u8))?;
    for _ in 0..8 {
        simulator.tick(clk)?;
        if simulator.get(command_valid) != BigUint::from(0_u8) {
            return Err("CPU-domain relock replayed a stale direct JTAG command".into());
        }
    }

    // Let the UPDATE edge detector clear, then update an unrelated
    // DR chain (neither CE1 nor CE2). It must not reuse the retained ER1 arm.
    for _ in 0..4 {
        simulator.tick(jtag_tck)?;
    }
    update(&mut simulator)?;
    for _ in 0..6 {
        simulator.tick(clk)?;
        if simulator.get(command_valid) != BigUint::from(0_u8) {
            return Err("non-user DR UPDATE replayed a stale ER1 direct JTAG command".into());
        }
    }

    // An ER2 scan updates only the status chain and must not replay the last
    // ER1 mailbox when UPDATE-DR occurs.
    simulator.modify(|io| {
        io.set(jtag_shift, 1_u8);
        io.set(jtag_ce2, 1_u8);
    })?;
    for _ in 0..8 {
        simulator.tick(jtag_tck)?;
    }
    simulator.modify(|io| {
        io.set(jtag_shift, 0_u8);
        io.set(jtag_ce2, 0_u8);
    })?;
    update(&mut simulator)?;
    for _ in 0..6 {
        simulator.tick(clk)?;
        if simulator.get(command_valid) != BigUint::from(0_u8) {
            return Err("ER2 UPDATE-DR replayed a stale ER1 direct JTAG command".into());
        }
    }
    println!("Direct JTAG transport: 128-bit ER1/JTDI alignment and stale-update isolation passed");
    Ok(())
}

fn board_diagnostic_instruction(pc: u64) -> u32 {
    match pc {
        0x00 => i_type(1, 0, 0, 31, 0x13),
        // The board uses a larger LUI delay. Keeping the same instruction and
        // control-flow shape with 4096 iterations makes it practical in RTL.
        0x04 => u_type(1, 1, 0x37),
        0x08 => i_type(-1, 1, 0, 1, 0x13),
        0x0c => b_type(-4, 0, 1, 1),
        0x10 => i_type(1, 31, 0, 31, 0x13),
        0x14 => j_type(-16, 0),
        _ => 0x0000_0073,
    }
}

fn verify_board_diagnostic(
    simulator: &mut celox::Simulator<celox::NativeBackend>,
) -> Result<(), Box<dyn Error>> {
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let instruction = simulator.signal("instr");
    let instruction_compressed = simulator.signal("instr_is_compressed");
    let instruction_valid = simulator.signal("instr_valid");
    let data_rdata = simulator.signal("data_rdata");
    let data_response_sequence = simulator.signal("data_response_sequence");
    let counter_freeze = simulator.signal("counter_freeze");
    let debug_counter_index = simulator.signal("debug_counter_index");

    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(instruction, 0x0000_0013_u32);
        io.set(instruction_compressed, 0_u8);
        io.set(instruction_valid, 1_u8);
        io.set(data_rdata, 0_u64);
        io.set(data_response_sequence, 0_u8);
        io.set(counter_freeze, 0_u8);
        io.set(debug_counter_index, 0_u8);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| io.set(reset, 1_u8))?;

    let mut x31_commits = 0_u64;
    let mut delay_addi_commits = 0_u64;
    let mut branch_commits = 0_u64;
    let mut elapsed_cycles = 0_usize;
    for cycle in 0..50_000 {
        let fetch_pc = value_native(simulator, "pc");
        simulator.modify(|io| {
            io.set(instruction, board_diagnostic_instruction(fetch_pc));
            io.set(instruction_compressed, 0_u8);
        })?;
        simulator.tick(clock)?;
        elapsed_cycles = cycle + 1;

        if value_native(simulator, "commit_valid") == 0 {
            continue;
        }
        match value_native(simulator, "commit_pc") {
            0x08 => {
                delay_addi_commits += 1;
                let expected = 4096 - delay_addi_commits;
                let observed = value_native(simulator, "commit_data");
                if observed != expected {
                    return Err(format!(
                        "board diagnostic: delay ADDI {delay_addi_commits} expected x1={expected}, got {observed}"
                    )
                    .into());
                }
            }
            0x0c => branch_commits += 1,
            _ => {}
        }
        let commit_pc = value_native(simulator, "commit_pc");
        if commit_pc != 0 && commit_pc != 0x10 {
            continue;
        }
        x31_commits += 1;
        let expected_instruction = if commit_pc == 0 {
            i_type(1, 0, 0, 31, 0x13)
        } else {
            i_type(1, 31, 0, 31, 0x13)
        };
        for (signal, expected) in [
            ("commit_instr", u64::from(expected_instruction)),
            ("commit_rd", 31),
            ("commit_data", x31_commits),
        ] {
            let observed = value_native(simulator, signal);
            if observed != expected {
                return Err(format!(
                    "board diagnostic: {signal} mismatch on x31 update {x31_commits}: expected 0x{expected:016x}, got 0x{observed:016x}"
                )
                .into());
            }
        }
        if x31_commits == 2 {
            break;
        }
    }

    if x31_commits != 2
        || value_native(simulator, "debug_x31") != 2
        || delay_addi_commits != 4096
        || branch_commits != 4096
    {
        return Err(format!(
            "board diagnostic: expected one complete 4096-iteration delay before the second x31 update; observed x31={x31_commits}, ADDI={delay_addi_commits}, BNE={branch_commits} in {elapsed_cycles} clocks"
        )
        .into());
    }
    println!(
        "Board diagnostic RTL: x31 advanced 0 -> 1 -> 2 through the ADDI/BNE/JAL loop in {elapsed_cycles} clocks"
    );
    Ok(())
}

fn verify_register_file(project: &Path) -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(project.join("src/RegisterFile.veryl"))?;
    let mut simulator = SimulatorBuilder::new(&source, "RegisterFile").build_native()?;
    let clock = simulator.event("clk");
    let write_enable = simulator.signal("write_enable");
    let write_address = simulator.signal("write_address");
    let write_data = simulator.signal("write_data");
    let read_enable = simulator.signal("read_enable");
    let read_address_a = simulator.signal("read_address_a");
    let read_address_b = simulator.signal("read_address_b");

    simulator.modify(|io| {
        io.set(write_enable, 1_u8);
        io.set(read_enable, 1_u8);
        io.set(write_address, 5_u8);
        io.set(write_data, 0x0123_4567_89ab_cdef_u64);
        io.set(read_address_a, 0_u8);
        io.set(read_address_b, 0_u8);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| {
        io.set(write_address, 7_u8);
        io.set(write_data, 0xfedc_ba98_7654_3210_u64);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| {
        io.set(write_enable, 0_u8);
        io.set(read_address_a, 5_u8);
        io.set(read_address_b, 7_u8);
    })?;
    simulator.tick(clock)?;

    for (signal, expected) in [
        ("read_data_a", 0x0123_4567_89ab_cdef_u64),
        ("read_data_b", 0xfedc_ba98_7654_3210_u64),
    ] {
        let observed = simulator
            .get(simulator.signal(signal))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0);
        if observed != expected {
            return Err(format!(
                "register file {signal} mismatch: expected 0x{expected:016x}, got 0x{observed:016x}"
            )
            .into());
        }
    }

    simulator.modify(|io| {
        io.set(write_enable, 1_u8);
        io.set(write_address, 0_u8);
        io.set(write_data, u64::MAX);
        io.set(read_address_a, 0_u8);
        io.set(read_address_b, 0_u8);
    })?;
    simulator.tick(clock)?;
    for signal in ["read_data_a", "read_data_b"] {
        let observed = simulator
            .get(simulator.signal(signal))
            .to_u64_digits()
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
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )?;
    let block_rams = mapped
        .cells()
        .iter()
        .filter(|cell| matches!(cell, Ecp5Cell::BlockRam { .. }))
        .count();
    if block_rams != 8 {
        return Err(
            format!("register file mapped to {block_rams} DP16KD cells; expected eight").into(),
        );
    }
    println!(
        "Register file @ 125 MHz: dual-read behavior passed; 2 logical memories -> {block_rams} DP16KD cells, data={} -> {} ps, overall={} -> {} ps",
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
    let project_options = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "--project").then_some(index))
        .collect::<Vec<_>>();
    if project_options.len() > 1 {
        return Err("--project may only be supplied once".into());
    }
    let project_option = project_options.first().copied();
    let project_override = project_option
        .map(|index| {
            arguments
                .get(index + 1)
                .filter(|path| !path.starts_with("--"))
                .map(Path::new)
                .ok_or("--project requires a project directory")
        })
        .transpose()?;
    let rtl_only = arguments.iter().any(|argument| argument == "--rtl-only");
    let configs = arguments
        .iter()
        .filter_map(|arg| verify_riscv::predictor_option(arg))
        .collect::<std::collections::BTreeSet<_>>();
    if configs.len() > 1 {
        return Err("select only one predictor configuration".into());
    }
    let predictor_config = configs
        .first()
        .copied()
        .unwrap_or(verify_riscv::DEFAULT_PREDICTOR);
    let static_prediction = predictor_config == 4;
    let cache_option = arguments.iter().position(|arg| arg == "--icache-kib");
    let icache_index_bits = if let Some(index) = cache_option {
        match arguments.get(index + 1).map(String::as_str) {
            Some("0") => 0,
            Some("2") => 7,
            Some("4") => 8,
            Some("8") => 9,
            _ => return Err("--icache-kib requires 0, 2, 4 or 8".into()),
        }
    } else {
        0
    };
    if cache_option.is_some() && !arguments.iter().any(|arg| arg == "--hardware-memory-rtl") {
        return Err("--icache-kib requires --hardware-memory-rtl".into());
    }
    let enable_zba = arguments.iter().any(|arg| arg == "--zba-extension");
    let enable_zbb = arguments.iter().any(|arg| arg == "--zbb-extension");
    let enable_zbs = arguments.iter().any(|arg| arg == "--zbs-extension");
    let early_bitmanip = arguments.iter().any(|arg| arg == "--early-bitmanip");
    let early_taken_fetch = arguments.iter().any(|arg| arg == "--early-taken-fetch");
    let early_divide = arguments.iter().any(|arg| arg == "--early-divide");
    let machine_interrupts = arguments.iter().any(|arg| arg == "--machine-interrupts");
    let strict_memory = arguments.iter().any(|arg| arg == "--strict-memory");
    let enable_sv39 = arguments.iter().any(|arg| arg == "--sv39");
    let enable_s = enable_sv39 || arguments.iter().any(|arg| arg == "--s-mode");
    let enable_u = enable_s || arguments.iter().any(|arg| arg == "--u-mode");
    if enable_u && !strict_memory {
        return Err("--u-mode and --s-mode require --strict-memory".into());
    }
    let enable_a = arguments.iter().any(|arg| arg == "--a-extension");
    if enable_a && !strict_memory {
        return Err("--a-extension requires --strict-memory and --hardware-memory-rtl".into());
    }
    let kernel_console = arguments.iter().any(|arg| arg == "--kernel-console");
    let machine_console = kernel_console || arguments.iter().any(|arg| arg == "--machine-console");
    if machine_console && !machine_interrupts {
        return Err("console options require --machine-interrupts".into());
    }
    let timed_clocks = arguments.iter().any(|arg| arg == "--timed-clocks");
    let use_dsp = arguments.iter().any(|arg| arg == "--dsp-multiplier");
    let enable_m = use_dsp || arguments.iter().any(|arg| arg == "--m-extension");
    if early_divide && !enable_m {
        return Err("--early-divide requires --m-extension or --dsp-multiplier".into());
    }
    if (enable_m
        || enable_zba
        || enable_zbb
        || enable_zbs
        || early_bitmanip
        || early_taken_fetch
        || early_divide
        || machine_interrupts
        || strict_memory
        || timed_clocks)
        && !arguments.iter().any(|arg| arg == "--hardware-memory-rtl")
    {
        return Err(
            "ISA/execution options and --timed-clocks require --hardware-memory-rtl".into(),
        );
    }
    let cycle_option = arguments.iter().position(|arg| arg == "--max-cycles");
    let max_cycles = if let Some(index) = cycle_option {
        let count = arguments
            .get(index + 1)
            .ok_or("--max-cycles requires a value")?
            .parse::<usize>()?;
        if count == 0 {
            return Err("--max-cycles must be positive".into());
        }
        count
    } else {
        100_000
    };
    if cycle_option.is_some() && !arguments.iter().any(|arg| arg == "--hardware-memory-rtl") {
        return Err("--max-cycles requires --hardware-memory-rtl".into());
    }
    let trace_option = arguments.iter().position(|arg| arg == "--timed-trace");
    let trace_start_option = arguments.iter().position(|arg| arg == "--trace-start");
    let trace_end_option = arguments.iter().position(|arg| arg == "--trace-end");
    if (trace_start_option.is_some() || trace_end_option.is_some()) && trace_option.is_none() {
        return Err("--trace-start and --trace-end require --timed-trace".into());
    }
    let timed_trace = if let Some(index) = trace_option {
        if !timed_clocks {
            return Err("--timed-trace requires --timed-clocks".into());
        }
        let path = Path::new(
            arguments
                .get(index + 1)
                .ok_or("--timed-trace requires a path")?,
        );
        let parse_cycle = |option: Option<usize>, default| -> Result<usize, Box<dyn Error>> {
            Ok(match option {
                Some(index) => arguments
                    .get(index + 1)
                    .ok_or("trace range requires a cycle")?
                    .parse()?,
                None => default,
            })
        };
        Some(verify_riscv::timed_memory::TraceConfig {
            path,
            start_cycle: parse_cycle(trace_start_option, 1)?,
            end_cycle: parse_cycle(trace_end_option, max_cycles)?,
        })
    } else {
        None
    };
    let lsu_option = arguments.iter().position(|arg| arg == "--fast-lsu");
    let fast_lsu = if let Some(index) = lsu_option {
        match arguments.get(index + 1).map(String::as_str) {
            Some("0") => 0,
            Some("1") => 1,
            Some("2") => 2,
            Some("3") => 3,
            Some("4") => 4,
            Some("5") => 5,
            Some("6") => 6,
            _ => {
                return Err(
                    "--fast-lsu requires 0 (original), 1 (store/forwarding), 2 (store only), 3 (direct load), 4 (CPU-registered load), 5 (read prefetch), or 6 (prefetched load with CPU return register)"
                        .into(),
                );
            }
        }
    } else {
        0
    };
    if lsu_option.is_some() && !arguments.iter().any(|arg| arg == "--hardware-memory-rtl") {
        return Err("--fast-lsu requires --hardware-memory-rtl".into());
    }
    let map_only = arguments.iter().any(|argument| argument == "--map-only");
    let post_map = arguments.iter().any(|argument| argument == "--post-map");
    let register_file_only = arguments
        .iter()
        .any(|argument| argument == "--regfile-only");
    let project =
        project_override.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap());
    if arguments
        .iter()
        .any(|argument| argument == "--hardware-memory-rtl")
    {
        let requested_elfs = arguments
            .iter()
            .enumerate()
            .skip_while(|(_, argument)| *argument != "--hardware-memory-rtl")
            .skip(1)
            .filter(|(index, argument)| {
                verify_riscv::predictor_option(argument).is_none()
                    && !project_option
                        .is_some_and(|option| *index == option || *index == option + 1)
                    && !cache_option.is_some_and(|option| *index == option || *index == option + 1)
                    && argument.as_str() != "--m-extension"
                    && argument.as_str() != "--zba-extension"
                    && argument.as_str() != "--zbb-extension"
                    && argument.as_str() != "--zbs-extension"
                    && argument.as_str() != "--timed-clocks"
                    && argument.as_str() != "--early-bitmanip"
                    && argument.as_str() != "--early-taken-fetch"
                    && argument.as_str() != "--early-divide"
                    && argument.as_str() != "--machine-interrupts"
                    && argument.as_str() != "--strict-memory"
                    && argument.as_str() != "--a-extension"
                    && argument.as_str() != "--u-mode"
                    && argument.as_str() != "--s-mode"
                    && argument.as_str() != "--sv39"
                    && argument.as_str() != "--kernel-console"
                    && argument.as_str() != "--machine-console"
                    && argument.as_str() != "--dsp-multiplier"
                    && !cycle_option.is_some_and(|option| *index == option || *index == option + 1)
                    && !lsu_option.is_some_and(|option| *index == option || *index == option + 1)
                    && ![trace_option, trace_start_option, trace_end_option]
                        .into_iter()
                        .flatten()
                        .any(|option| *index == option || *index == option + 1)
            })
            .map(|(_, argument)| PathBuf::from(argument))
            .collect::<Vec<_>>();
        if timed_clocks {
            if requested_elfs.is_empty() {
                return Err("--timed-clocks requires explicit ELF paths".into());
            }
            if timed_trace.is_some() && requested_elfs.len() != 1 {
                return Err("--timed-trace requires exactly one ELF".into());
            }
            for elf in &requested_elfs {
                verify_riscv::timed_memory::run(
                    project,
                    elf,
                    predictor_config,
                    (icache_index_bits, fast_lsu),
                    verify_riscv::IsaExtensions {
                        m: enable_m,
                        zba: enable_zba,
                        zbb: enable_zbb,
                        zbs: enable_zbs,
                    },
                    verify_riscv::timed_memory::ExecutionConfig {
                        use_dsp,
                        early_bitmanip,
                        early_taken_fetch,
                        early_divide,
                        machine_interrupts,
                        strict_memory,
                        enable_a,
                        enable_u,
                        enable_s,
                        enable_sv39,
                        machine_console,
                        kernel_console,
                        trace: timed_trace,
                    },
                    max_cycles,
                )?;
            }
            return Ok(());
        }
        return verify_hardware_memory_rtl(
            project,
            &requested_elfs,
            HardwareModelConfig {
                predictor_config,
                icache_index_bits,
                fast_lsu,
                enable_m,
                use_dsp,
                enable_zba,
                enable_zbb,
                enable_zbs,
                early_bitmanip,
                early_taken_fetch,
                early_divide,
                machine_interrupts,
                strict_memory,
                enable_a,
                enable_u,
                enable_s,
                enable_sv39,
                machine_console,
                kernel_console,
            },
            max_cycles,
        );
    }
    if predictor_config != verify_riscv::DEFAULT_PREDICTOR && !rtl_only {
        return Err("predictor selection requires --rtl-only or --hardware-memory-rtl; use nextpnr-riscv HardwareStaticTestTop for mapping".into());
    }
    verify_direct_jtag_transport(project)?;
    if arguments.iter().any(|argument| argument == "--jtag-only") {
        return Ok(());
    }
    verify_register_file(project)?;
    if register_file_only {
        return Ok(());
    }
    let core_source = verify_riscv::core_source(project)?;

    let mut rtl = SimulatorBuilder::new(&core_source, "Core")
        .param("BRANCH_PREDICTOR_CONFIG", predictor_config)
        .build_native()?;
    verify_native(
        &mut rtl,
        "Celox RTL native",
        if static_prediction { Some(105) } else { None },
    )?;
    let mut board_rtl = SimulatorBuilder::new(&core_source, "Core")
        .param("BRANCH_PREDICTOR_CONFIG", predictor_config)
        .build_native()?;
    verify_board_diagnostic(&mut board_rtl)?;
    if rtl_only {
        return Ok(());
    }

    let lower_started = Instant::now();
    let design = analyze_and_lower(&core_source, "rica_riscv", "Core")?;
    println!("Struo lower completed in {:?}", lower_started.elapsed());
    let synth_started = Instant::now();
    let synthesized = synthesize(&design)?;
    println!("Struo synthesis completed in {:?}", synth_started.elapsed());
    let map_started = Instant::now();
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )?;
    let map_elapsed = map_started.elapsed();
    if !mapped.retiming().equivalence_signed_off {
        return Err("Struo mapping equivalence was not signed off".into());
    }

    println!(
        "Struo ECP5 map @ 125 MHz: {} nodes, {} registers, {} cells, data={} -> {} ps, overall={} -> {} ps, LUT depth={} -> {}, critical registers={} -> {}, retimed={}, map={map_elapsed:?}",
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

    let post_map_frontend_started = Instant::now();
    let post_map_builder = ecp5_simulator(&mapped)?;
    println!(
        "Celox post-map frontend completed in {:?}",
        post_map_frontend_started.elapsed()
    );
    let post_map_native_started = Instant::now();
    let mut post_map = post_map_builder.build_native()?;
    println!(
        "Celox post-map native build completed in {:?}",
        post_map_native_started.elapsed()
    );
    verify_post_map(&mut post_map, "Celox post-map simulation", Some(126))?;
    println!(
        "Struo: {} nodes, {} registers, {} ECP5 cells; equivalence passed",
        synthesized.netlist.nodes().len(),
        synthesized.netlist.registers().len(),
        mapped.cells().len()
    );
    Ok(())
}
