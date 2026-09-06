use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};

const NOPS: u64 = 0x0000_0013_0000_0013;

fn native_image(index_bits: u64) -> &'static NativeProgramImage {
    static IMAGES: [OnceLock<NativeProgramImage>; 3] = [const { OnceLock::new() }; 3];
    IMAGES[(index_bits - 7) as usize].get_or_init(|| {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = fs::read_to_string(project.join("src/InstructionCache.veryl")).unwrap();
        SimulatorBuilder::new(&source, "InstructionCache")
            .param("INDEX_BITS", index_bits)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

struct Driver {
    simulator: Simulator<NativeBackend>,
    memory: BTreeMap<u32, u64>,
    replies: VecDeque<(usize, u32, u8, u64)>,
    requests: Vec<(u32, u8)>,
    responses: Vec<(u32, u8)>,
    sequence: u8,
    cycles: usize,
    latency: usize,
    automatic_response: bool,
}

impl Driver {
    fn new(index_bits: u64, memory: impl IntoIterator<Item = (u32, u64)>) -> Self {
        let mut simulator = SimulatorBuilder::new("", "InstructionCache")
            .build_native_from_image(native_image(index_bits).clone())
            .unwrap();
        let clock = simulator.event("clk");
        let rst_n = simulator.signal("rst_n");
        let invalidate = simulator.signal("invalidate");
        let pc = simulator.signal("pc");
        let response_address = simulator.signal("memory_response_address");
        let response_line = simulator.signal("memory_response_line");
        let response_valid = simulator.signal("memory_response_valid");
        let response_sequence = simulator.signal("memory_response_sequence");
        simulator
            .modify(|io| {
                io.set(rst_n, 0_u8);
                io.set(invalidate, 0_u8);
                io.set(pc, 0_u32);
                io.set(response_address, 0_u32);
                io.set(response_line, 0_u64);
                io.set(response_valid, 0_u8);
                io.set(response_sequence, 0_u8);
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
        let mut driver = Self {
            simulator,
            memory: memory.into_iter().collect(),
            replies: VecDeque::new(),
            requests: Vec::new(),
            responses: Vec::new(),
            sequence: 0,
            cycles: 0,
            latency: 4,
            automatic_response: true,
        };
        for _ in 0..1 << index_bits {
            assert!(!driver.valid(), "instruction escaped the reset scrub");
            driver.step();
        }
        driver
    }

    fn value(&mut self, name: &str) -> u64 {
        let signal = self.simulator.signal(name);
        self.simulator
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn valid(&mut self) -> bool {
        self.value("instruction_valid") != 0
    }

    fn pc(&mut self, value: u32) {
        let pc = self.simulator.signal("pc");
        self.simulator.modify(|io| io.set(pc, value)).unwrap();
    }

    fn invalidate(&mut self, value: bool) {
        let invalidate = self.simulator.signal("invalidate");
        self.simulator
            .modify(|io| io.set(invalidate, u8::from(value)))
            .unwrap();
    }

    fn response(&mut self, address: u32, sequence: u8, line: u64) {
        self.responses.push((address, sequence));
        let response_address = self.simulator.signal("memory_response_address");
        let response_line = self.simulator.signal("memory_response_line");
        let response_valid = self.simulator.signal("memory_response_valid");
        let response_sequence = self.simulator.signal("memory_response_sequence");
        self.simulator
            .modify(|io| {
                io.set(response_address, address);
                io.set(response_line, line);
                io.set(response_valid, 1_u8);
                io.set(response_sequence, sequence);
            })
            .unwrap();
    }

    fn step(&mut self) {
        if self.automatic_response && self.replies.front().is_some_and(|r| r.0 <= self.cycles) {
            let (_, address, sequence, line) = self.replies.pop_front().unwrap();
            // Hold the last response bundle, as HardwareMemory does. A stale
            // valid response must not satisfy a different transaction.
            self.response(address, sequence, line);
        }
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
        self.cycles += 1;
        let sequence = self.value("memory_request_sequence") as u8;
        if sequence != self.sequence {
            let address = self.value("memory_request_address") as u32;
            self.sequence = sequence;
            self.requests.push((address, sequence));
            self.replies.push_back((
                self.cycles + self.latency,
                address,
                sequence,
                self.memory.get(&address).copied().unwrap_or(NOPS),
            ));
        }
    }

    fn fetch(&mut self, pc: u32) -> u32 {
        self.pc(pc);
        for _ in 0..100 {
            if self.valid() {
                return self.value("instruction") as u32;
            }
            self.step();
        }
        panic!(
            "fetch timed out at {pc:#010x}; requests={:?}",
            self.requests
        );
    }

    fn await_request(&mut self) -> (u32, u8) {
        let previous = self.requests.len();
        for _ in 0..100 {
            self.step();
            if self.requests.len() != previous {
                return *self.requests.last().unwrap();
            }
        }
        panic!("no memory request");
    }
}

#[test]
fn both_cross_line_orders_and_address_wrap_produce_the_right_word() {
    for (pc, lower, upper) in [
        (0x1006, 0x1000, 0x1008),
        (0x200e, 0x2008, 0x2010),
        (0xffff_fffe, 0xffff_fff8, 0),
    ] {
        let mut driver = Driver::new(
            7,
            [
                (lower, 0x00b3_0001_0001_0001),
                (upper, 0x0001_0001_0001_a55a),
            ],
        );
        assert_eq!(driver.fetch(pc), 0xa55a_00b3);
        assert_eq!(driver.value("instruction_compressed"), 0);
    }
}

#[test]
fn final_compressed_parcel_is_usable_before_the_successor_returns() {
    let mut driver = Driver::new(7, [(0x3008, 0x4505_0001_0001_0001)]);
    assert_eq!(driver.fetch(0x300e), 0x4505);
    assert_eq!(driver.value("instruction_compressed"), 1);
    assert!(driver.responses.iter().all(|r| r.0 != 0x3010));
}

#[test]
fn warm_sequential_fetch_sustains_one_window_per_cycle() {
    for stride in [2, 4] {
        let mut driver = Driver::new(
            7,
            (0x4000..0x4080).step_by(8).map(|address| {
                let line = if stride == 4 {
                    NOPS
                } else {
                    0x0001_0001_0001_0001
                };
                (address, line)
            }),
        );
        for address in (0x4000..0x4080).step_by(8) {
            driver.fetch(address);
        }
        driver.fetch(0x4000);
        let requests = driver.requests.len();
        for pc in (0x4000..0x4070).step_by(stride) {
            driver.pc(pc);
            assert!(driver.valid(), "warm hit stalled at {pc:#x}");
            assert_eq!(
                driver.value("instruction_compressed"),
                u64::from(stride == 2)
            );
            assert_eq!(
                driver.value("instruction") & 0xffff,
                if stride == 2 { 1 } else { 0x13 }
            );
            driver.step();
        }
        assert_eq!(
            driver.requests.len(),
            requests,
            "warm stream generated a miss"
        );
    }
}

#[test]
fn all_capacities_keep_full_address_tags_and_refetch_evicted_lines() {
    for bits in [7, 8, 9] {
        let original = 0x8000_1000;
        let alias = original + (1 << (bits + 4));
        let mut driver = Driver::new(
            bits,
            [
                (original, 0x0010_0513_0010_0513),
                (alias, 0x0020_0513_0020_0513),
            ],
        );
        assert_eq!(driver.fetch(original), 0x0010_0513);
        assert_eq!(driver.fetch(alias), 0x0020_0513);
        let previous = driver.requests.len();
        assert_eq!(driver.fetch(original), 0x0010_0513);
        assert!(driver.requests[previous..].iter().any(|r| r.0 == original));
        assert_eq!(
            driver.fetch(original & 0x7fff_ffff),
            0x13,
            "physical address bit 31 must be part of the tag"
        );
    }
}

#[test]
fn redirect_during_a_miss_never_exposes_the_old_pc_payload() {
    let mut driver = Driver::new(
        7,
        [
            (0x5000, 0x0010_0513_0010_0513),
            (0x7008, 0x0020_0513_0020_0513),
        ],
    );
    driver.latency = 12;
    driver.pc(0x5000);
    assert_eq!(driver.await_request().0, 0x5000);
    driver.pc(0x7008);
    for _ in 0..12 {
        assert!(
            !driver.valid(),
            "redirect consumed a response for the old PC"
        );
        driver.step();
    }
    assert_eq!(driver.fetch(0x7008), 0x0020_0513);
    assert_eq!(driver.fetch(0x5000), 0x0010_0513);
}

#[test]
fn invalidation_discards_pending_and_held_responses_even_at_the_same_address() {
    let mut driver = Driver::new(7, []);
    driver.automatic_response = false;
    driver.pc(0x8000);
    let (address, stale_sequence) = driver.await_request();
    driver.response(address, stale_sequence, NOPS);
    driver.invalidate(true);
    assert!(!driver.valid());
    driver.step();
    driver.invalidate(false);
    for _ in 0..128 {
        assert!(!driver.valid(), "invalidation scrub exposed a cached line");
        driver.step();
    }
    let (fresh_address, fresh_sequence) = driver.await_request();
    assert_eq!(fresh_address, address);
    assert_ne!(fresh_sequence, stale_sequence);
    for _ in 0..8 {
        driver.step();
        assert!(
            !driver.valid(),
            "pre-FENCE.I response satisfied a post-FENCE.I miss"
        );
    }
    driver.response(address, fresh_sequence, 0x0020_0513_0020_0513);
    assert_eq!(driver.fetch(address), 0x0020_0513);
}

#[test]
fn invalidation_removes_warm_lines_and_handles_a_bank_write_collision() {
    let mut driver = Driver::new(7, [(0x9000, NOPS), (0x9008, NOPS)]);
    assert_eq!(driver.fetch(0x9000), 0x13);
    assert_eq!(driver.fetch(0x9008), 0x13);
    driver.memory.insert(0x9000, 0x0030_0513_0030_0513);
    driver.memory.insert(0x9008, 0x0040_0513_0040_0513);
    driver.invalidate(true);
    driver.step();
    driver.invalidate(false);
    for _ in 0..128 {
        assert!(!driver.valid());
        driver.step();
    }
    assert_eq!(driver.fetch(0x9000), 0x0030_0513);
    assert_eq!(driver.fetch(0x9008), 0x0040_0513);
}

#[test]
fn mixed_halfword_windows_survive_redirects_evictions_and_repeated_invalidations() {
    let mut random = 0x1234_5678_9abc_def0_u64;
    let mut next_random = || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random
    };
    for bits in [7, 9] {
        let memory = (0x1000..0x5000)
            .step_by(8)
            .map(|address| (address, next_random()))
            .collect::<Vec<_>>();
        let mut driver = Driver::new(bits, memory);
        for access in 0..400 {
            if access % 73 == 72 {
                driver.memory.insert(0x1000, next_random());
                driver.invalidate(true);
                driver.step();
                driver.invalidate(false);
                for _ in 0..1 << bits {
                    assert!(!driver.valid());
                    driver.step();
                }
            }
            let pc = if access % 3 == 0 {
                0x1000 + (access % 32) * 2
            } else {
                0x1000 + ((next_random() as u32) & 0x3ffe)
            };
            let byte = |address: u32| {
                let line = driver.memory.get(&(address & !7)).copied().unwrap_or(NOPS);
                (line >> (8 * (address & 7))) as u8
            };
            let expected =
                u32::from_le_bytes(std::array::from_fn(|offset| byte(pc + offset as u32)));
            let observed = driver.fetch(pc);
            let compressed = expected & 3 != 3;
            assert_eq!(
                driver.value("instruction_compressed"),
                u64::from(compressed)
            );
            let mask = if compressed { 0xffff } else { u32::MAX };
            assert_eq!(
                observed & mask,
                expected & mask,
                "{bits} index bits, access {access}, PC {pc:#x}"
            );
        }
    }
}

#[test]
fn cache_maps_to_ebr_and_the_mapped_model_refills_and_invalidates() {
    use struo_celox::ecp5_simulator;
    use struo_frontend_veryl::analyze_and_lower;
    use struo_synth::synthesize;
    use struo_target_ecp5::{Ecp5Cell, MappingOptions, map_to_ecp5_with_options};

    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = fs::read_to_string(project.join("src/InstructionCache.veryl")).unwrap();
    let design = analyze_and_lower(&source, "rica_riscv", "InstructionCache").unwrap();
    let synthesized = synthesize(&design).unwrap();
    assert_eq!(synthesized.netlist.memories().len(), 2);
    let mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 125,
            ..MappingOptions::default()
        },
    )
    .unwrap();
    assert!(mapped.retiming().equivalence_signed_off);
    assert_eq!(
        mapped
            .cells()
            .iter()
            .filter(|cell| matches!(cell, Ecp5Cell::BlockRam { .. }))
            .count(),
        10
    );
    let mut simulator = ecp5_simulator(&mapped).unwrap().build_native().unwrap();
    let clock = simulator.event("clk");
    let rst_n = simulator.signal("rst_n");
    let invalidate = simulator.signal("invalidate");
    let pc = simulator.signal("pc");
    let response_address = simulator.signal("memory_response_address");
    let response_line = simulator.signal("memory_response_line");
    let response_sequence = simulator.signal("memory_response_sequence");
    let response_valid = simulator.signal("memory_response_valid");
    let request_address = simulator.signal("memory_request_address");
    let request_sequence = simulator.signal("memory_request_sequence");
    let instruction = simulator.signal("instruction");
    let valid = simulator.signal("instruction_valid");
    simulator
        .modify(|io| {
            io.set(rst_n, 0_u8);
            io.set(invalidate, 0_u8);
            io.set(pc, 0x1006_u32);
            io.set(response_address, 0_u32);
            io.set(response_line, 0_u64);
            io.set(response_sequence, 0_u8);
            io.set(response_valid, 0_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
    for (phase, target) in [0x1006_u32, 0x1806, 0x1006, 0x1006].into_iter().enumerate() {
        let word = if phase == 3 {
            0xbeef_00b3_u32
        } else {
            0xa55a_00b3_u32
        };
        simulator
            .modify(|io| {
                io.set(pc, target);
                io.set(invalidate, u8::from(phase == 3));
            })
            .unwrap();
        if phase == 3 {
            simulator.tick(clock).unwrap();
            simulator.modify(|io| io.set(invalidate, 0_u8)).unwrap();
        }
        let mut found = false;
        for _ in 0..180 {
            if simulator
                .get(valid)
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0)
                != 0
            {
                assert_eq!(
                    simulator.get(instruction).to_u64_digits()[0],
                    u64::from(word)
                );
                found = true;
                break;
            }
            let address = simulator
                .get(request_address)
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0) as u32;
            let sequence = simulator
                .get(request_sequence)
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0) as u8;
            simulator
                .modify(|io| {
                    io.set(response_address, address);
                    io.set(response_sequence, sequence);
                    io.set(response_valid, 1_u8);
                    io.set(
                        response_line,
                        if address & 8 == 0 {
                            0x00b3_0001_0001_0001_u64
                        } else {
                            u64::from(word >> 16)
                        },
                    );
                })
                .unwrap();
            simulator.tick(clock).unwrap();
        }
        assert!(found, "mapped cache timed out in phase {phase}");
    }
}

#[test]
fn stopping_and_reloading_the_cpu_scrubs_previously_warm_lines() {
    let mut driver = Driver::new(7, [(0xa000, NOPS), (0xa008, NOPS)]);
    assert_eq!(driver.fetch(0xa000), 0x13);
    assert_eq!(driver.fetch(0xa008), 0x13);
    let rst_n = driver.simulator.signal("rst_n");
    driver.simulator.modify(|io| io.set(rst_n, 0_u8)).unwrap();
    for _ in 0..4 {
        driver.step();
    }
    driver.memory.insert(0xa000, 0x0060_0513_0060_0513);
    driver.memory.insert(0xa008, 0x0070_0513_0070_0513);
    driver.simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
    for _ in 0..128 {
        assert!(!driver.valid());
        driver.step();
    }
    assert_eq!(driver.fetch(0xa000), 0x0060_0513);
    assert_eq!(driver.fetch(0xa008), 0x0070_0513);
}

#[test]
fn held_pc_prefetches_exactly_two_successor_lines_including_address_wrap() {
    for base in [0x4000_u32, 0x4008, 0xffff_fff0, 0xffff_fff8] {
        let lines = [base, base.wrapping_add(8), base.wrapping_add(16)];
        let mut driver = Driver::new(7, lines.map(|address| (address, NOPS)));
        assert_eq!(driver.fetch(base + 4), 0x13);
        for _ in 0..80 {
            driver.step();
            assert!(driver.valid(), "prefetch disturbed the held current line");
            assert_eq!(driver.value("instruction"), 0x13);
        }
        assert_eq!(
            driver
                .requests
                .iter()
                .map(|(address, _)| *address)
                .collect::<Vec<_>>(),
            lines,
            "prefetch must reach PC+16 and stop while PC is held"
        );
    }
}
