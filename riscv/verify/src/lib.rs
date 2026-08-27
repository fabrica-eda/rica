use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fs;
use std::path::Path;

use celox::{Simulator, SimulatorBuilder};
use object::{Architecture, Object, ObjectSegment, ObjectSymbol};

const CORE_SOURCES: [&str; 5] = [
    "PipelineInterfaces.veryl",
    "RegisterFile.veryl",
    "Decoder.veryl",
    "ExecuteUnit.veryl",
    "Core.veryl",
];

fn reset_trampoline(entry: u32) -> [u32; 2] {
    let upper = entry.wrapping_add(0x800) & 0xffff_f000;
    let lower = entry.wrapping_sub(upper) & 0xfff;
    let lui_t0 = upper | (5 << 7) | 0x37;
    let jalr_t0 = (lower << 20) | (5 << 15) | 0x67;
    [lui_t0, jalr_t0]
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
    memory: BTreeMap<u32, u8>,
    pub tohost: u32,
}

impl ElfImage {
    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let bytes = fs::read(path)?;
        let elf = object::File::parse(bytes.as_slice())?;
        if elf.architecture() != Architecture::Riscv32 {
            return Err(format!("{} is not an RV32 ELF", path.display()).into());
        }
        if !elf.is_little_endian() {
            return Err(format!("{} is not little-endian", path.display()).into());
        }
        let mut memory = BTreeMap::new();
        for segment in elf.segments() {
            let address = u32::try_from(segment.address())?;
            for (offset, byte) in segment.data()?.iter().copied().enumerate() {
                memory.insert(
                    address
                        .checked_add(u32::try_from(offset)?)
                        .ok_or("ELF segment address overflow")?,
                    byte,
                );
            }
        }
        let entry = u32::try_from(elf.entry())?;
        if entry & 3 != 0 {
            return Err(format!(
                "{} has entry 0x{entry:08x}, which is not word-aligned",
                path.display()
            )
            .into());
        }
        if entry != 0 {
            if (0..8).any(|address| memory.contains_key(&address)) {
                return Err(format!(
                    "{} has entry 0x{entry:08x} but also occupies the reset trampoline at 0x00000000",
                    path.display()
                )
                .into());
            }
            for (offset, byte) in reset_trampoline(entry)
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .enumerate()
            {
                memory.insert(offset as u32, byte);
            }
        }
        let tohost = elf
            .symbols()
            .find(|symbol| symbol.name().ok() == Some("tohost"))
            .ok_or_else(|| format!("{} has no tohost symbol", path.display()))
            .and_then(|symbol| {
                u32::try_from(symbol.address()).map_err(|error| error.to_string())
            })?;
        Ok(Self { memory, tohost })
    }

    fn read_word(&self, address: u32) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|offset| {
            self.memory
                .get(&address.wrapping_add(offset as u32))
                .copied()
                .unwrap_or(0)
        }))
    }

    fn write_word(&mut self, address: u32, value: u32) {
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
            self.memory
                .insert(address.wrapping_add(offset as u32), byte);
        }
    }
}

#[derive(Clone, Copy)]
struct Commit {
    pc: u32,
    instruction: u32,
    rd: u32,
    data: u32,
}

fn value(simulator: &mut Simulator, name: &str) -> u32 {
    simulator
        .get(simulator.signal(name))
        .to_u32_digits()
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
    let data_rdata = simulator.signal("data_rdata");
    simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(instruction, 0x0000_0013_u32);
        io.set(data_rdata, 0_u32);
    })?;
    simulator.tick(clock)?;
    simulator.modify(|io| io.set(reset, 1_u8))?;

    let mut commits = 0;
    let mut trace = VecDeque::with_capacity(16);
    for cycle in 1..=max_cycles {
        let fetch_pc = value(&mut simulator, "pc");
        let data_address = value(&mut simulator, "data_addr");
        let store = (value(&mut simulator, "data_we") != 0)
            .then(|| (data_address, value(&mut simulator, "data_wdata")));
        let fetched_instruction = image.read_word(fetch_pc);
        let memory_read_data = image.read_word(data_address);
        simulator.modify(|io| {
            io.set(instruction, fetched_instruction);
            io.set(data_rdata, memory_read_data);
        })?;
        simulator.tick(clock)?;

        if let Some((address, data)) = store {
            if address & 3 != 0 {
                return Err(format!(
                    "unaligned word store [0x{address:08x}] = 0x{data:08x} at cycle {cycle}"
                )
                .into());
            }
            image.write_word(address, data);
            if address == image.tohost && data != 0 {
                if data == 1 {
                    return Ok(ElfRun {
                        cycles: cycle,
                        commits,
                    });
                }
                return Err(format!(
                    "self-check failed with tohost=0x{data:08x} (test {}) at cycle {cycle}\n{}",
                    data >> 1,
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
                instruction: value(&mut simulator, "commit_instr"),
                rd: value(&mut simulator, "commit_rd"),
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
            "\n  pc=0x{:08x} instr=0x{:08x} rd=x{} data=0x{:08x}",
            commit.pc, commit.instruction, commit.rd, commit.data
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::reset_trampoline;

    #[test]
    fn reset_trampoline_reaches_full_address_space() {
        for entry in [0x0000_4000_u32, 0x7fff_fffc, 0x8000_0000, 0xffff_f000] {
            let [lui, jalr] = reset_trampoline(entry);
            let upper = lui & 0xffff_f000;
            let lower = ((jalr as i32) >> 20) as u32;
            assert_eq!(upper.wrapping_add(lower) & !1, entry);
            assert_eq!(lui & 0x0000_0fff, (5 << 7) | 0x37);
            assert_eq!(jalr & 0x000f_ffff, (5 << 15) | 0x67);
        }
    }
}
