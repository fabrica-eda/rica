//! Keep the loader's registered byte and address together on consecutive edges.
use celox::{SimBackend, Simulator};

pub(crate) fn check_stream<B: SimBackend>(mut sim: Simulator<B>) -> Result<(), String> {
    let clk = sim.event("clk");
    let rst = sim.signal("rst_n");
    let we = sim.signal("debug_write");
    let address = sim.signal("debug_address");
    let data = sim.signal("debug_data");
    let instr_address = sim.signal("cpu_instr_address");
    let data_address = sim.signal("cpu_data_address");
    let data_sequence = sim.signal("cpu_data_request_sequence");
    let data_write = sim.signal("cpu_data_write");
    let data_mask = sim.signal("cpu_data_mask");
    let data_we = sim.signal("cpu_data_we");
    let instr_sequence = sim.signal("cpu_instr_request_sequence");
    let read_data = sim.signal("cpu_data_read");
    let instr_line = sim.signal("cpu_instr_line");
    sim.modify(|io| {
        io.set(rst, 0_u8);
        io.set(we, 0_u8);
        io.set(address, 0_u32);
        io.set(data, 0_u8);
        io.set(instr_address, 0_u32);
        io.set(data_address, 0_u32);
        io.set(data_sequence, 0_u8);
        io.set(data_write, 0_u64);
        io.set(data_mask, 0_u8);
        io.set(data_we, 0_u8);
        io.set(instr_sequence, 0_u8);
    })
    .map_err(|e| e.to_string())?;
    sim.tick(clk).map_err(|e| e.to_string())?;
    sim.modify(|io| io.set(rst, 1_u8))
        .map_err(|e| e.to_string())?;
    let mut sequence = 0_u8;
    for base in [0_u32, 0x8000_0000, 0x8001_ff00] {
        for byte in 0..32 {
            sim.modify(|io| {
                io.set(address, base + byte);
                io.set(data, (byte * 7 + 13) as u8);
                io.set(we, 1_u8);
            })
            .map_err(|e| e.to_string())?;
            sim.tick(clk).map_err(|e| e.to_string())?;
        }
        sim.modify(|io| io.set(we, 0_u8))
            .map_err(|e| e.to_string())?;
        sim.tick(clk).map_err(|e| e.to_string())?;
        sim.tick(clk).map_err(|e| e.to_string())?;
        for offset in (0..32).step_by(8) {
            let physical = if base == 0 { 0x3_f000 } else { base & 0x3_ffff };
            sequence ^= 1;
            sim.modify(|io| {
                io.set(instr_address, base + offset);
                io.set(data_address, physical + offset);
                io.set(data_sequence, sequence);
            })
            .map_err(|e| e.to_string())?;
            for _ in 0..5 {
                sim.tick(clk).map_err(|e| e.to_string())?;
            }
            let want = u64::from_le_bytes(std::array::from_fn(|byte| {
                ((offset + byte as u32) * 7 + 13) as u8
            }));
            for (port, signal) in [("data", read_data), ("instruction", instr_line)] {
                let got = sim
                    .get(signal)
                    .to_u64_digits()
                    .first()
                    .copied()
                    .unwrap_or(0);
                if got != want {
                    return Err(format!(
                        "{port} base={base:#x} offset={offset}: got={got:#018x}, expected={want:#018x}"
                    ));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn consecutive_debug_bytes_survive_both_bram_ports() {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let source = std::fs::read_to_string(project.join("src/HardwareMemory.veryl")).unwrap();
    let parameterized = source.replace(
        "module HardwareMemory (",
        "module HardwareMemory #(param FAST_LSU: u32 = 0,) (",
    );
    assert_ne!(source, parameterized);
    for (label, source) in [("original", source), ("unused-parameter", parameterized)] {
        for level in [
            celox::OptLevel::O0,
            celox::OptLevel::O1,
            celox::OptLevel::O2,
        ] {
            let build = || celox::SimulatorBuilder::new(&source, "HardwareMemory").opt_level(level);
            check_stream(build().build_native().unwrap())
                .unwrap_or_else(|error| panic!("native {label} {level:?}: {error}"));
            check_stream(build().build_wasm().unwrap())
                .unwrap_or_else(|error| panic!("wasm {label} {level:?}: {error}"));
        }
    }
}
