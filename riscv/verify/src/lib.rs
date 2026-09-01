use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fs;
use std::path::Path;

use celox::{NativeBackend, Simulator, SimulatorBuilder};
use object::{Architecture, Object, ObjectSegment, ObjectSymbol};

const CORE_SOURCES: [&str; 12] = [
    "PipelineInterfaces.veryl",
    "RegisterFile.veryl",
    "CompressedDecoder.veryl",
    "Decoder.veryl",
    "PerformanceCounter64.veryl",
    "PerformanceEventCounters4.veryl",
    "CsrFile.veryl",
    "ExecuteUnit.veryl",
    "ShiftUnit.veryl",
    "ShiftHighUnit.veryl",
    "CompletionUnit.veryl",
    "Core.veryl",
];

fn reset_trampoline(entry: u64) -> Result<Vec<u32>, String> {
    if entry < 0x7fff_f800 {
        let entry = entry as u32;
        let upper = entry.wrapping_add(0x800) & 0xffff_f000;
        let lower = entry.wrapping_sub(upper) & 0xfff;
        return Ok(vec![
            upper | (5 << 7) | 0x37,
            (lower << 20) | (5 << 15) | 0x67,
        ]);
    }
    if entry == 0x8000_0000 {
        return Ok(vec![
            0x4000_02b7, // lui t0, 0x40000
            0x0052_82b3, // add t0, t0, t0
            0x0002_8067, // jalr x0, 0(t0)
        ]);
    }
    Err(format!(
        "entry 0x{entry:016x} is outside the reset trampoline's implemented address range"
    ))
}

pub fn core_source(project: &Path) -> Result<String, Box<dyn Error>> {
    Ok(CORE_SOURCES
        .into_iter()
        .map(|file| fs::read_to_string(project.join("src").join(file)))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n"))
}

#[derive(Clone)]
pub struct ElfImage {
    memory: BTreeMap<u64, u8>,
    pub tohost: u64,
}

impl ElfImage {
    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let bytes = fs::read(path)?;
        let elf = object::File::parse(bytes.as_slice())?;
        if elf.architecture() != Architecture::Riscv64 {
            return Err(format!("{} is not an RV64 ELF", path.display()).into());
        }
        if !elf.is_little_endian() {
            return Err(format!("{} is not little-endian", path.display()).into());
        }
        let mut memory = BTreeMap::new();
        for segment in elf.segments() {
            let address = segment.address();
            let data = segment.data()?;
            let end = address
                .checked_add(u64::try_from(data.len())?)
                .ok_or("ELF segment address overflow")?;
            if end > 0x1_0000_0000 {
                return Err(format!(
                    "{} has a loadable segment outside Rica's 32-bit physical address space",
                    path.display()
                )
                .into());
            }
            for (offset, byte) in data.iter().copied().enumerate() {
                memory.insert(
                    address
                        .checked_add(u64::try_from(offset)?)
                        .ok_or("ELF segment address overflow")?,
                    byte,
                );
            }
        }
        let entry = elf.entry();
        if entry & 1 != 0 {
            return Err(format!(
                "{} has entry 0x{entry:016x}, which is not halfword-aligned",
                path.display()
            )
            .into());
        }
        if entry != 0 {
            let trampoline = reset_trampoline(entry)?;
            let trampoline_bytes = trampoline.len() * 4;
            if (0..u64::try_from(trampoline_bytes)?).any(|address| memory.contains_key(&address)) {
                return Err(format!(
                    "{} has entry 0x{entry:016x} but also occupies the reset trampoline at 0x00000000",
                    path.display()
                )
                .into());
            }
            for (offset, byte) in trampoline
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .enumerate()
            {
                memory.insert(offset as u64, byte);
            }
        }
        let tohost = elf
            .symbols()
            .find(|symbol| symbol.name().ok() == Some("tohost"))
            .ok_or_else(|| format!("{} has no tohost symbol", path.display()))
            .map(|symbol| symbol.address())?;
        if tohost > u64::from(u32::MAX) {
            return Err(format!(
                "{} places tohost outside Rica's 32-bit physical address space",
                path.display()
            )
            .into());
        }
        Ok(Self { memory, tohost })
    }

    fn read_doubleword(&self, address: u64) -> u64 {
        u64::from_le_bytes(std::array::from_fn(|offset| {
            self.memory
                .get(&address.wrapping_add(offset as u64))
                .copied()
                .unwrap_or(0)
        }))
    }

    fn read_instruction(&self, address: u64) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|offset| {
            self.memory
                .get(&address.wrapping_add(offset as u64))
                .copied()
                .unwrap_or(0)
        }))
    }

    fn write_masked(&mut self, address: u64, value: u64, mask: u8) {
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
            if mask & (1 << offset) != 0 {
                self.memory
                    .insert(address.wrapping_add(offset as u64), byte);
            }
        }
    }

    pub fn load_bytes(&self) -> impl Iterator<Item = (u64, u8)> + '_ {
        self.memory.iter().map(|(address, byte)| (*address, *byte))
    }
}

#[derive(Clone, Copy)]
struct Commit {
    pc: u64,
    instruction: u32,
    rd: u32,
    data: u64,
}

fn value(simulator: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    simulator
        .get(simulator.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

pub struct ElfRun {
    pub cycles: usize,
    pub commits: usize,
}

pub fn run_elf(
    core_source: &str,
    mut image: ElfImage,
    max_cycles: usize,
) -> Result<ElfRun, Box<dyn Error>> {
    let mut simulator = SimulatorBuilder::new(core_source, "Core").build_native()?;
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

    let mut commits = 0;
    let mut trace = VecDeque::with_capacity(16);
    for cycle in 1..=max_cycles {
        let fetch_pc = value(&mut simulator, "pc");
        let data_address = value(&mut simulator, "data_addr");
        let store = (value(&mut simulator, "data_we") != 0).then(|| {
            (
                data_address,
                value(&mut simulator, "data_wdata"),
                value(&mut simulator, "data_wmask") as u8,
            )
        });
        let fetched_instruction = image.read_instruction(fetch_pc);
        let memory_read_data = image.read_doubleword(data_address);
        simulator.modify(|io| {
            io.set(instruction, fetched_instruction);
            io.set(
                instruction_compressed,
                u8::from(fetched_instruction & 0b11 != 0b11),
            );
            io.set(data_rdata, memory_read_data);
        })?;
        simulator.tick(clock)?;

        if let Some((address, data, mask)) = store {
            let alignment_mask = match mask {
                0x01 => 0,
                0x03 => 1,
                0x0f => 3,
                0xff => 7,
                _ => {
                    return Err(
                        format!("unsupported store mask 0x{mask:02x} at cycle {cycle}").into(),
                    );
                }
            };
            if address & alignment_mask != 0 {
                return Err(format!(
                    "unaligned store [0x{address:016x}]/0x{mask:02x} = 0x{data:016x} at cycle {cycle}"
                )
                .into());
            }
            image.write_masked(address, data, mask);
            // ACT4's Sail harness uses the same 64-bit `tohost` mailbox for
            // both console output and termination. It updates the low word
            // first and commits the message by writing the high word: a zero
            // high word is pass/fail, while 0x01010000 is a console byte.
            // Do not mistake that transient low-word character for failure.
            let mailbox_committed = (address == image.tohost && mask == 0xff)
                || (address == image.tohost.wrapping_add(4) && mask == 0x0f);
            let tohost = image.read_doubleword(image.tohost);
            if mailbox_committed && tohost != 0 && (tohost >> 32) == 0 {
                if tohost == 1 {
                    return Ok(ElfRun {
                        cycles: cycle,
                        commits,
                    });
                }
                return Err(format!(
                    "self-check failed with tohost=0x{tohost:016x} (test {}) at cycle {cycle}\n{}",
                    tohost >> 1,
                    format_trace(&trace)
                )
                .into());
            }
        }

        if value(&mut simulator, "commit_valid") != 0 {
            commits += 1;
            if trace.len() == trace.capacity() {
                trace.pop_front();
            }
            trace.push_back(Commit {
                pc: value(&mut simulator, "commit_pc"),
                instruction: value(&mut simulator, "commit_instr") as u32,
                rd: value(&mut simulator, "commit_rd") as u32,
                data: value(&mut simulator, "commit_data"),
            });
        }
        if value(&mut simulator, "halted") != 0 {
            return Err(format!(
                "core halted before reporting tohost at cycle {cycle}\n{}",
                format_trace(&trace)
            )
            .into());
        }
    }
    Err(format!(
        "timeout after {max_cycles} cycles without a tohost result\n{}",
        format_trace(&trace)
    )
    .into())
}

fn format_trace(trace: &VecDeque<Commit>) -> String {
    let mut output = String::from("recent commits:");
    for commit in trace {
        output.push_str(&format!(
            "\n  pc=0x{:016x} instr=0x{:08x} rd=x{} data=0x{:016x}",
            commit.pc, commit.instruction, commit.rd, commit.data
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::reset_trampoline;

    #[test]
    fn reset_trampoline_covers_local_and_act4_entry_points() {
        let local = reset_trampoline(0x4000).unwrap();
        assert_eq!(local, [0x0000_42b7, 0x0002_8067]);

        let act4 = reset_trampoline(0x8000_0000).unwrap();
        assert_eq!(act4, [0x4000_02b7, 0x0052_82b3, 0x0002_8067]);
        assert!(reset_trampoline(0x1_0000_0000).is_err());
    }
}
