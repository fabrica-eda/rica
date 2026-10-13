//! Linux bring-up using the actual Core RTL with host-backed RAM and MMIO.
use celox::SimulatorBuilder;
use object::{Architecture, Object, ObjectSegment};
use std::{
    collections::VecDeque,
    error::Error,
    fs,
    io::{self, Write},
    path::Path,
    time::Instant,
};
const BASE: u64 = 0x8000_0000;
const END: u64 = 0x8800_0000;
const KERNEL: u64 = 0x8020_0000;
const INITRD: u64 = 0x8600_0000;
const DTB: u64 = 0x87e0_0000;
fn byte(ram: &[u8], rom: &[u8], addr: u64) -> u8 {
    if (BASE..END).contains(&addr) {
        ram[(addr - BASE) as usize]
    } else {
        rom.get(addr as usize).copied().unwrap_or(0)
    }
}
fn load(ram: &mut [u8], addr: u64, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let offset = addr.checked_sub(BASE).ok_or("image below RAM")? as usize;
    ram.get_mut(offset..offset.checked_add(data.len()).ok_or("image overflow")?)
        .ok_or("image outside RAM")?
        .copy_from_slice(data);
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(5..=6).contains(&args.len()) {
        return Err(
            "usage: linux-sim Image firmware.elf platform.dtb initramfs.cpio [max-cycles]".into(),
        );
    }
    let limit = args
        .get(5)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(500_000_000);
    let trace_every = std::env::var("LINUX_SIM_TRACE_EVERY")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(0);
    let trace_after_user = std::env::var_os("LINUX_SIM_TRACE_AFTER_USER").is_some();
    let cycles_per_tick = std::env::var("LINUX_SIM_TIMER_DIVISOR")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(100);
    if cycles_per_tick == 0 {
        return Err("timer divisor must be positive".into());
    }
    let mut ram = vec![0u8; (END - BASE) as usize];
    let kernel = fs::read(&args[1])?;
    if kernel.len() < 64 || kernel.get(48..53) != Some(b"RISCV") {
        return Err("not a RISC-V Linux Image".into());
    }
    let offset = u64::from_le_bytes(kernel[8..16].try_into()?);
    let size = u64::from_le_bytes(kernel[16..24].try_into()?);
    let flags = u64::from_le_bytes(kernel[24..32].try_into()?);
    if offset != KERNEL - BASE
        || flags & 1 != 0
        || size == 0
        || size.max(kernel.len() as u64) > INITRD - KERNEL
    {
        return Err("Image must be little-endian, have a 2 MiB offset and fit below initrd".into());
    }
    load(&mut ram, KERNEL, &kernel)?;
    let fw = fs::read(&args[2])?;
    let elf = object::File::parse(fw.as_slice())?;
    if elf.architecture() != Architecture::Riscv64 || !elf.is_little_endian() || elf.entry() != BASE
    {
        return Err("firmware must be little-endian RV64 ELF with entry 0x80000000".into());
    }
    for segment in elf.segments() {
        if segment.address() < BASE
            || segment
                .address()
                .checked_add(segment.size())
                .is_none_or(|end| end > KERNEL)
        {
            return Err("firmware segment outside reserved 2 MiB region".into());
        }
        load(&mut ram, segment.address(), segment.data()?)?;
    }
    let dtb = fs::read(&args[3])?;
    if dtb.get(..4) != Some(&[0xd0, 0x0d, 0xfe, 0xed]) {
        return Err("invalid flattened device tree magic".into());
    }
    load(&mut ram, DTB, &dtb)?;
    let initrd = fs::read(&args[4])?;
    if initrd.len() as u64 > DTB - INITRD {
        return Err("initrd overlaps device tree".into());
    }
    load(&mut ram, INITRD, &initrd)?;
    let mut rom = vec![0; 4096];
    for (i, v) in [0x400002b7u32, 0x005282b3, 0x00028067].iter().enumerate() {
        rom[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    eprintln!("Compiling Rica Core RTL, RAM={:#x}..{:#x}", BASE, END);
    let source = verify_riscv::core_source(project)?;
    let mut sim = SimulatorBuilder::new(&source, "Core")
        .param("ENABLE_M", 1u64)
        .param("ENABLE_A", 1u64)
        .param("ENABLE_U", 1u64)
        .param("ENABLE_S", 1u64)
        .param("ENABLE_SV39", 1u64)
        .param("ENABLE_INTERRUPTS", 1u64)
        .param("STRICT_MEMORY", 1u64)
        .param("SYNC_DATA_MEMORY", 1u64)
        .param("RAM_END", END)
        .param("BRANCH_PREDICTOR_CONFIG", 8u64)
        .build_native()?;
    let clk = sim.event("clk");
    macro_rules! sig {
        ($n:literal) => {
            sim.signal($n)
        };
    }
    let rst = sig!("rst_n");
    let instr = sig!("instr");
    let compressed = sig!("instr_is_compressed");
    let valid = sig!("instr_valid");
    let rdata = sig!("data_rdata");
    let resp = sig!("data_response_sequence");
    let irq = sig!("machine_interrupts");
    let pc = sig!("pc");
    let translated = sig!("fetch_translate");
    let frame = sig!("fetch_frame");
    let next_frame = sig!("fetch_next_page");
    let addr = sig!("data_addr");
    let wdata = sig!("data_wdata");
    let mask = sig!("data_wmask");
    let we = sig!("data_we");
    let seq = sig!("data_request_sequence");
    let cv = sig!("commit_valid");
    let cp = sig!("commit_pc");
    let ci = sig!("commit_instr");
    let cr = sig!("commit_rd");
    let cd = sig!("commit_data");
    sim.modify(|io| {
        io.set(rst, 0u8);
        io.set(valid, 1u8);
        io.set(instr, 0x13u32);
        io.set(resp, 0u8);
        io.set(irq, 0u8);
    })?;
    sim.tick(clk)?;
    sim.modify(|io| io.set(rst, 1u8))?;
    let start = Instant::now();
    let mut ack = 0u8;
    let mut memory_data = 0u64;
    let mut compare = u64::MAX;
    let mut trace = VecDeque::with_capacity(24);
    let mut console = String::new();
    let mut commits = 0u64;
    let mut registers = [0u64; 32];
    let mut user_trace_count = 0;
    for cycle in 0..limit {
        let virtual_pc = sim.get_as::<u64>(pc);
        let translate = sim.get_as::<u8>(translated) != 0;
        let physical = if translate {
            (u64::from(sim.get_as::<u32>(frame)) << 12) | (virtual_pc & 4095)
        } else {
            virtual_pc
        };
        let mut insn = [0; 4];
        for (i, b) in insn.iter_mut().enumerate() {
            let a = if translate && (virtual_pc & 4095) + i as u64 >= 4096 {
                (u64::from(sim.get_as::<u32>(next_frame)) << 12) | ((virtual_pc + i as u64) & 4095)
            } else {
                physical + i as u64
            };
            *b = byte(&ram, &rom, a);
        }
        let instruction = u32::from_le_bytes(insn);
        let request = sim.get_as::<u8>(seq);
        let timer = cycle / cycles_per_tick;
        if request != ack {
            let address = sim.get_as::<u64>(addr);
            let write = sim.get_as::<u8>(we) != 0;
            let data = sim.get_as::<u64>(wdata);
            let lanes = sim.get_as::<u8>(mask);
            if write {
                match address {
                    0x02001000 => {
                        let c = data as u8;
                        print!("{}", c as char);
                        io::stdout().flush()?;
                        console.push(c as char);
                    }
                    0x02001008 => {
                        eprintln!(
                            "Firmware exit={data}; cycles={cycle}; commits={commits}; elapsed={:?}",
                            start.elapsed()
                        );
                        if data == 1 && console.contains("RICA_LINUX_USERSPACE_OK") {
                            return Ok(());
                        }
                        return Err(format!("guest exited without userspace success (code {data}); recent commits: {trace:x?}").into());
                    }
                    0x02004000 => compare = data,
                    _ => {
                        for (i, b) in data.to_le_bytes().into_iter().enumerate() {
                            if lanes & (1 << i) != 0 {
                                let a = address + i as u64;
                                if !(BASE..END).contains(&a) {
                                    return Err(
                                        format!("invalid store {a:#x} at cycle {cycle}").into()
                                    );
                                }
                                ram[(a - BASE) as usize] = b;
                            }
                        }
                    }
                }
            }
            memory_data = match address {
                0x0200bff8 => timer,
                0x02004000 => compare,
                0x0200bff0 => 1_000_000,
                _ => u64::from_le_bytes(std::array::from_fn(|i| {
                    byte(&ram, &rom, address + i as u64)
                })),
            };
            if trace_after_user && user_trace_count != 0 {
                eprintln!("memory: cycle={cycle} address={address:#x} write={write} data={data:#x} mask={lanes:#x} read={memory_data:#x}");
            }
            ack = request;
        }
        sim.modify(|io| {
            io.set(instr, instruction);
            io.set(compressed, u8::from(instruction & 3 != 3));
            io.set(rdata, memory_data);
            io.set(resp, ack);
            io.set(irq, if timer >= compare { 2u8 } else { 0u8 });
        })?;
        sim.tick(clk)?;
        if sim.get_as::<u8>(cv) != 0 {
            commits += 1;
            if trace.len() == 24 {
                trace.pop_front();
            }
            let rd = sim.get_as::<u8>(cr) as usize;
            if rd != 0 {
                registers[rd] = sim.get_as::<u64>(cd);
            }
            let committed_pc = sim.get_as::<u64>(cp);
            if trace_after_user && user_trace_count != 0 {
                eprintln!(
                    "retire: cycle={cycle} pc={committed_pc:#x} instr={:#x} rd={rd} data={:#x}",
                    sim.get_as::<u32>(ci),
                    sim.get_as::<u64>(cd)
                );
            }
            if (0x1000..BASE).contains(&committed_pc) && user_trace_count < 64 {
                if trace_after_user && user_trace_count == 0 {
                    eprintln!("initial user registers: {registers:x?}");
                }
                user_trace_count += 1;
                eprintln!(
                    "user commit: pc={committed_pc:#x} instr={:#x} rd={rd} data={:#x}",
                    sim.get_as::<u32>(ci),
                    sim.get_as::<u64>(cd)
                );
            }
            trace.push_back((
                sim.get_as::<u64>(cp),
                sim.get_as::<u32>(ci),
                sim.get_as::<u8>(cr),
                sim.get_as::<u64>(cd),
            ));
        }
        if trace_every != 0 && (cycle + 1) % trace_every == 0 {
            eprintln!(
                "registers at cycle {}: {registers:x?}\nrecent commits: {trace:x?}",
                cycle + 1
            );
        }
        if cycle % 1_000_000 == 999_999 {
            eprintln!(
                "cycles={} commits={commits} pc={virtual_pc:#x} elapsed={:?}",
                cycle + 1,
                start.elapsed()
            );
        }
    }
    Err(format!("cycle limit reached; recent commits: {trace:x?}").into())
}
