//! Exercise the board's complete JTAG-to-BRAM-to-CPU path.
//! This complements physical WRITE64 diagnostics: a passing zero-delay model
//! cannot establish the timing of the independent JTAG clock domain.
use std::fs;
use std::path::Path;

use celox::{NativeBackend, Simulator, SimulatorBuilder};
use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{MappingOptions, map_to_ecp5_with_options};

struct Board {
    sim: Simulator<NativeBackend>,
    cycles: usize,
}

impl Board {
    fn set(&mut self, name: &str, value: u8) {
        let signal = self.sim.signal(name);
        self.sim.modify(|io| io.set(signal, value)).unwrap();
    }

    fn tick(&mut self, name: &str) {
        let event = self.sim.event(name);
        self.sim.tick(event).unwrap();
    }

    fn run(&mut self, cycles: usize) {
        for _ in 0..cycles {
            if self.cycles.is_multiple_of(2) {
                self.tick("clk_mem");
            }
            self.tick("clk_cpu");
            self.cycles += 1;
        }
    }

    fn command(&mut self, opcode: u8, address: u32, value: u64, mask: u8) {
        let mut bytes = [0_u8; 16];
        bytes[..3].copy_from_slice(&[0xd5, opcode << 4 | 1, 0x55]);
        bytes[3..7].copy_from_slice(&address.to_le_bytes());
        bytes[7..15].copy_from_slice(&value.to_le_bytes());
        bytes[15] = mask;
        self.set("jtag_ce1", 1);
        self.set("jtag_shift", 1);
        let mut delayed_tdi = 0;
        for bit in 0..128 {
            self.set("jtag_tdi", delayed_tdi);
            self.tick("jtag_tck");
            delayed_tdi = bytes[bit / 8] >> (bit % 8) & 1;
            if bit % 8 == 0 {
                self.run(20);
            }
        }
        self.set("jtag_shift", 0);
        self.set("jtag_ce1", 0);
        self.set("jtag_tdi", delayed_tdi);
        self.set("jtag_update", 1);
        self.tick("jtag_tck");
        self.set("jtag_update", 0);
        self.tick("jtag_tck");
        self.tick("jtag_tck");
        self.run(80);
    }

    fn read(&mut self, first: u32, width: u32) -> u64 {
        let mut result = 0_u64;
        for bit in 0..width {
            self.command(4, first + bit, 0, 0);
            let signal = self.sim.signal("jtag_tdo1");
            let value = self.sim.get(signal).to_u64_digits();
            result |= value.first().copied().unwrap_or(0) << bit;
        }
        result
    }
}

fn board_source() -> String {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut source = verify_riscv::core_source(project).unwrap();
    for name in [
        "InstructionBuffer.veryl",
        "InstructionCache.veryl",
        "HardwareMemory.veryl",
        "JtagDebugTransport.veryl",
        "HardwareTestTop.veryl",
        "HardwareCachedTestTop.veryl",
        "HardwareCompactTestTop.veryl",
    ] {
        source.push_str(&fs::read_to_string(project.join("src").join(name)).unwrap());
    }
    source
}

#[test]
fn native_board_clears_captures_and_freezes_terminal_results() {
    let sim = SimulatorBuilder::new(&board_source(), "HardwareCompactAccessFaultTestTop")
        .build_native()
        .unwrap();
    exercise_board(sim);
}

#[test]
#[ignore = "struo-celox rejects the derived board reset with NonScalarControl; see the WRITE64 MRE"]
fn mapped_board_loads_and_executes_masked_write64_payloads() {
    let source = board_source();
    let design = analyze_and_lower(&source, "rica_riscv", "HardwareCached4KTestTop").unwrap();
    let synthesized = synthesize(&design).unwrap();
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )
    .unwrap();
    assert!(mapped.retiming().equivalence_signed_off);
    eprintln!(
        "mapped {} cells; compiling native board",
        mapped.cells().len()
    );
    let sim = ecp5_simulator(&mapped).unwrap().build_native().unwrap();
    exercise_board(sim);
}

fn exercise_board(sim: Simulator<NativeBackend>) {
    let mut board = Board { sim, cycles: 0 };
    board.set("pll_locked", 0);
    board.set("jtag_rst_n", 0);
    board.set("jtag_ce1", 0);
    board.set("jtag_ce2", 0);
    board.set("jtag_shift", 0);
    board.set("jtag_tdi", 0);
    board.tick("jtag_tck");
    board.set("jtag_update", 0);
    board.run(10);
    board.set("pll_locked", 1);
    board.set("jtag_rst_n", 1);
    board.run(10);

    for write_bytes in [8, 1] {
        for (immediate, terminal) in [(0x5a_u32, 1_u32), (0x123, 0), (0x200, 3)] {
            // Set x31, then retire a terminal store to 0x100.
            let words = [
                immediate << 20 | 0xf93,
                0x1000_0093,
                terminal << 20 | 0x113,
                0x0020_b023,
                0x0000_006f,
                0x0000_0013,
            ];
            let bytes = words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            board.command(3, 0, 2, 0);
            assert_eq!(board.read(32, 64), 0, "CONTROL clears the previous result");
            assert_eq!(board.read(18, 1), 0, "CONTROL clears result_valid");
            for (index, chunk) in bytes.chunks(write_bytes).enumerate() {
                let mut payload = [0_u8; 8];
                payload[..chunk.len()].copy_from_slice(chunk);
                let mask = ((1_u16 << chunk.len()) - 1) as u8;
                board.command(
                    1,
                    (index * write_bytes) as u32,
                    u64::from_le_bytes(payload),
                    mask,
                );
            }
            board.command(2, 0x100, 0, 0);
            board.command(3, 0, 1, 0);
            board.run(2_000);
            assert_eq!(
                board.read(32, 64),
                u64::from(terminal),
                "write_bytes={write_bytes} immediate={immediate:x}"
            );
            assert_eq!(board.read(96, 64), u64::from(immediate));
            assert_eq!(board.read(18, 1), u64::from(terminal != 0));
            let retired = board.read(160 + 256, 64);
            board.run(200);
            let later_retired = board.read(160 + 256, 64);
            if terminal != 0 {
                assert_eq!(
                    retired, 4,
                    "minstret includes the acknowledged terminal store"
                );
                assert_eq!(later_retired, retired, "nonzero result freezes counters");
            } else {
                assert!(
                    later_retired > retired,
                    "zero result leaves the CPU running"
                );
            }
            board.command(0, 0, 0, 0);
            board.command(2, 0x108, 0, 0);
            board.command(3, 0, 0, 0);
            assert_eq!(
                board.read(32, 64),
                u64::from(terminal),
                "stopping without clear preserves the result"
            );
        }
    }
}
