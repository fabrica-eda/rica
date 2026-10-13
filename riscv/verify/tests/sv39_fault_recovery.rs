//! Late CM faults at high kernel virtual PCs must beat predictor repair.
use celox::{OptLevel, SimulatorBuilder};
use std::{collections::BTreeMap, path::Path};

const KERNEL: u64 = 0xffff_ffff_8000_0000;
const PHYSICAL: u64 = 0x8000_0000;
const ROOT: u64 = 0x8002_0000;

fn addi(rd: u32, rs: u32, imm: i32) -> u32 {
    ((imm as u32 & 4095) << 20) | (rs << 15) | (rd << 7) | 0x13
}
fn csr_write(csr: u32, rs: u32) -> u32 {
    (csr << 20) | (rs << 15) | 0x1073
}
fn constant(program: &mut Vec<u32>, rd: u32, value: u64) {
    program.push(addi(rd, 0, 0));
    for byte in value.to_be_bytes() {
        program.push((8 << 20) | (rd << 15) | (1 << 12) | (rd << 7) | 0x13);
        program.push(addi(rd, rd, i32::from(byte)));
    }
}
fn put(memory: &mut BTreeMap<u64, u8>, address: u64, bytes: &[u8]) {
    for (i, byte) in bytes.iter().enumerate() {
        memory.insert(address + i as u64, *byte);
    }
}
fn read(memory: &BTreeMap<u64, u8>, address: u64) -> u64 {
    u64::from_le_bytes(std::array::from_fn(|i| {
        memory.get(&(address + i as u64)).copied().unwrap_or(0)
    }))
}

#[test]
fn high_virtual_pc_page_and_atomic_faults_reach_handler_before_younger_work() {
    let source =
        verify_riscv::core_source(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    let image = SimulatorBuilder::new(&source, "Core")
        .param("ENABLE_U", 1u64)
        .param("ENABLE_S", 1u64)
        .param("ENABLE_SV39", 1u64)
        .param("ENABLE_A", 1u64)
        .param("STRICT_MEMORY", 1u64)
        .param("SYNC_DATA_MEMORY", 1u64)
        .param("BRANCH_PREDICTOR_CONFIG", 8u64)
        .opt_level(OptLevel::O0)
        .compile_native()
        .unwrap()
        .into_program_image();
    // Missing PTE for load/store, followed by an atomic alignment fault on
    // a valid mapping. All three faults are discovered after EX.
    for (instruction, address, cause) in [
        (0x0005_8783u32, 0x100u64, 13u64), // lb a5, 0(a1)
        (0x00f5_8023, 0x100, 15),          // sb a5, 0(a1)
        (0x0005_b7af, KERNEL + 1, 6),      // amoadd.d a5, zero, (a1)
    ] {
        let mut memory = BTreeMap::new();
        let mut boot = Vec::new();
        for (csr, value) in [
            (0x3b0, 0x3fff_ffff), // all physical memory PMP RWX
            (0x3a0, 0x1f),
            (0x302, (1 << 13) | (1 << 15) | (1 << 6)),
            (0x105, KERNEL + 0x100),
            (0x180, (8 << 60) | (ROOT >> 12)),
            (0x341, KERNEL),
            (0x300, 0x800), // MPP=S
        ] {
            constant(&mut boot, 5, value);
            boot.push(csr_write(csr, 5));
        }
        constant(&mut boot, 11, address);
        boot.push(0x3020_0073); // mret
        for (i, word) in boot.iter().enumerate() {
            put(&mut memory, i as u64 * 4, &word.to_le_bytes());
        }
        let leaf = ((PHYSICAL >> 12) << 10) | 0xcf; // 1 GiB S RWXAD
        put(&mut memory, ROOT + 510 * 8, &leaf.to_le_bytes());
        for (i, word) in [instruction, addi(31, 0, 99), 0x0000_006f]
            .iter()
            .enumerate()
        {
            put(&mut memory, PHYSICAL + i as u64 * 4, &word.to_le_bytes());
        }
        // Read the saved architectural fault state before reporting success.
        for (i, word) in [
            0x1420_2573u32,
            0x1410_25f3,
            0x1430_2673,
            addi(31, 0, 42),
            0x0000_006f,
        ]
        .iter()
        .enumerate()
        {
            put(
                &mut memory,
                PHYSICAL + 0x100 + i as u64 * 4,
                &word.to_le_bytes(),
            );
        }
        let mut sim = SimulatorBuilder::new("", "Core")
            .build_native_from_image(image.clone())
            .unwrap();
        let clk = sim.event("clk");
        macro_rules! sig {
            ($s:literal) => {
                sim.signal($s)
            };
        }
        let rst = sig!("rst_n");
        let valid = sig!("instr_valid");
        let insn = sig!("instr");
        let compressed = sig!("instr_is_compressed");
        let rdata = sig!("data_rdata");
        let response = sig!("data_response_sequence");
        let pc = sig!("pc");
        let translated = sig!("fetch_translate");
        let frame = sig!("fetch_frame");
        let request = sig!("data_request_sequence");
        let addr = sig!("data_addr");
        let we = sig!("data_we");
        let commit = sig!("commit_valid");
        let commit_pc = sig!("commit_pc");
        let rd = sig!("commit_rd");
        let data = sig!("commit_data");
        sim.modify(|io| {
            io.set(rst, 0u8);
            io.set(valid, 1u8);
            io.set(insn, 0x13u32);
            io.set(compressed, 0u8);
            io.set(response, 0u8);
        })
        .unwrap();
        sim.tick(clk).unwrap();
        sim.modify(|io| io.set(rst, 1u8)).unwrap();
        let mut ack = 0u8;
        let mut memory_data = 0u64;
        let mut saved = [0u64; 3];
        let mut done = false;
        for _ in 0..10_000 {
            let va = sim.get_as::<u64>(pc);
            let pa = if sim.get_as::<u8>(translated) != 0 {
                (u64::from(sim.get_as::<u32>(frame)) << 12) | (va & 4095)
            } else {
                va
            };
            let instruction = read(&memory, pa) as u32;
            let token = sim.get_as::<u8>(request);
            if token != ack {
                assert_eq!(sim.get_as::<u8>(we), 0, "faulting store issued a write");
                memory_data = read(&memory, sim.get_as::<u64>(addr));
                ack = token;
            }
            sim.modify(|io| {
                io.set(insn, instruction);
                io.set(compressed, u8::from(instruction & 3 != 3));
                io.set(rdata, memory_data);
                io.set(response, ack);
            })
            .unwrap();
            sim.tick(clk).unwrap();
            if sim.get_as::<u8>(commit) != 0 {
                let at = sim.get_as::<u64>(commit_pc);
                assert!(
                    !(KERNEL..KERNEL + 12).contains(&at),
                    "fault or younger instruction retired: {at:#x}, cause {cause}"
                );
                if (KERNEL + 0x100..KERNEL + 0x10c).contains(&at) {
                    saved[(at - KERNEL - 0x100) as usize / 4] = sim.get_as::<u64>(data);
                }
                if sim.get_as::<u8>(rd) == 31 {
                    assert_eq!(sim.get_as::<u64>(data), 42);
                    done = true;
                    break;
                }
            }
        }
        assert!(done, "handler never reached for cause {cause}");
        assert_eq!(saved, [cause, KERNEL, address]);
    }
}
