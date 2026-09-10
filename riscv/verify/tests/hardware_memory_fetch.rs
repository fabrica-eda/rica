//! Instruction responses must keep bank, permission, address and token aligned.
use celox::{SimBackend, Simulator, SimulatorBuilder};

fn check_responses<B: SimBackend>(mut sim: Simulator<B>, strict: bool) {
    let clk = sim.event("clk");
    let rst = sim.signal("rst_n");
    let timer_rst = sim.signal("timer_rst_n");
    let debug_write = sim.signal("debug_write");
    let debug_address = sim.signal("debug_address");
    let debug_data = sim.signal("debug_data");
    let address = sim.signal("cpu_instr_address");
    let sequence = sim.signal("cpu_instr_request_sequence");
    let response_address = sim.signal("cpu_instr_response_address");
    let response_sequence = sim.signal("cpu_instr_response_sequence");
    let response_valid = sim.signal("cpu_instr_response_valid");
    let response_line = sim.signal("cpu_instr_line");
    sim.modify(|io| {
        io.set(rst, 0_u8);
        io.set(timer_rst, 0_u8);
        io.set(debug_write, 0_u8);
        io.set(address, 0_u32);
        io.set(sequence, 0_u8);
    })
    .unwrap();
    sim.tick(clk).unwrap();
    sim.modify(|io| io.set(rst, 1_u8)).unwrap();

    let physical = |addr: u32| {
        // Legacy instruction fetches with bit 31 clear alias the boot page.
        // STRICT_MEMORY must mask those aliases while retaining legacy mode.
        if addr < 0x8000_0000 {
            0x3_f000 + (addr & 0xfff) as usize
        } else {
            (addr & 0x3_ffff) as usize
        }
    };
    let mut expected = vec![0_u8; 256 * 1024];
    for (index, base) in [
        0_u32,
        0xff8,
        0x8000_0000,
        0x8000_1000,
        0x8001_fff8,
        0x8002_0000,
        0x8003_eff8,
    ]
    .into_iter()
    .enumerate()
    {
        for byte in 0..8 {
            let value = (index * 31 + byte * 17 + 9) as u8;
            let addr = base + byte as u32;
            expected[physical(addr)] = value;
            sim.modify(|io| {
                io.set(debug_write, 1_u8);
                io.set(debug_address, addr);
                io.set(debug_data, value);
            })
            .unwrap();
            sim.tick(clk).unwrap();
        }
    }
    sim.modify(|io| io.set(debug_write, 0_u8)).unwrap();
    for _ in 0..3 {
        sim.tick(clk).unwrap();
    }

    let requests = [
        0_u32,
        0x8000_0005,
        0x8002_0007,
        0x8001_fffa,
        0x8003_f000, // Forbidden alias of the boot trampoline.
        0xffe,
        0x8003_effa,
        0x4000_0000, // Legacy alias of the boot page.
        0xc000_0000, // Forbidden alias of the low RAM bank.
        0x8006_0000, // Forbidden alias of the high RAM bank.
        0x0002_0000, // Another legacy alias of the boot page.
        0xffff_fffa,
        0x1000,
        0x8000_1000,
    ];
    let (mut previous, mut previous_sequence) = (0_u32, 0_u8);
    for cycle in 0..257 {
        // Exercise every adjacent transition first, then interleave repeated
        // addresses, bank changes and token holds with a deterministic shuffle.
        let next = requests[if cycle < requests.len() {
            cycle
        } else {
            (cycle * 7 + cycle / 3) % requests.len()
        }];
        let next_sequence = ((cycle / 3) & 1) as u8;
        sim.modify(|io| {
            io.set(address, next);
            io.set(sequence, next_sequence);
        })
        .unwrap();
        sim.tick(clk).unwrap();
        let mut read = |signal| {
            sim.get(signal)
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0)
        };
        let allowed = previous < 0x1000 || (0x8000_0000..0x8003_f000).contains(&previous);
        let start = physical(previous & !7);
        let want = if strict && !allowed {
            0
        } else {
            u64::from_le_bytes(expected[start..start + 8].try_into().unwrap())
        };
        assert_eq!(read(response_valid), 1, "cycle={cycle}");
        assert_eq!(read(response_address), u64::from(previous & !7));
        assert_eq!(read(response_sequence), u64::from(previous_sequence));
        assert_eq!(
            read(response_line),
            want,
            "cycle={cycle}, address={previous:#x}, strict={strict}"
        );
        (previous, previous_sequence) = (next, next_sequence);
    }
}

#[test]
fn back_to_back_fetches_preserve_bank_permission_and_response_tags() {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let source = std::fs::read_to_string(project.join("src/HardwareMemory.veryl")).unwrap();
    for strict in [false, true] {
        for fast in [0, 6] {
            let build = || {
                SimulatorBuilder::new(&source, "HardwareMemory")
                    .param("FAST_LSU", fast)
                    .param("STRICT_MEMORY", u64::from(strict))
            };
            check_responses(build().build_native().unwrap(), strict);
            check_responses(build().build_wasm().unwrap(), strict);
        }
    }
}
