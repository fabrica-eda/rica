use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, OptLevel, Simulator, SimulatorBuilder};

const NOP: u32 = 0x0000_0013;
const EARLY_DIVIDE_CONFIG: u64 = 32_648;
const INTERRUPTS_CONFIG: u64 = EARLY_DIVIDE_CONFIG | 32_768;
const ACCESS_CONFIG: u64 = INTERRUPTS_CONFIG | 65_536;
const KIND_CONDITIONAL: u64 = 1;
const KIND_DIRECT: u64 = 2;
const KIND_CALL: u64 = 4;

fn project_path() -> PathBuf {
    // Allow recovery regressions against an immutable RTL snapshot while a
    // separate physical implementation continues using the working project.
    if let Some(project) = std::env::var_os("RICA_CORE_RECOVERY_PROJECT") {
        return PathBuf::from(project);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn native_image(config: u64) -> &'static NativeProgramImage {
    static ACCESS: OnceLock<NativeProgramImage> = OnceLock::new();
    static ACCESS_STATIC: OnceLock<NativeProgramImage> = OnceLock::new();
    static INTERRUPTS: OnceLock<NativeProgramImage> = OnceLock::new();
    static INTERRUPTS_STATIC: OnceLock<NativeProgramImage> = OnceLock::new();
    static STATIC: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_EARLY_DIVIDE: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_TAKEN_FETCH: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT32: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_M: OnceLock<NativeProgramImage> = OnceLock::new();
    static STATIC_M: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBB_ONLY: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBS: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_B: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_B_DSP_M: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBS_ZBB: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBS_ZBA: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBB_DSP_M: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBB: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_ZBA: OnceLock<NativeProgramImage> = OnceLock::new();
    static COMPACT_DSP_M: OnceLock<NativeProgramImage> = OnceLock::new();
    static EARLY_1544: OnceLock<NativeProgramImage> = OnceLock::new();
    static EARLY_1928: OnceLock<NativeProgramImage> = OnceLock::new();
    static EARLY_2056: OnceLock<NativeProgramImage> = OnceLock::new();
    static EARLY_3976: OnceLock<NativeProgramImage> = OnceLock::new();
    let image = match config {
        ACCESS_CONFIG => &ACCESS,
        98_436 => &ACCESS_STATIC,
        INTERRUPTS_CONFIG => &INTERRUPTS,
        32_900 => &INTERRUPTS_STATIC,
        EARLY_DIVIDE_CONFIG => &COMPACT_EARLY_DIVIDE,
        8200 => &COMPACT_TAKEN_FETCH,
        8072 => &EARLY_3976,
        6152 => &EARLY_2056,
        6024 => &EARLY_1928,
        5640 => &EARLY_1544,
        4 => &STATIC,
        8 => &COMPACT,
        24 => &COMPACT32,
        136 => &COMPACT_M,
        132 => &STATIC_M,
        392 => &COMPACT_DSP_M,
        520 => &COMPACT_ZBA,
        1544 => &COMPACT_ZBB,
        1928 => &COMPACT_ZBB_DSP_M,
        1032 => &COMPACT_ZBB_ONLY,
        2056 => &COMPACT_ZBS,
        3592 => &COMPACT_B,
        3976 => &COMPACT_B_DSP_M,
        3080 => &COMPACT_ZBS_ZBB,
        2568 => &COMPACT_ZBS_ZBA,
        _ => panic!("unknown predictor configuration: {config}"),
    };
    image.get_or_init(|| {
        let source = verify_riscv::core_source(&project_path()).unwrap();
        SimulatorBuilder::new(&source, "Core")
            .param("SYNC_DATA_MEMORY", 1)
            .param("ENABLE_INTERRUPTS", u64::from(config & 32_768 != 0))
            .param("STRICT_MEMORY", u64::from(config & 65_536 != 0))
            .param("ENABLE_ZBA", u64::from(config & 512 != 0))
            .param("ENABLE_ZBB", u64::from(config & 1024 != 0))
            .param("ENABLE_ZBS", u64::from(config & 2048 != 0))
            .param("EARLY_BITMANIP", u64::from(config & 4096 != 0))
            .param("EARLY_TAKEN_FETCH", u64::from(config & 8192 != 0))
            .param("ENABLE_M", u64::from(config & 128 != 0))
            .param("M_USE_DSP", u64::from(config & 256 != 0))
            .param("EARLY_DIVIDE", u64::from(config & 16_384 != 0))
            .param("BRANCH_PREDICTOR_CONFIG", config & 127)
            .opt_level(OptLevel::O0)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

fn encode_branch(rs1: u32, rs2: u32, funct3: u32, offset: i32) -> u32 {
    assert_eq!(offset & 1, 0);
    assert!((-4096..=4094).contains(&offset));
    let immediate = offset as u32;
    ((immediate >> 12) & 0x1) << 31
        | ((immediate >> 5) & 0x3f) << 25
        | (rs2 & 0x1f) << 20
        | (rs1 & 0x1f) << 15
        | (funct3 & 0x7) << 12
        | ((immediate >> 1) & 0xf) << 8
        | ((immediate >> 11) & 0x1) << 7
        | 0x63
}

fn encode_jal(rd: u32, offset: i32) -> u32 {
    assert_eq!(offset & 1, 0);
    assert!((-1_048_576..=1_048_574).contains(&offset));
    let immediate = offset as u32;
    ((immediate >> 20) & 0x1) << 31
        | ((immediate >> 1) & 0x3ff) << 21
        | ((immediate >> 11) & 0x1) << 20
        | ((immediate >> 12) & 0xff) << 12
        | (rd & 0x1f) << 7
        | 0x6f
}

fn encode_ld(rd: u32, rs1: u32, offset: i32) -> u32 {
    assert!((-2048..=2047).contains(&offset));
    ((offset as u32) & 0xfff) << 20 | (rs1 & 0x1f) << 15 | 3 << 12 | (rd & 0x1f) << 7 | 0x03
}

fn encode_sd(rs2: u32, rs1: u32, offset: i32) -> u32 {
    assert!((-2048..=2047).contains(&offset));
    let immediate = (offset as u32) & 0xfff;
    ((immediate >> 5) & 0x7f) << 25
        | (rs2 & 0x1f) << 20
        | (rs1 & 0x1f) << 15
        | 3 << 12
        | (immediate & 0x1f) << 7
        | 0x23
}

fn encode_csrrwi(rd: u32, csr: u32, immediate: u32) -> u32 {
    assert!(csr < 4096);
    assert!(immediate < 32);
    csr << 20 | immediate << 15 | 5 << 12 | (rd & 0x1f) << 7 | 0x73
}

fn encode_addi(rd: u32, rs1: u32, immediate: i32) -> u32 {
    assert!((-2048..=2047).contains(&immediate));
    ((immediate as u32) & 0xfff) << 20 | (rs1 & 0x1f) << 15 | (rd & 0x1f) << 7 | 0x13
}

fn encode_slli(rd: u32, rs1: u32, amount: u32) -> u32 {
    assert!(amount < 64);
    amount << 20 | (rs1 & 0x1f) << 15 | 1 << 12 | (rd & 0x1f) << 7 | 0x13
}

#[derive(Clone, Copy, Debug)]
struct Cycle {
    fetch_pc: u32,
    fetch_cursor: u32,
    ex_valid: bool,
    ex_pc: u32,
    rx_valid: bool,
    rx_pc: u32,
    execution_hazard: bool,
    cm_valid: bool,
    cm_pc: u32,
    cm_is_load: bool,
    cm_is_store: bool,
    cm_is_csr: bool,
    memory_wait: bool,
    muldiv_start: bool,
    muldiv_cancel: bool,
    instruction_flush: bool,
    data_we: bool,
    redirect_pending: bool,
    branch_redirect: bool,
    ex_mismatch: bool,
    ex_exception: bool,
    recover: bool,
    discard: bool,
    retire: bool,
    predicted_kind: u64,
    ex_kind: u64,
    predictor_request_ready: bool,
    predictor_request: bool,
    predictor_request_accept: bool,
    predictor_response_valid: bool,
    predictor_response_accept: bool,
    predictor_response_kind: u64,
    predictor_response_taken: bool,
    predictor_response_next_pc: u32,
    commit_valid: bool,
    commit_pc: u32,
    commit_rd: u8,
    commit_data: u64,
    commit_mem_we: bool,
}

struct CoreDriver {
    simulator: Simulator<NativeBackend>,
    program: BTreeMap<u32, u32>,
    data_response_sequence: u8,
}

impl CoreDriver {
    fn new(program: impl IntoIterator<Item = (u32, u32)>) -> Self {
        Self::with_config(program, 8)
    }

    fn with_config(program: impl IntoIterator<Item = (u32, u32)>, config: u64) -> Self {
        let simulator = SimulatorBuilder::new("", "Core")
            .build_native_from_image(native_image(config).clone())
            .unwrap();
        let mut driver = Self {
            simulator,
            program: program.into_iter().collect(),
            data_response_sequence: 0,
        };

        let rst_n = driver.simulator.signal("rst_n");
        let instr = driver.simulator.signal("instr");
        let instr_is_compressed = driver.simulator.signal("instr_is_compressed");
        let instr_valid = driver.simulator.signal("instr_valid");
        let data_rdata = driver.simulator.signal("data_rdata");
        let data_response_sequence = driver.simulator.signal("data_response_sequence");
        let machine_interrupts = driver.simulator.signal("machine_interrupts");
        let counter_freeze = driver.simulator.signal("counter_freeze");
        let debug_counter_index = driver.simulator.signal("debug_counter_index");
        driver
            .simulator
            .modify(|io| {
                io.set(rst_n, 0_u8);
                io.set(instr, NOP);
                io.set(instr_is_compressed, 0_u8);
                io.set(instr_valid, 1_u8);
                io.set(data_rdata, 0_u64);
                io.set(data_response_sequence, 0_u8);
                io.set(counter_freeze, 0_u8);
                io.set(machine_interrupts, 0_u8);
                io.set(debug_counter_index, 0_u8);
            })
            .unwrap();
        let clock = driver.simulator.event("clk");
        driver.simulator.tick(clock).unwrap();
        driver.simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
        driver
    }

    fn value(&mut self, name: &str) -> u64 {
        let signal = if let Some((child, signal)) = name.split_once('.') {
            self.simulator.child_signal(&[(child, 0)], signal)
        } else {
            self.simulator.signal(name)
        };
        self.simulator
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn csr_value(&mut self, name: &str) -> u64 {
        let signal = self.simulator.child_signal(&[("csr_file", 0)], name);
        self.simulator
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// The compact predictor scrubs its table after reset and accepts no
    /// request until the sweep finishes.
    fn wait_for_predictor_scrub(&mut self) {
        for _ in 0..256 {
            if self.value("predictor_ready") != 0 {
                return;
            }
            let clock = self.simulator.event("clk");
            self.simulator.tick(clock).unwrap();
        }
        panic!("the predictor scrub never finished");
    }

    fn set_interrupts(&mut self, levels: u8) {
        let signal = self.simulator.signal("machine_interrupts");
        self.simulator.modify(|io| io.set(signal, levels)).unwrap();
    }

    fn set_data_response_sequence(&mut self, sequence: u8) {
        self.data_response_sequence = sequence;
    }

    fn set_data_rdata(&mut self, value: u64) {
        let data_rdata = self.simulator.signal("data_rdata");
        self.simulator
            .modify(|io| io.set(data_rdata, value))
            .unwrap();
    }

    fn set_instr_valid(&mut self, valid: bool) {
        let instr_valid = self.simulator.signal("instr_valid");
        self.simulator
            .modify(|io| io.set(instr_valid, u8::from(valid)))
            .unwrap();
    }

    fn acknowledge_data_request(&mut self) {
        self.data_response_sequence = self.value("data_request_sequence") as u8;
    }

    fn step(&mut self) -> Cycle {
        let fetch_pc = self.value("pc") as u32;
        let instruction = self.program.get(&fetch_pc).copied().unwrap_or(NOP);
        let instr = self.simulator.signal("instr");
        let instr_is_compressed = self.simulator.signal("instr_is_compressed");
        let data_response_sequence = self.simulator.signal("data_response_sequence");
        self.simulator
            .modify(|io| {
                io.set(instr, instruction);
                io.set(instr_is_compressed, u8::from(instruction & 0b11 != 0b11));
                io.set(data_response_sequence, self.data_response_sequence);
            })
            .unwrap();

        let observed = Cycle {
            fetch_pc,
            fetch_cursor: self.value("fetch_address") as u32,
            ex_valid: self.value("ex_valid_q") != 0,
            ex_pc: self.value("ex_pc_q") as u32,
            rx_valid: self.value("rx_valid_q") != 0,
            rx_pc: self.value("rx_pc_q") as u32,
            execution_hazard: self.value("execution_hazard") != 0,
            cm_valid: self.value("cm_valid_q") != 0,
            cm_pc: self.value("cm_pc_q") as u32,
            cm_is_load: self.value("cm_is_load_q") != 0,
            cm_is_store: self.value("cm_is_store_q") != 0,
            cm_is_csr: self.value("cm_is_csr_q") != 0,
            memory_wait: self.value("memory_wait") != 0,
            muldiv_start: self.value("muldiv_start") != 0,
            muldiv_cancel: self.value("redirect_discards_cm") != 0,
            instruction_flush: self.value("instruction_flush") != 0,
            data_we: self.value("data_we") != 0,
            redirect_pending: self.value("redirect_pending_q") != 0,
            branch_redirect: self.value("branch_redirect_valid") != 0,
            ex_mismatch: self.value("ex_prediction_mismatch") != 0,
            ex_exception: self.value("ex_exception") != 0,
            recover: self.value("predictor_recover_valid") != 0,
            discard: self.value("predictor_discard_all") != 0,
            retire: self.value("cm_retire_event") != 0,
            predicted_kind: self.value("rx_predicted_kind_q"),
            ex_kind: self.value("ex_control_kind_q"),
            predictor_request_ready: self.value("predictor_request_ready") != 0,
            predictor_request: self.value("predictor_request_valid") != 0,
            predictor_request_accept: self.value("predictor_request_accept") != 0,
            predictor_response_valid: self.value("predictor_response_valid") != 0,
            predictor_response_accept: self.value("predictor_response_accept") != 0,
            predictor_response_kind: self.value("predictor_response_kind"),
            predictor_response_taken: self.value("predictor_response_taken") != 0,
            predictor_response_next_pc: self.value("predictor_response_next_pc") as u32,
            commit_valid: self.value("commit_valid") != 0,
            commit_pc: self.value("commit_pc") as u32,
            commit_rd: self.value("commit_rd") as u8,
            commit_data: self.value("commit_data"),
            commit_mem_we: self.value("commit_mem_we") != 0,
        };
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
        observed
    }

}

fn direct_trained_driver(config: u64) -> CoreDriver {
    let mut driver =
        CoreDriver::with_config([(0, encode_jal(0, 4)), (4, encode_jal(0, -4))], config);
    for _ in 0..300 {
        let cycle = driver.step();
        if cycle.rx_valid && cycle.rx_pc == 0 && cycle.predicted_kind == KIND_DIRECT {
            return driver;
        }
    }
    panic!("the direct-control BTB entry never became usable");
}

#[test]
fn non_bypass_dependency_stalls_exactly_once() {
    // ADDI can feed SLLI through the fast EX bypass, but the registered shift
    // result is not available until CM.  The dependent ADDI must wait exactly
    // one cycle and then observe that completed value.
    let mut driver = CoreDriver::new([
        (0, encode_addi(1, 0, 3)),
        (4, encode_slli(2, 1, 2)),
        (8, encode_addi(31, 2, 1)),
    ]);
    let mut hazard_cycles = 0;
    let mut consumer_retired = false;

    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.execution_hazard {
            hazard_cycles += 1;
            assert!(
                cycle.ex_valid && cycle.ex_pc == 4,
                "hazard producer was not SLLI"
            );
            assert!(
                cycle.rx_valid && cycle.rx_pc == 8,
                "hazard consumer was not dependent ADDI"
            );
            assert!(
                !cycle.predictor_request_accept,
                "frontend advanced while the dependency held RX"
            );
        }
        if cycle.retire && cycle.cm_pc == 8 {
            consumer_retired = true;
        }
        if consumer_retired && driver.value("debug_x31") == 13 {
            break;
        }
    }

    assert!(consumer_retired, "dependent ADDI did not retire");
    assert_eq!(
        driver.value("debug_x31"),
        13,
        "dependent ADDI observed stale shift data"
    );
    assert_eq!(
        hazard_cycles, 1,
        "non-bypass dependency did not insert exactly one bubble"
    );
}

#[test]
fn registered_decision_queue_sustains_one_not_taken_request_per_clock() {
    let mut driver = CoreDriver::new([]);
    driver.wait_for_predictor_scrub();
    let mut consecutive = 0;
    let mut longest = 0;
    let mut previous_request_pc = None;

    for _ in 0..40 {
        let cycle = driver.step();
        if cycle.predictor_request
            && cycle.predictor_request_accept
            && cycle.predictor_response_accept
        {
            if let Some(pc) = previous_request_pc {
                assert_eq!(cycle.fetch_pc, pc + 4);
            }
            previous_request_pc = Some(cycle.fetch_pc);
            consecutive += 1;
            longest = longest.max(consecutive);
        } else {
            consecutive = 0;
            previous_request_pc = None;
        }
    }

    assert!(
        longest >= 8,
        "registered NT pipeline never reached one request/accept per clock"
    );
}

#[test]
fn learned_taken_cancel_holds_redirect_target_and_skips_fallthrough() {
    // Once the always-taken branch is learned, the predictor has already
    // launched PC 4 as its sequential shadow when the registered taken
    // decision redirects to PC 8.  PC 4 would overwrite x31 with 2047 if its
    // canceled response ever became architectural; the target instead
    // increments x31 once per completed loop.
    let mut driver = CoreDriver::new([
        (0, encode_branch(0, 0, 0, 8)),
        (4, encode_addi(31, 0, 2047)),
        (8, encode_addi(31, 31, 1)),
        (12, encode_jal(0, -12)),
    ]);
    let mut target_commits = 0_u64;
    let mut target_commits_at_cancel = 0_u64;
    let mut cancellation_seen = false;
    let mut expect_target_request = false;
    let mut target_request_seen = false;

    for _ in 0..500 {
        let cycle = driver.step();

        if cycle.commit_valid && cycle.commit_pc == 4 {
            panic!("the canceled fall-through instruction committed");
        }
        if cycle.commit_valid && cycle.commit_pc == 8 {
            target_commits += 1;
            assert_eq!(cycle.commit_rd, 31);
            assert_eq!(
                cycle.commit_data, target_commits,
                "x31 contains a side effect from the skipped fall-through",
            );
        }

        if expect_target_request {
            assert_eq!(
                cycle.fetch_pc, 8,
                "the redirect target was not held across shadow cancellation",
            );
            assert!(cycle.predictor_request, "the target lookup was not offered");
            assert!(
                cycle.predictor_request_ready,
                "the predictor did not reopen after consuming the shadow",
            );
            assert!(
                cycle.predictor_request_accept,
                "the target lookup was not accepted on the cycle after cancellation",
            );
            expect_target_request = false;
            target_request_seen = true;
        }

        let conditional_redirect = cycle.predictor_response_valid
            && cycle.predictor_response_accept
            && cycle.predictor_response_kind == KIND_CONDITIONAL
            && cycle.predictor_response_taken;
        if conditional_redirect {
            assert_eq!(
                cycle.predictor_response_next_pc, 8,
                "the learned conditional redirected to the wrong target",
            );
            assert!(
                !cycle.predictor_request_accept,
                "a request entered on the shadow-cancellation edge",
            );
            if !cancellation_seen {
                target_commits_at_cancel = target_commits;
            }
            cancellation_seen = true;
            expect_target_request = true;
        }

        if cancellation_seen {
            assert!(
                !(cycle.rx_valid && cycle.rx_pc == 4),
                "the canceled fall-through reached RX",
            );
            assert!(
                !(cycle.ex_valid && cycle.ex_pc == 4),
                "the canceled fall-through reached EX",
            );
            assert!(
                !(cycle.cm_valid && cycle.cm_pc == 4),
                "the canceled fall-through reached CM",
            );
        }

        if cancellation_seen && target_request_seen && target_commits > target_commits_at_cancel {
            assert_eq!(driver.value("debug_x31"), target_commits);
            return;
        }
    }

    assert!(
        cancellation_seen,
        "no learned conditional shadow was canceled"
    );
    assert!(
        target_request_seen,
        "the redirect target was never requested"
    );
    panic!("the redirect target never executed after cancellation");
}

#[test]
fn taken_target_preview_preserves_load_dependencies_and_mispredicted_loop_exit() {
    for config in [8, 8200] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_addi(1, 0, 8)),
                (4, encode_addi(3, 0, 0)),
                (8, encode_ld(2, 0, 0)),
                (12, 0x0021_81b3), // add x3, x3, x2
                (16, encode_addi(1, 1, -1)),
                (20, encode_branch(1, 0, 1, -12)),
                (24, encode_addi(31, 3, 0)),
                (28, encode_jal(0, 0)),
                (32, encode_addi(31, 0, 2047)),
            ],
            config,
        );
        driver.set_data_rdata(7);
        let expected = [0, 4]
            .into_iter()
            .chain((0..8).flat_map(|_| [8, 12, 16, 20]))
            .chain([24])
            .collect::<Vec<_>>();
        let mut retired = Vec::new();
        let mut waited = 0;
        let mut held_previews = 0;
        let mut complete = false;
        for _ in 0..1500 {
            let cycle = driver.step();
            if cycle.fetch_pc != cycle.fetch_cursor {
                assert_eq!(config, 8200);
                assert!(
                    !cycle.predictor_request_accept,
                    "preview data acquired the old cursor's token"
                );
                if cycle.memory_wait {
                    held_previews += 1;
                }
            }
            if cycle.memory_wait {
                waited += 1;
                if waited >= 5 {
                    driver.acknowledge_data_request();
                }
            } else {
                waited = 0;
            }
            if cycle.retire && retired.len() < expected.len() {
                assert_eq!(
                    cycle.cm_pc,
                    expected[retired.len()],
                    "retirement order changed for config {config}"
                );
                retired.push(cycle.cm_pc);
            }
            if cycle.commit_valid && cycle.commit_pc == 24 {
                assert_eq!(retired, expected);
                assert_eq!(cycle.commit_rd, 31);
                assert_eq!(
                    cycle.commit_data, 56,
                    "a load result or the final not-taken exit was lost"
                );
                complete = true;
                break;
            }
        }
        assert!(complete, "loop did not finish for config {config}");
        if config == 8200 {
            assert!(
                held_previews > 0,
                "the delayed load never overlapped target preview"
            );
        }
    }
}

#[test]
fn memory_wait_refreshes_only_invalid_payload_slots() {
    let load = encode_ld(2, 0, 0);
    let first = encode_addi(5, 0, 1);
    let second = encode_addi(5, 0, 2);
    let mut driver = CoreDriver::new([(0, load), (4, first)]);
    driver.wait_for_predictor_scrub();
    driver.set_data_response_sequence(0);

    let first_cycle = driver.step();
    assert_eq!(first_cycle.fetch_pc, 0);
    driver.set_instr_valid(false);

    let mut first_wait_payload: Option<(u64, u64)> = None;
    let mut refreshed = false;
    for _ in 0..30 {
        let cycle = driver.step();
        if !cycle.memory_wait || !cycle.cm_is_load {
            continue;
        }

        assert_eq!(driver.value("fetch_queue.valid_q"), 0);
        assert_eq!(driver.value("fetch_queue.lookahead_valid_q"), 0);
        assert_eq!(driver.value("fetch_queue.tail_valid_q"), 0);

        let payload = (
            driver.value("fetch_queue.lookahead_payload_q"),
            driver.value("fetch_queue.tail_payload_q"),
        );
        if let Some(previous) = first_wait_payload {
            assert_ne!(
                payload.0, previous.0,
                "the invalid lookahead payload did not use its native CE"
            );
            assert_ne!(
                payload.1, previous.1,
                "the invalid tail payload did not use its native CE"
            );
            assert_eq!(
                payload.0, payload.1,
                "invalid payload banks did not refresh from the same input"
            );
            refreshed = true;
            break;
        } else {
            first_wait_payload = Some(payload);
            // The cursor is held by memory_wait. Changing the instruction at
            // that same PC makes a refresh observable without making the
            // invalid slot architectural.
            driver.program.insert(cycle.fetch_pc, second);
        }
    }

    assert!(
        refreshed,
        "the load never exposed a second memory-wait cycle for payload refresh"
    );
}

#[test]
fn static_memory_wait_defers_exception_discard() {
    check_exception_during_memory_wait(4);
}

fn check_exception_during_memory_wait(config: u64) {
    let mut driver = CoreDriver::with_config([(0, encode_ld(2, 0, 0)), (4, 0xffff_ffff)], config);
    driver.set_data_response_sequence(0);

    let mut held_cycles = 0;
    let mut valid_payload_snapshot = None;
    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.memory_wait && cycle.ex_valid && cycle.ex_pc == 4 && cycle.ex_exception {
            held_cycles += 1;
            assert!(!cycle.recover);
            assert!(
                !cycle.discard,
                "held exception discarded predictor state early"
            );
            // The shorter static frontend can enter this stall before the
            // tail becomes valid. Only occupied entries must preserve data.
            let current_payload = [
                ("fetch_queue.valid_q", "fetch_queue.payload_q"),
                ("fetch_queue.lookahead_valid_q", "fetch_queue.lookahead_payload_q"),
                ("fetch_queue.tail_valid_q", "fetch_queue.tail_payload_q"),
            ]
            .map(|(valid, payload)| {
                let occupied = driver.value(valid) != 0;
                (occupied, if occupied { driver.value(payload) } else { 0 })
            });
            if let Some(snapshot) = valid_payload_snapshot {
                assert_eq!(
                    current_payload, snapshot,
                    "a valid fetch payload changed while memory_wait held the queue"
                );
            } else {
                if config == 0 {
                    assert!(current_payload.iter().all(|(occupied, _)| *occupied));
                }
                valid_payload_snapshot = Some(current_payload);
            }
            if held_cycles == 3 {
                break;
            }
        }
    }
    assert_eq!(
        held_cycles, 3,
        "load did not hold the younger exception in EX"
    );

    driver.acknowledge_data_request();
    let release = driver.step();
    assert!(!release.memory_wait);
    assert!(release.ex_exception);
    assert!(
        release.discard,
        "exception release did not discard speculative state"
    );
    assert!(!release.recover);

    let following = driver.step();
    assert!(
        !following.discard,
        "exception discard pulsed more than once"
    );
    assert!(!following.recover);
}

#[test]
fn static_memory_wait_defers_redirect_recovery() {
    check_mismatch_during_memory_wait(4);
}

fn check_mismatch_during_memory_wait(config: u64) {
    let mut driver =
        CoreDriver::with_config([(0, encode_ld(2, 0, 0)), (4, encode_jal(1, 12))], config);
    driver.set_data_response_sequence(0);

    let mut held_cycles = 0;
    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.memory_wait
            && cycle.ex_valid
            && cycle.ex_pc == 4
            && (cycle.ex_mismatch || config & 8 != 0)
        {
            held_cycles += 1;
            assert_eq!(cycle.ex_kind, KIND_CALL);
            assert!(
                !cycle.recover,
                "held mismatch recovered predictor state early"
            );
            assert!(!cycle.discard);
            if held_cycles == 3 {
                break;
            }
        }
    }
    assert_eq!(
        held_cycles, 3,
        "load did not hold the younger mismatch in EX"
    );

    driver.acknowledge_data_request();
    let release = driver.step();
    assert!(!release.memory_wait);
    if config & 8 != 0 {
        assert!(!release.ex_mismatch);
        assert!(
            !release.recover,
            "compact call recovered before reaching CM"
        );
        let resolution = driver.step();
        assert!(resolution.cm_valid && resolution.cm_pc == 4);
        assert!(resolution.recover && resolution.branch_redirect && resolution.retire);
    } else {
        assert!(release.ex_mismatch);
        assert!(
            release.recover,
            "mismatch release did not recover speculative state"
        );
    }
    assert!(!release.discard);

    let following = driver.step();
    assert!(
        !following.recover,
        "mismatch recovery pulsed more than once"
    );
    assert!(!following.discard);
}

#[test]
fn static_correct_fallthrough_does_not_flush_for_control_kind_or_direction() {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 1)),
            (4, encode_branch(1, 1, 1, 12)), // BNE not taken.
            (8, encode_jal(0, 4)),           // Taken, but next PC is fallthrough.
            (12, encode_branch(1, 1, 0, 4)), // BEQ taken to fallthrough.
            (16, encode_addi(3, 0, 7)),
        ],
        4,
    );
    let mut reached_target = false;
    for _ in 0..120 {
        let cycle = driver.step();
        assert!(
            !cycle.branch_redirect && !cycle.ex_mismatch && !cycle.recover,
            "correct static next PC caused a prediction repair"
        );
        if cycle.commit_valid && cycle.commit_pc == 16 {
            assert_eq!((cycle.commit_rd, cycle.commit_data), (3, 7));
            reached_target = true;
            break;
        }
    }
    assert!(reached_target, "fallthrough sequence did not commit");
}

#[test]
fn static_jalr_to_fallthrough_redirects_without_duplicate_retirement() {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(2, 0, 8)),
            (4, (2 << 15) | (1 << 7) | 0x67), // JALR x1, 0(x2), target = PC + 4.
            (8, encode_addi(3, 1, 7)),
            (12, encode_jal(0, 0)),
        ],
        4,
    );
    let mut commits = Vec::new();
    let mut repairs = 0;
    for _ in 0..120 {
        let cycle = driver.step();
        if cycle.ex_valid && cycle.ex_pc == 4 && cycle.ex_mismatch {
            repairs += 1;
        }
        if cycle.commit_valid && cycle.commit_pc < 12 {
            commits.push(cycle.commit_pc);
            if cycle.commit_pc == 8 {
                assert_eq!((cycle.commit_rd, cycle.commit_data), (3, 15));
            }
        }
    }
    assert_eq!(commits, [0, 4, 8]);
    assert_eq!(repairs, 1);
}

#[test]
fn static_redirect_drops_wrong_path_memory_and_csr_side_effects() {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_jal(0, 16)),
            (4, encode_sd(1, 0, 0)),
            (8, encode_csrrwi(0, 0x340, 17)),
            (12, encode_ld(2, 0, 0)),
            (16, encode_addi(3, 0, 7)),
            (20, encode_jal(0, 0)),
        ],
        4,
    );
    let mut reached_target = false;
    for _ in 0..120 {
        let cycle = driver.step();
        assert!(!cycle.data_we, "wrong-path store reached memory");
        if cycle.commit_valid {
            assert!(!matches!(cycle.commit_pc, 4 | 8 | 12));
            if cycle.commit_pc == 16 {
                assert_eq!((cycle.commit_rd, cycle.commit_data), (3, 7));
                reached_target = true;
            }
        }
    }
    assert!(reached_target, "redirect target did not commit");
    assert_eq!(driver.csr_value("mscratch_q"), 0);
}

fn check_stale_load(config: u64) {
    // First learn a direct-control BTB entry at PC zero. Replacing that
    // instruction with a load models stale predictor state after executable
    // memory changes: the load itself owns the next-PC repair, but it must
    // still wait for its synchronous data response and retire that response.
    let mut driver = direct_trained_driver(config);
    driver.program.insert(0, encode_ld(31, 0, 0));
    let load_value = 0x0123_4567_89ab_cdef;
    let mut acknowledged = false;

    for _ in 0..100 {
        let cycle = driver.step();
        if (if config & 8 != 0 {
            cycle.branch_redirect
        } else {
            cycle.redirect_pending
        }) && cycle.cm_valid
            && cycle.cm_pc == 0
            && cycle.cm_is_load
        {
            assert!(
                cycle.memory_wait,
                "the load's own prediction repair bypassed its pending memory response"
            );
            driver.set_data_rdata(load_value);
            driver.acknowledge_data_request();
            acknowledged = true;
            break;
        }
    }
    assert!(
        acknowledged,
        "the stale direct prediction never reached the load repair window"
    );

    for _ in 0..20 {
        let cycle = driver.step();
        if cycle.commit_valid && cycle.commit_pc == 0 {
            assert_eq!(cycle.commit_rd, 31, "the repaired load wrote the wrong rd");
            assert_eq!(
                cycle.commit_data, load_value,
                "the repaired load retired stale response data"
            );
            assert_eq!(
                driver.value("debug_x31"),
                load_value,
                "the repaired load did not update its architectural register"
            );
            return;
        }
    }
    panic!("the repaired load never retired after its response");
}

fn check_stale_store(config: u64) {
    let mut driver = direct_trained_driver(config);
    driver.program.insert(0, encode_sd(0, 0, 0));

    let mut acknowledged = false;
    for _ in 0..100 {
        let cycle = driver.step();
        if (if config & 8 != 0 {
            cycle.branch_redirect
        } else {
            cycle.redirect_pending
        }) && cycle.cm_valid
            && cycle.cm_pc == 0
            && cycle.cm_is_store
        {
            assert!(
                cycle.memory_wait,
                "the repaired store did not wait for its response"
            );
            assert!(
                cycle.data_we,
                "the repaired store suppressed its physical write"
            );
            driver.acknowledge_data_request();
            acknowledged = true;
            break;
        }
    }
    assert!(
        acknowledged,
        "the stale direct prediction never reached the store repair window"
    );

    for _ in 0..20 {
        let cycle = driver.step();
        if cycle.commit_valid && cycle.commit_pc == 0 {
            assert!(
                cycle.commit_mem_we,
                "the repaired store retired without its store payload"
            );
            return;
        }
    }
    panic!("the repaired store never retired after its response");
}

fn check_stale_csr(config: u64) {
    let mut driver = direct_trained_driver(config);
    driver.program.insert(0, encode_csrrwi(0, 0x340, 7));

    for _ in 0..100 {
        let cycle = driver.step();
        if (if config & 8 != 0 {
            cycle.branch_redirect
        } else {
            cycle.redirect_pending
        }) && cycle.cm_valid
            && cycle.cm_pc == 0
            && cycle.cm_is_csr
        {
            assert_eq!(
                driver.csr_value("mscratch_q"),
                7,
                "the CSR owner's prediction repair suppressed its write"
            );
            return;
        }
    }
    panic!("the stale direct prediction never reached the CSR repair window");
}

#[test]
fn fence_i_waits_for_the_older_store_then_retires_once_and_restarts_at_fallthrough() {
    for config in [4, 8, 24] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_sd(0, 0, 0)),
                (4, 0xffff_9f8f), // FENCE.I: reserved fields must be ignored.
                (8, encode_addi(5, 0, 17)),
                (12, encode_jal(0, 0)),
            ],
            config,
        );
        let mut wait_cycles = 0;
        let mut flushed = 0;
        let mut commits = Vec::new();
        for _ in 0..180 {
            if wait_cycles >= 12 {
                driver.acknowledge_data_request();
            }
            let cycle = driver.step();
            if cycle.memory_wait {
                wait_cycles += 1;
                assert!(
                    !cycle.instruction_flush,
                    "FENCE.I escaped an older LSU wait"
                );
            }
            if cycle.instruction_flush {
                assert!(wait_cycles >= 12);
                assert_eq!(cycle.cm_pc, 4);
                assert!(cycle.retire);
                flushed += 1;
            }
            if cycle.commit_valid && cycle.commit_pc < 12 {
                commits.push(cycle.commit_pc);
                if cycle.commit_pc == 8 {
                    assert_eq!(flushed, 1);
                    assert_eq!(cycle.commit_rd, 5);
                    assert_eq!(cycle.commit_data, 17);
                }
            }
        }
        assert_eq!(flushed, 1, "configuration {config}");
        assert_eq!(commits, [0, 4, 8], "configuration {config}");
    }
}

#[test]
fn wrong_path_fence_i_neither_invalidates_nor_retires() {
    for config in [4, 8, 24] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_branch(0, 0, 0, 8)),
                (4, 0x0000_100f),
                (8, encode_addi(5, 0, 23)),
                (12, encode_jal(0, 0)),
            ],
            config,
        );
        let mut committed_target = false;
        for _ in 0..100 {
            let cycle = driver.step();
            assert!(
                !cycle.instruction_flush,
                "wrong-path FENCE.I invalidated the cache"
            );
            if cycle.commit_valid {
                assert_ne!(cycle.commit_pc, 4);
                if cycle.commit_pc == 8 {
                    assert_eq!(cycle.commit_data, 23);
                    committed_target = true;
                }
            }
        }
        assert!(committed_target, "configuration {config}");
    }
}

#[test]
fn compact_memory_wait_defers_exception_and_redirect() {
    for config in [8, 24] {
        check_exception_during_memory_wait(config);
        check_mismatch_during_memory_wait(config);
    }
}

#[test]
fn compact_stale_predictions_preserve_load_store_and_csr_effects() {
    for config in [8, 24] {
        check_stale_load(config);
        check_stale_store(config);
        check_stale_csr(config);
    }
}

fn encode_muldiv(rd: u32, rs1: u32, rs2: u32, operation: u32) -> u32 {
    1 << 25 | rs2 << 20 | rs1 << 15 | operation << 12 | rd << 7 | 0x33
}

#[test]
fn muldiv_holds_younger_memory_and_releases_dependencies_exactly_once() {
    for config in [132, 136, 392, EARLY_DIVIDE_CONFIG] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_addi(1, 0, 123)),
                (4, encode_addi(2, 0, 7)),
                (8, encode_muldiv(3, 1, 2, 0)),
                (12, encode_muldiv(4, 3, 2, 4)),
                (16, encode_sd(4, 0, 256)),
                (20, encode_addi(31, 4, 1)),
                (24, encode_jal(0, 0)),
            ],
            config,
        );
        let mut commits = BTreeMap::<u32, (usize, u64)>::new();
        let mut held = 0;
        for _ in 0..350 {
            let mul_wait = driver.value("muldiv_wait") != 0;
            let cycle = driver.step();
            if mul_wait {
                held += 1;
                assert!(!cycle.data_we, "younger store escaped during M execution");
            }
            if cycle.memory_wait {
                driver.acknowledge_data_request();
            }
            if cycle.commit_valid && cycle.commit_pc <= 20 {
                let entry = commits.entry(cycle.commit_pc).or_default();
                entry.0 += 1;
                entry.1 = cycle.commit_data;
            }
        }
        assert!(
            held >= if config & 16_384 != 0 {
                20
            } else if config & 256 != 0 {
                75
            } else {
                90
            }
        );
        assert_eq!(commits[&8], (1, 861));
        assert_eq!(commits[&12], (1, 123));
        assert_eq!(commits[&16].0, 1);
        assert_eq!(commits[&20], (1, 124));
    }
}

#[test]
fn older_branch_squashes_muldiv_and_younger_csr_store() {
    let mut exercised_speculative_start_and_cancel = false;
    for config in [132, 136, 392, EARLY_DIVIDE_CONFIG] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_addi(1, 0, 7)),
                (4, encode_branch(1, 1, 0, 20)),
                (8, encode_muldiv(31, 1, 0, 4)),
                (12, encode_csrrwi(0, 0x340, 9)),
                (16, encode_sd(1, 0, 256)),
                (20, encode_addi(31, 0, 99)),
                (24, encode_muldiv(31, 1, 1, 0)),
                (28, encode_jal(0, 0)),
            ],
            config,
        );
        let mut result = None;
        let mut wrong_path_started = false;
        let mut wrong_path_cancelled = false;
        for _ in 0..200 {
            let cycle = driver.step();
            wrong_path_started |= cycle.muldiv_start && cycle.ex_valid && cycle.ex_pc == 8;
            wrong_path_cancelled |= cycle.muldiv_cancel && cycle.cm_valid && cycle.cm_pc == 8;
            assert!(!cycle.data_we);
            if cycle.commit_valid {
                assert!(!(8..24).contains(&cycle.commit_pc));
                if cycle.commit_pc == 24 {
                    result = Some(cycle.commit_data);
                }
            }
        }
        assert_eq!(result, Some(49));
        assert_eq!(driver.csr_value("mscratch_q"), 0);
        exercised_speculative_start_and_cancel |= wrong_path_started && wrong_path_cancelled;
    }
    assert!(exercised_speculative_start_and_cancel);
}

#[test]
fn muldiv_defers_a_younger_exception_until_its_result_retires() {
    for config in [132, 136, 392, EARLY_DIVIDE_CONFIG] {
        let mut driver = CoreDriver::with_config(
            [
                (0, encode_addi(1, 0, 7)),
                (4, encode_muldiv(31, 1, 1, 0)),
                (8, 0xffff_ffff),
            ],
            config,
        );
        let mut multiply_committed = false;
        let mut trap = false;
        for _ in 0..200 {
            let cycle = driver.step();
            if driver.value("muldiv_wait") != 0 {
                assert_eq!(driver.csr_value("mcause_q"), 0);
            }
            if cycle.commit_valid && cycle.commit_pc == 4 {
                assert_eq!(cycle.commit_data, 49);
                multiply_committed = true;
            }
            if driver.csr_value("mcause_q") == 2 {
                assert!(
                    multiply_committed
                        || (driver.value("commit_valid") != 0
                            && driver.value("commit_pc") == 4
                            && driver.value("commit_data") == 49)
                );
                assert_eq!(driver.value("debug_x31"), 49);
                assert_eq!(driver.csr_value("mepc_q") << 1, 8);
                trap = true;
                break;
            }
        }
        assert!(trap);
    }
}

#[test]
fn stale_prediction_on_muldiv_preserves_its_pending_result() {
    check_stale_prediction_on_divide(136);
    check_stale_prediction_on_divide(EARLY_DIVIDE_CONFIG);
}

fn check_stale_prediction_on_divide(config: u64) {
    let mut driver = direct_trained_driver(config);
    driver.program.insert(0, encode_muldiv(31, 0, 0, 5)); // DIVU by zero.
    let mut repaired = false;
    for _ in 0..240 {
        let cycle = driver.step();
        if cycle.branch_redirect && cycle.cm_valid && cycle.cm_pc == 0 {
            repaired = true;
        }
        if cycle.commit_valid && cycle.commit_pc == 0 && cycle.commit_rd == 31 {
            assert!(repaired, "the stale M owner did not repair its prediction");
            assert_eq!(cycle.commit_data, u64::MAX);
            return;
        }
    }
    panic!("the repaired M owner never retired its result");
}

#[test]
fn misa_reports_m_only_when_enabled() {
    for config in [8, 136] {
        let mut driver = CoreDriver::with_config(
            [(0, 0x3010_2ff3), (4, encode_jal(0, 0))], // CSRRS x31, misa, x0.
            config,
        );
        let expected = 0x8000_0000_0000_0104 | if config & 128 != 0 { 1 << 12 } else { 0 };
        let mut result = None;
        for _ in 0..180 {
            let cycle = driver.step();
            if cycle.commit_valid && cycle.commit_pc == 0 {
                result = Some(cycle.commit_data);
                break;
            }
        }
        assert_eq!(result, Some(expected));
    }
}

fn encode_zba(rd: u32, rs1: u32, rs2: u32, shift: u32, uw: bool) -> u32 {
    let funct7 = if shift == 0 { 4 } else { 16 };
    funct7 << 25
        | rs2 << 20
        | rs1 << 15
        | (shift * 2) << 12
        | rd << 7
        | if uw { 0x3b } else { 0x33 }
}

#[test]
fn zba_forwards_both_operands_and_waits_for_slli_uw_completion() {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, -1)),
            (4, encode_addi(2, 0, 7)),
            (8, encode_zba(3, 1, 2, 3, true)),
            (12, encode_zba(4, 3, 3, 1, false)),
            (16, 0x0800_101b | 33 << 20 | 4 << 15 | 5 << 7),
            (20, encode_zba(6, 5, 4, 0, true)),
            (24, encode_sd(6, 0, 256)),
            (28, encode_jal(0, 0)),
        ],
        520,
    );
    let mut commits = BTreeMap::<u32, (usize, u64)>::new();
    for _ in 0..160 {
        let cycle = driver.step();
        if cycle.memory_wait {
            driver.acknowledge_data_request();
        }
        if cycle.commit_valid && cycle.commit_pc <= 24 {
            let entry = commits.entry(cycle.commit_pc).or_default();
            entry.0 += 1;
            entry.1 = cycle.commit_data;
        }
    }
    let a = (u64::from(u32::MAX) << 3) + 7;
    let b = a * 3;
    assert_eq!(commits[&8], (1, a));
    assert_eq!(commits[&12], (1, b));
    assert_eq!(commits[&16], (1, u64::from(b as u32) << 33));
    assert_eq!(commits[&20], (1, b));
    assert_eq!(commits[&24].0, 1);
}

#[test]
fn zba_wrong_path_does_not_commit_and_disabled_zba_traps_precisely() {
    let instruction = encode_zba(31, 1, 1, 2, true);
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 7)),
            (4, encode_branch(1, 1, 0, 12)),
            (8, instruction),
            (12, encode_sd(31, 0, 256)),
            (16, encode_zba(31, 1, 1, 1, true)),
            (20, encode_jal(0, 0)),
        ],
        520,
    );
    let mut result = None;
    for _ in 0..120 {
        let cycle = driver.step();
        assert!(!cycle.data_we);
        if cycle.commit_valid {
            assert!(![8, 12].contains(&cycle.commit_pc));
            if cycle.commit_pc == 16 {
                result = Some(cycle.commit_data);
            }
        }
    }
    assert_eq!(result, Some(21));
    let mut driver = CoreDriver::with_config([(0, encode_addi(1, 0, 7)), (4, instruction)], 8);
    let mut trapped = false;
    for _ in 0..100 {
        let cycle = driver.step();
        assert!(!(cycle.commit_valid && cycle.commit_pc == 4));
        if driver.csr_value("mcause_q") == 2 {
            assert_eq!(driver.csr_value("mepc_q") << 1, 4);
            assert_eq!(driver.csr_value("mtval_q"), u64::from(instruction));
            trapped = true;
            break;
        }
    }
    assert!(trapped);
}

#[test]
fn system_trap_flushes_a_younger_muldiv_before_it_can_start() {
    for config in [132, 136, 392, EARLY_DIVIDE_CONFIG] {
        for trap in [0x0000_0073, 0x0010_0073] {
            let mut driver = CoreDriver::with_config(
                (0..8).map(|i| (i * 4, NOP)).chain([
                    (32, encode_addi(1, 0, 7)),
                    (36, trap),
                    (40, encode_muldiv(31, 1, 1, 0)),
                    (44, encode_sd(31, 0, 256)),
                ]),
                config,
            );
            for _ in 0..140 {
                // Unlike branch repair, a system instruction already flushes
                // younger EX work before its CM redirect is issued.
                assert_eq!(driver.value("muldiv_start"), 0);
                let cycle = driver.step();
                assert!(!cycle.data_we);
                assert!(!(cycle.commit_valid && [40, 44].contains(&cycle.commit_pc)));
                assert_eq!(driver.value("debug_x31"), 0);
            }
            assert_eq!(driver.csr_value("mepc_q") << 1, 36);
            assert_eq!(
                driver.csr_value("mcause_q"),
                if trap == 0x73 { 11 } else { 3 }
            );
        }
    }
}

fn exercise_zbb_completion_forwards_both_inputs_and_survives_memory_wait(config: u64) {
    let reg = |base: u32, rd: u32, a: u32, b: u32| base | rd << 7 | a << 15 | b << 20;
    let unary = |base: u32, rd: u32, a: u32| base | rd << 7 | a << 15;
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, -128)),
            (4, unary(0x60401013, 2, 1)),   // sext.b: -128
            (8, unary(0x60201013, 3, 2)),   // cpop: 57
            (12, reg(0x60001033, 4, 3, 3)), // rol(57,57)
            (16, encode_sd(4, 0, 256)),
            (20, unary(0x60001013, 5, 4)),  // clz: 1
            (24, reg(0x0a004033, 6, 2, 5)), // min(-128,1)
            (28, unary(0x6010101b, 7, 6)),  // ctzw: 7
            (32, reg(0x6000503b, 8, 6, 7)), // rorw(-128,7): 0x01ffffff
            (36, encode_zba(9, 8, 7, 2, true)),
            (40, encode_jal(0, 0)),
        ],
        config,
    );
    let mut commits = BTreeMap::<u32, (usize, u64)>::new();
    let mut waits = 0;
    for _ in 0..220 {
        let cycle = driver.step();
        if cycle.memory_wait {
            waits += 1;
            if waits >= 8 {
                driver.acknowledge_data_request();
            }
        }
        if cycle.commit_valid && cycle.commit_pc <= 36 {
            let entry = commits.entry(cycle.commit_pc).or_default();
            entry.0 += 1;
            entry.1 = cycle.commit_data;
        }
    }
    assert!(waits >= 8);
    for (pc, expected) in [
        (4, (-128i64) as u64),
        (8, 57),
        (12, 57u64.rotate_left(57)),
        (20, 1),
        (24, (-128i64) as u64),
        (28, 7),
        (32, 0x01ffffff),
        (36, 0x08000003),
    ] {
        assert_eq!(commits[&pc], (1, expected), "pc={pc}");
    }
    assert_eq!(commits[&16].0, 1);
}

#[test]
fn zbb_completion_forwards_both_inputs_and_survives_memory_wait() {
    exercise_zbb_completion_forwards_both_inputs_and_survives_memory_wait(1544);
}

#[test]
fn early_zbb_completion_forwards_both_inputs_and_survives_memory_wait() {
    exercise_zbb_completion_forwards_both_inputs_and_survives_memory_wait(5640);
}

#[test]
fn zbb_redirect_discards_partial_results_and_disabled_instruction_traps() {
    let instruction = 0x60201013 | 1 << 15 | 31 << 7;
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 7)),
            (4, encode_branch(1, 1, 0, 12)),
            (8, instruction),
            (12, encode_sd(31, 0, 256)),
            (16, 0x60001013 | 1 << 15 | 31 << 7),
            (20, encode_jal(0, 0)),
        ],
        1544,
    );
    let mut result = None;
    for _ in 0..120 {
        let cycle = driver.step();
        assert!(!cycle.data_we);
        if cycle.commit_valid {
            assert!(![8, 12].contains(&cycle.commit_pc));
            if cycle.commit_pc == 16 {
                result = Some(cycle.commit_data);
            }
        }
    }
    assert_eq!(result, Some(61));
    for instruction in [instruction, 0x6200501b | 1 << 15 | 31 << 7] {
        // The first instruction is Zbb-disabled; the second is reserved even
        // when Zbb is enabled (roriw shamt[5] must be zero).
        let config = if instruction & 0xfe00707f == 0x6200501b {
            1544
        } else {
            520
        };
        let mut driver =
            CoreDriver::with_config([(0, encode_addi(1, 0, 7)), (4, instruction)], config);
        let mut trapped = false;
        for _ in 0..100 {
            let cycle = driver.step();
            assert!(!(cycle.commit_valid && cycle.commit_pc == 4));
            if driver.csr_value("mcause_q") == 2 {
                assert_eq!(driver.csr_value("mepc_q") << 1, 4);
                assert_eq!(driver.csr_value("mtval_q"), u64::from(instruction));
                trapped = true;
                break;
            }
        }
        assert!(trapped);
    }
}

fn exercise_zbb_results_cross_muldiv_and_csr_boundaries(config: u64) {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 13)),
            (4, encode_addi(2, 0, 7)),
            (8, encode_muldiv(3, 1, 2, 0)),                 // MUL = 91
            (12, 0x60201013 | 3 << 15 | 4 << 7),            // CPOP = 5
            (16, 0x60001013 | 4 << 15 | 5 << 7),            // CLZ = 61
            (20, encode_muldiv(6, 4, 5, 0)),                // MUL = 305
            (24, encode_muldiv(7, 6, 4, 5)),                // DIVU = 61
            (28, 0x0a007033 | 4 << 20 | 7 << 15 | 8 << 7),  // MAXU = 61
            (32, 0x34001073 | 8 << 15),                     // CSRRW mscratch, x8
            (36, 0x34002073 | 9 << 7),                      // CSRRS x9, mscratch, x0
            (40, 0x6000503b | 2 << 20 | 6 << 15 | 10 << 7), // RORW(305,7)
            (44, encode_sd(10, 0, 256)),
            (48, encode_jal(0, 0)),
        ],
        config,
    );
    let mut commits = BTreeMap::<u32, (usize, u64)>::new();
    for _ in 0..400 {
        let cycle = driver.step();
        if cycle.memory_wait {
            driver.acknowledge_data_request();
        }
        if cycle.commit_valid && cycle.commit_pc <= 44 {
            let entry = commits.entry(cycle.commit_pc).or_default();
            entry.0 += 1;
            entry.1 = cycle.commit_data;
        }
    }
    for (pc, value) in [
        (8, 91),
        (12, 5),
        (16, 61),
        (20, 305),
        (24, 61),
        (28, 61),
        (36, 61),
        (40, 0x62000002),
    ] {
        assert_eq!(commits[&pc], (1, value), "pc={pc}");
    }
    assert_eq!(commits[&44].0, 1);
}

#[test]
fn zbb_results_cross_muldiv_and_csr_boundaries() {
    exercise_zbb_results_cross_muldiv_and_csr_boundaries(1928);
}

#[test]
fn early_zbb_results_cross_muldiv_and_csr_boundaries() {
    exercise_zbb_results_cross_muldiv_and_csr_boundaries(6024);
}

fn exercise_zbb_forwarding_covers_instruction_gaps_both_operands_and_younger_writers(config: u64) {
    for gap in 0..=3 {
        for reverse in [false, true] {
            for overwrite in [false, true] {
                if overwrite && gap == 0 {
                    continue;
                }
                let mut program = (0..8).map(|i| (i * 4, NOP)).collect::<Vec<_>>();
                program.extend([
                    (32, encode_addi(1, 0, 7)),
                    (36, 0x60201013 | 1 << 15 | 2 << 7),
                ]);
                for i in 0..gap {
                    program.push((
                        40 + i * 4,
                        if overwrite && i == 0 {
                            encode_addi(2, 0, 99)
                        } else {
                            NOP
                        },
                    ));
                }
                let pc = 40 + gap * 4;
                let (a, b) = if reverse { (1, 2) } else { (2, 1) };
                program.extend([
                    (pc, 0x40004033 | a << 15 | b << 20 | 31 << 7),
                    (pc + 4, encode_jal(0, 0)),
                ]);
                let mut driver = CoreDriver::with_config(program, config);
                let mut result = None;
                for _ in 0..180 {
                    let cycle = driver.step();
                    if cycle.commit_valid && cycle.commit_pc == pc {
                        result = Some(cycle.commit_data);
                    }
                }
                assert_eq!(
                    result,
                    Some(!((if overwrite { 99u64 } else { 3 }) ^ 7)),
                    "gap={gap} reverse={reverse} overwrite={overwrite}"
                );
            }
        }
    }
}

#[test]
fn zbb_forwarding_covers_instruction_gaps_both_operands_and_younger_writers() {
    exercise_zbb_forwarding_covers_instruction_gaps_both_operands_and_younger_writers(1544);
}

#[test]
fn early_zbb_forwarding_covers_instruction_gaps_both_operands_and_younger_writers() {
    exercise_zbb_forwarding_covers_instruction_gaps_both_operands_and_younger_writers(5640);
}

fn exercise_zbs_forwarding_covers_instruction_gaps_inputs_and_younger_writers(config: u64) {
    for gap in 0..=3 {
        for reverse in [false, true] {
            for overwrite in [false, true] {
                if overwrite && gap == 0 {
                    continue;
                }
                let mut program = (0..8).map(|i| (i * 4, NOP)).collect::<Vec<_>>();
                program.extend([
                    (32, encode_addi(1, 0, 7)),
                    (36, 0x48001013 | 1 << 20 | 1 << 15 | 2 << 7),
                ]); // bclri x2,x1,1 = 5
                for i in 0..gap {
                    program.push((
                        40 + i * 4,
                        if overwrite && i == 0 {
                            encode_addi(2, 0, 99)
                        } else {
                            NOP
                        },
                    ));
                }
                let pc = 40 + gap * 4;
                let (a, b) = if reverse { (1, 2) } else { (2, 1) };
                program.extend([
                    (pc, 0x68001033 | a << 15 | b << 20 | 31 << 7),
                    (pc + 4, encode_jal(0, 0)),
                ]);
                // Zbs is independently usable with Zbb and Zba disabled.
                let mut driver = CoreDriver::with_config(program, config);
                let mut result = None;
                for _ in 0..180 {
                    let cycle = driver.step();
                    if cycle.commit_valid && cycle.commit_pc == pc {
                        result = Some(cycle.commit_data);
                    }
                }
                let produced = if overwrite { 99u64 } else { 5 };
                let expected = if reverse {
                    7 ^ (1 << (produced & 63))
                } else {
                    produced ^ (1 << 7)
                };
                assert_eq!(
                    result,
                    Some(expected),
                    "gap={gap} reverse={reverse} overwrite={overwrite}"
                );
            }
        }
    }
}

#[test]
fn zbs_forwarding_covers_instruction_gaps_inputs_and_younger_writers() {
    exercise_zbs_forwarding_covers_instruction_gaps_inputs_and_younger_writers(2056);
}

#[test]
fn early_zbs_forwarding_covers_instruction_gaps_inputs_and_younger_writers() {
    exercise_zbs_forwarding_covers_instruction_gaps_inputs_and_younger_writers(6152);
}

fn exercise_zbs_results_cross_zbb_muldiv_csr_and_memory_wait(config: u64) {
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 13)),
            (4, encode_addi(2, 0, 7)),
            (8, encode_muldiv(3, 1, 2, 0)),                 // 91
            (12, 0x48001013 | 6 << 20 | 3 << 15 | 4 << 7),  // bclri = 27
            (16, 0x60201013 | 4 << 15 | 5 << 7),            // cpop = 4
            (20, 0x28001033 | 5 << 20 | 5 << 15 | 6 << 7),  // bset = 20
            (24, encode_muldiv(7, 6, 5, 5)),                // divu = 5
            (28, 0x68001013 | 63 << 20 | 7 << 15 | 8 << 7), // binvi = 0x8000000000000005
            (32, encode_sd(8, 0, 256)),
            (36, 0x48005033 | 2 << 20 | 8 << 15 | 9 << 7), // bext = 0
            (40, 0x48005013 | 63 << 20 | 8 << 15 | 10 << 7), // bexti = 1
            (44, 0x34001073 | 10 << 15),
            (48, 0x34002073 | 11 << 7),
            (52, encode_zba(12, 11, 7, 2, true)), // sh2add.uw = 9
            (56, encode_jal(0, 0)),
        ],
        config,
    );
    let mut commits = BTreeMap::<u32, (usize, u64)>::new();
    let mut waits = 0;
    for _ in 0..500 {
        let cycle = driver.step();
        if cycle.memory_wait {
            waits += 1;
            if waits >= 10 {
                driver.acknowledge_data_request();
            }
        }
        if cycle.commit_valid && cycle.commit_pc <= 52 {
            let entry = commits.entry(cycle.commit_pc).or_default();
            entry.0 += 1;
            entry.1 = cycle.commit_data;
        }
    }
    assert!(waits >= 10);
    for (pc, expected) in [
        (8, 91),
        (12, 27),
        (16, 4),
        (20, 20),
        (24, 5),
        (28, 0x8000000000000005),
        (36, 0),
        (40, 1),
        (48, 1),
        (52, 9),
    ] {
        assert_eq!(commits[&pc], (1, expected), "pc={pc}");
    }
    assert_eq!(commits[&32].0, 1);
}

#[test]
fn zbs_results_cross_zbb_muldiv_csr_and_memory_wait() {
    exercise_zbs_results_cross_zbb_muldiv_csr_and_memory_wait(3976);
}

#[test]
fn early_zbs_results_cross_zbb_muldiv_csr_and_memory_wait() {
    exercise_zbs_results_cross_zbb_muldiv_csr_and_memory_wait(8072);
}

#[test]
fn zbs_redirect_disabled_and_word_encoding_traps_are_precise() {
    let instruction = 0x28001013 | 63 << 20 | 1 << 15 | 31 << 7;
    let mut driver = CoreDriver::with_config(
        [
            (0, encode_addi(1, 0, 7)),
            (4, encode_branch(1, 1, 0, 12)),
            (8, instruction),
            (12, encode_sd(31, 0, 256)),
            (16, 0x48005013 | 2 << 20 | 1 << 15 | 31 << 7),
            (20, encode_jal(0, 0)),
        ],
        3592,
    );
    let mut result = None;
    for _ in 0..120 {
        let cycle = driver.step();
        assert!(!cycle.data_we);
        if cycle.commit_valid {
            assert!(![8, 12].contains(&cycle.commit_pc));
            if cycle.commit_pc == 16 {
                result = Some(cycle.commit_data);
            }
        }
    }
    assert_eq!(result, Some(1));
    for (instruction, config) in [
        (instruction, 1544),
        (instruction | 8, 3592),
        (0x4800103b | 2 << 20 | 1 << 15 | 31 << 7, 3592),
        (0x2c001033 | 2 << 20 | 1 << 15 | 31 << 7, 3592),
    ] {
        let mut driver =
            CoreDriver::with_config([(0, encode_addi(1, 0, 7)), (4, instruction)], config);
        let mut trapped = false;
        for _ in 0..100 {
            let cycle = driver.step();
            assert!(!(cycle.commit_valid && cycle.commit_pc == 4));
            if driver.csr_value("mcause_q") == 2 {
                assert_eq!(driver.csr_value("mepc_q") << 1, 4);
                assert_eq!(driver.csr_value("mtval_q"), u64::from(instruction));
                trapped = true;
                break;
            }
        }
        assert!(trapped, "config={config} instr={instruction:08x}");
    }
}

#[test]
fn misa_b_is_set_only_when_zba_zbb_zbs_are_all_enabled() {
    for config in [8, 520, 1032, 1544, 2056, 2568, 3080, 3592, 3976] {
        let mut driver =
            CoreDriver::with_config([(0, 0x30102073 | 1 << 7), (4, encode_jal(0, 0))], config);
        let mut result = None;
        for _ in 0..100 {
            let cycle = driver.step();
            if cycle.commit_valid && cycle.commit_pc == 0 {
                result = Some(cycle.commit_data);
            }
        }
        let expected = 0x8000000000000104
            | if config & 128 != 0 { 1 << 12 } else { 0 }
            | if config & 3584 == 3584 { 2 } else { 0 };
        assert_eq!(result, Some(expected), "config={config}");
    }
}

fn encode_csr(rd: u32, csr: u32, source: u32, funct3: u32) -> u32 {
    csr << 20 | source << 15 | funct3 << 12 | rd << 7 | 0x73
}

fn access_program() -> BTreeMap<u32, u32> {
    [
        (0, encode_addi(1, 0, 0x400)),
        (4, encode_csr(0, 0x305, 1, 1)),
        (0x400, encode_csr(20, 0x342, 0, 2)),
        (0x404, encode_csr(21, 0x341, 0, 2)),
        (0x408, encode_csr(22, 0x343, 0, 2)),
        (0x40c, encode_jal(0, 0)),
    ]
    .into_iter()
    .collect()
}

fn append_constant(program: &mut BTreeMap<u32, u32>, pc: &mut u32, rd: u32, value: u64) {
    program.insert(*pc, encode_addi(rd, 0, 0));
    *pc += 4;
    for byte in value.to_be_bytes() {
        program.insert(*pc, encode_slli(rd, rd, 8));
        program.insert(*pc + 4, encode_addi(rd, rd, i32::from(byte)));
        *pc += 8;
    }
}

#[test]
fn access_faulting_loads_and_stores_never_issue_or_commit() {
    for config in [ACCESS_CONFIG, 98_436] {
        for (address, fault_address, sizes) in [
            (0x1000_u64, 0x1000, 0..4),
            (0x8003_f000, 0x8003_f000, 0..4),
            (0x0000_0001_0000_0300, 0x0000_0001_0000_0300, 0..4),
            (0xffff_ffff_8000_0300, 0xffff_ffff_8000_0300, 0..4),
            (0xfff, 0x1000, 1..4),
            (0x8003_efff, 0x8003_f000, 1..4),
            (0x0200_4001, 0x0200_4001, 1..4),
        ] {
            // Exercise the separately registered last-byte offset across
            // its sign change and beyond the signed 12-bit positive limit.
            for offset in [-2048_i32, -1, 0, 2047] {
                for size in sizes.clone() {
                    for store in [false, true] {
                        let mut program = access_program();
                        let mut pc = 8;
                        append_constant(
                            &mut program,
                            &mut pc,
                            2,
                            address.wrapping_sub(offset as u64),
                        );
                        program.insert(pc, encode_addi(31, 0, 77));
                        let fault_pc = pc + 4;
                        let instruction = if store {
                            (encode_sd(31, 2, offset) & !(7 << 12)) | size << 12
                        } else {
                            (encode_ld(31, 2, offset) & !(7 << 12)) | size << 12
                        };
                        program.insert(fault_pc, instruction);
                        // Both are younger than the exception, including the store
                        // immediately following a fault first detected in EX.
                        program.insert(fault_pc + 4, encode_sd(31, 0, 0x300));
                        program.insert(fault_pc + 8, encode_addi(31, 0, 99));
                        let mut driver = CoreDriver::with_config(program, config);
                        let mut done = false;
                        let mut trace = Vec::new();
                        for _ in 0..300 {
                            if trace.len() < 32 && driver.value("ex_pc_q") >= u64::from(fault_pc) {
                                trace.push([
                                    driver.value("pc"),
                                    driver.value("ex_pc_q"),
                                    driver.value("lsu_address"),
                                    driver.value("lsu_access_fault"),
                                    driver.value("cm_pc_q"),
                                    driver.value("cm_exception_q"),
                                    driver.value("cm_trap_event"),
                                    driver.value("redirect_pending_q"),
                                ]);
                            }
                            assert_eq!(
                                driver.value("data_request_sequence"),
                                0,
                                "fault issued memory request: {address:x}, offset {offset}, size {size}, store {store}"
                            );
                            let cycle = driver.step();
                            assert!(!cycle.data_we && !cycle.commit_mem_we);
                            if cycle.commit_valid {
                                assert!(!(fault_pc..fault_pc + 12).contains(&cycle.commit_pc));
                                if cycle.commit_pc == 0x408 {
                                    done = true;
                                    break;
                                }
                            }
                        }
                        assert!(
                            done,
                            "no trap: config {config}, {address:x}, offset {offset}, size {size}, store {store}; {trace:x?}"
                        );
                        assert_eq!(driver.csr_value("mcause_q"), if store { 7 } else { 5 });
                        assert_eq!(driver.csr_value("mepc_q") << 1, u64::from(fault_pc));
                        assert_eq!(driver.csr_value("mtval_q"), fault_address);
                        assert_eq!(driver.value("debug_x31"), 77);
                    }
                }
            }
        }
    }
}

#[test]
fn access_effective_address_uses_signed_offset_and_xlen_wraparound() {
    for (address, allowed) in [
        (0x300_u64, true),
        (0x8000_fffe, true),
        (0x8001_0000, true),
        (0xfff, true),
        (0x1000, false),
        (0x0000_0001_0000_0000, false),
        (0x0001_0000_0000_0000, false),
        (0xffff_ffff_ffff_ffff, false),
    ] {
        for offset in [-2048_i32, -17, -1, 0, 1, 17, 2047] {
            for store in [false, true] {
                for (logic, gap) in [
                    (false, 0),
                    (true, 0),
                    (false, 1),
                    (true, 1),
                    (false, 3),
                    (true, 3),
                ] {
                    let mut program = access_program();
                    let mut pc = 8;
                    let base = address.wrapping_sub(offset as u64);
                    append_constant(&mut program, &mut pc, 3, base);
                    // Arithmetic and logic producers exercise both fast bypass
                    // sources. Gaps exercise the already captured operand as well.
                    program.insert(
                        pc,
                        if logic {
                            3 << 15 | 6 << 12 | 2 << 7 | 0x33
                        } else {
                            encode_addi(2, 3, 0)
                        },
                    );
                    pc += 4;
                    for _ in 0..gap {
                        program.insert(pc, encode_addi(0, 0, 0));
                        pc += 4;
                    }
                    let instruction = if store {
                        encode_sd(0, 2, offset) & !(7 << 12) // SB
                    } else {
                        (encode_ld(31, 2, offset) & !(7 << 12)) | 4 << 12 // LBU
                    };
                    program.insert(pc, instruction);
                    program.insert(pc + 4, encode_jal(0, 0));
                    let mut driver = CoreDriver::with_config(program, ACCESS_CONFIG);
                    let mut retired = false;
                    let mut trapped = false;
                    let mut token = 0;
                    let mut requests = 0;
                    for _ in 0..300 {
                        let next_token = driver.value("data_request_sequence");
                        if token != next_token {
                            assert!(allowed, "invalid {address:x} issued a request");
                            assert_eq!(driver.value("data_addr"), address);
                            token = next_token;
                            requests += 1;
                        }
                        driver.acknowledge_data_request();
                        let cycle = driver.step();
                        if !allowed {
                            assert!(!cycle.data_we && !cycle.commit_mem_we);
                        }
                        if cycle.commit_valid && cycle.commit_pc == pc {
                            retired = true;
                            break;
                        }
                        if cycle.commit_valid && cycle.commit_pc == 0x408 {
                            trapped = true;
                            break;
                        }
                    }
                    assert_eq!(
                        retired, allowed,
                        "base {base:x}, offset {offset}, store {store}"
                    );
                    assert_eq!(trapped, !allowed);
                    assert_eq!(requests, usize::from(allowed));
                    if !allowed {
                        assert_eq!(driver.csr_value("mcause_q"), if store { 7 } else { 5 });
                        assert_eq!(driver.csr_value("mepc_q") << 1, u64::from(pc));
                        assert_eq!(driver.csr_value("mtval_q"), address);
                    }
                }
            }
        }
    }
}

#[test]
fn access_fault_on_invalid_control_target_preserves_full_pc_and_jalr_link() {
    for config in [ACCESS_CONFIG, 98_436] {
        for target in [
            0x1000,
            0x8003_f000,
            0x0000_0001_0000_0200,
            0xffff_ffff_8000_0200,
        ] {
            for mret in [false, true] {
                let mut program = access_program();
                let mut pc = 8;
                // JALR must clear bit zero, even on an inaccessible target.
                append_constant(&mut program, &mut pc, 2, target | 1);
                if mret {
                    program.insert(pc, encode_csr(0, 0x341, 2, 1));
                    pc += 4;
                    program.insert(pc, 0x3020_0073);
                } else {
                    program.insert(pc, 2 << 15 | 31 << 7 | 0x67);
                }
                let control_pc = pc;
                let mut driver = CoreDriver::with_config(program, config);
                let mut control_retired = false;
                let mut done = false;
                for _ in 0..300 {
                    let cycle = driver.step();
                    assert!(!cycle.data_we);
                    if cycle.commit_valid && cycle.commit_pc == control_pc {
                        control_retired = true;
                        if !mret {
                            assert_eq!(cycle.commit_data, u64::from(control_pc + 4));
                        }
                    }
                    if cycle.commit_valid && cycle.commit_pc == 0x408 {
                        done = true;
                        break;
                    }
                }
                assert!(
                    done && control_retired,
                    "target {target:016x} mret {mret} config {config}"
                );
                assert_eq!(driver.csr_value("mcause_q"), 1);
                assert_eq!(driver.csr_value("mepc_q") << 1, target);
                assert_eq!(driver.csr_value("mtval_q"), target);
            }
        }
    }
}

#[test]
fn access_instruction_boundary_reports_first_inaccessible_parcel() {
    for target in [0xffe, 0x8003_effe] {
        for compressed in [false, true] {
            let mut program = access_program();
            let mut pc = 8;
            append_constant(&mut program, &mut pc, 2, target);
            program.insert(pc, 2 << 15 | 0x67);
            program.insert(target as u32, if compressed { 0x0001 } else { NOP });
            let mut driver = CoreDriver::with_config(program, ACCESS_CONFIG);
            let mut boundary_retired = false;
            let mut done = false;
            for _ in 0..300 {
                let cycle = driver.step();
                boundary_retired |= cycle.commit_valid && u64::from(cycle.commit_pc) == target;
                if cycle.commit_valid && cycle.commit_pc == 0x408 {
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(boundary_retired, compressed);
            assert_eq!(driver.csr_value("mcause_q"), 1);
            assert_eq!(
                driver.csr_value("mepc_q") << 1,
                target + if compressed { 2 } else { 0 }
            );
            assert_eq!(driver.csr_value("mtval_q"), target + 2);
        }
    }
}

#[test]
fn access_fault_waits_for_older_store_and_precedes_pending_interrupt() {
    for config in [ACCESS_CONFIG, 98_436] {
        let mut program = interrupt_program([]);
        let mut pc = 20;
        append_constant(&mut program, &mut pc, 2, 0x0000_0001_0000_0300);
        let store_pc = pc;
        let fault_pc = pc + 4;
        program.insert(store_pc, encode_sd(1, 0, 0x300));
        program.insert(fault_pc, encode_ld(31, 2, 0));
        program.insert(fault_pc + 4, encode_sd(1, 0, 0x308));
        program.insert(0x40c, encode_jal(0, 0));
        let mut driver = CoreDriver::with_config(program, config);
        let mut waiting = 0;
        let mut stores_retired = 0;
        let mut request_edges = 0;
        let mut previous_request = 0;
        let mut done = false;
        for _ in 0..400 {
            if driver.value("cm_valid_q") != 0 && driver.value("cm_pc_q") == u64::from(store_pc) {
                waiting += 1;
                driver.set_interrupts(7);
                if waiting == 8 {
                    driver.acknowledge_data_request();
                }
            }
            assert_eq!(
                driver.value("interrupt_event"),
                0,
                "interrupt overtook synchronous fault"
            );
            let request = driver.value("data_request_sequence");
            request_edges += u32::from(request != previous_request);
            previous_request = request;
            let cycle = driver.step();
            if cycle.commit_valid {
                assert_ne!(cycle.commit_pc, fault_pc);
                assert_ne!(cycle.commit_pc, fault_pc + 4);
                stores_retired += u32::from(cycle.commit_mem_we);
                if cycle.commit_pc == 0x408 {
                    done = true;
                    break;
                }
            }
        }
        assert!(done && waiting >= 8);
        assert_eq!(request_edges, 1);
        assert_eq!(stores_retired, 1);
        assert_eq!(driver.csr_value("mcause_interrupt_q"), 0);
        assert_eq!(driver.csr_value("mcause_q"), 5);
        assert_eq!(driver.csr_value("mepc_q") << 1, u64::from(fault_pc));
        assert_eq!(driver.csr_value("mtval_q"), 0x0000_0001_0000_0300);
    }
}

#[test]
fn access_pc_relative_jump_and_jalr_carry_keep_full_rv64_target() {
    for indirect in [false, true] {
        let mut program = access_program();
        let mut pc = 8;
        let target = if indirect {
            append_constant(&mut program, &mut pc, 2, 0xffff_fff8);
            program.insert(pc, 16 << 20 | 2 << 15 | 31 << 7 | 0x67);
            0x0000_0001_0000_0008
        } else {
            program.insert(pc, encode_jal(31, -16));
            0xffff_ffff_ffff_fff8
        };
        let mut driver = CoreDriver::with_config(program, ACCESS_CONFIG);
        let mut linked = false;
        let mut done = false;
        for _ in 0..300 {
            let cycle = driver.step();
            if cycle.commit_valid && cycle.commit_pc == pc {
                linked = true;
                assert_eq!(cycle.commit_data, u64::from(pc + 4));
            }
            if cycle.commit_valid && cycle.commit_pc == 0x408 {
                done = true;
                break;
            }
        }
        assert!(linked && done);
        assert_eq!(driver.csr_value("mepc_q") << 1, target);
        assert_eq!(driver.csr_value("mtval_q"), target);
        assert_eq!(driver.csr_value("mcause_q"), 1);
    }
}

#[test]
fn access_jalr_upper_carry_and_borrow_survive_forwarding_and_memory_waits() {
    for config in [ACCESS_CONFIG, 98_436] {
        for (base, offset) in [
            (0x0000_0000_ffff_fff8_u64, 16_i32),
            (0x0000_0001_0000_0008, -16),
            (0x0000_0001_0000_0800, -2048),
            (0x0000_0000_ffff_f801, 2047),
            (0x1234_5678_ffff_fff9, 16),
            (0x1234_5678_0000_0009, -16),
            (0xffff_ffff_ffff_ff80, 512),
            (0x0000_0000_0000_0241, -64),
            // Invalid fetches whose speculative successor crosses bit 32 or
            // wraps XLEN still trap at the complete original target.
            (0x0000_0000_ffff_fffe, 0),
            (0xffff_ffff_ffff_fff8, 0),
            (0xffff_ffff_ffff_ffff, 0),
        ] {
            for wait_for_store in [false, true] {
                let target = base.wrapping_add_signed(i64::from(offset)) & !1;
                let accessible = target < 0x1000;
                let mut program = access_program();
                let mut pc = 8;
                append_constant(&mut program, &mut pc, 2, base);
                if wait_for_store {
                    program.insert(pc, encode_sd(2, 0, 0x300));
                    pc += 4;
                }
                program.insert(
                    pc,
                    ((offset as u32) & 0xfff) << 20 | 2 << 15 | 31 << 7 | 0x67,
                );
                if accessible {
                    program.insert(target as u32, encode_addi(30, 0, 77));
                    program.insert(target as u32 + 4, encode_jal(0, 0));
                }
                let mut driver = CoreDriver::with_config(program, config);
                let mut waits = 0;
                let mut links = 0;
                let mut done = false;
                for _ in 0..400 {
                    let cycle = driver.step();
                    if cycle.memory_wait {
                        waits += 1;
                        if waits >= 12 {
                            driver.acknowledge_data_request();
                        }
                    }
                    if cycle.commit_valid && cycle.commit_pc == pc {
                        links += 1;
                        assert_eq!(cycle.commit_data, u64::from(pc + 4));
                    }
                    if cycle.commit_valid {
                        if accessible && u64::from(cycle.commit_pc) == target {
                            assert_eq!(cycle.commit_data, 77);
                            done = true;
                            break;
                        }
                        if !accessible && cycle.commit_pc == 0x408 {
                            assert_eq!(driver.csr_value("mcause_q"), 1);
                            assert_eq!(driver.csr_value("mepc_q") << 1, target);
                            assert_eq!(driver.csr_value("mtval_q"), target);
                            done = true;
                            break;
                        }
                    }
                }
                assert!(
                    done && links == 1,
                    "config={config} base={base:016x} offset={offset} store={wait_for_store}"
                );
                assert_eq!(waits >= 12, wait_for_store);
            }
        }
    }
}

#[test]
fn access_wrong_path_faults_and_boundary_prefetch_have_no_architectural_effect() {
    for config in [ACCESS_CONFIG, 98_436] {
        for store in [false, true] {
            let mut program = access_program();
            let mut pc = 8;
            append_constant(&mut program, &mut pc, 2, 0x1_0000_0300);
            program.insert(pc, encode_branch(0, 0, 0, 8));
            program.insert(
                pc + 4,
                if store {
                    encode_sd(2, 2, 0)
                } else {
                    encode_ld(31, 2, 0)
                },
            );
            let mut cursor = pc + 8;
            append_constant(&mut program, &mut cursor, 3, 0xffc);
            program.insert(cursor, 3 << 15 | 0x67);
            // Repeatedly fetch the last legal word. Sequential lookahead at
            // 0x1000 must be discarded when this back edge repairs prediction.
            program.insert(0xffc, encode_jal(0, pc as i32 - 0xffc));
            let mut driver = CoreDriver::with_config(program, config);
            let mut loops = 0;
            for _ in 0..500 {
                let cycle = driver.step();
                assert_eq!(driver.value("data_request_sequence"), 0);
                if cycle.commit_valid {
                    assert_ne!(cycle.commit_pc, pc + 4);
                    assert!(
                        !(0x400..0x410).contains(&cycle.commit_pc),
                        "wrong-path trap"
                    );
                    loops += u32::from(cycle.commit_pc == 0xffc);
                }
            }
            assert!(
                loops >= 4,
                "config {config}: no repeated boundary execution"
            );
            assert_eq!(driver.csr_value("mcause_q"), 0);
        }
    }
}

fn interrupt_program(body: impl IntoIterator<Item = (u32, u32)>) -> BTreeMap<u32, u32> {
    [
        (0, encode_addi(1, 0, 0x400)),
        (4, encode_csr(0, 0x305, 1, 1)),      // mtvec, direct mode
        (8, encode_addi(1, 0, -1912)),        // low bits = 0x888
        (12, encode_csr(0, 0x304, 1, 1)),     // mie: MEIE/MTIE/MSIE
        (16, encode_csr(0, 0x300, 8, 6)),     // csrsi mstatus, 8
        (0x400, encode_csr(20, 0x342, 0, 2)), // mcause
        (0x404, encode_csr(21, 0x341, 0, 2)), // mepc
        (0x408, encode_csr(22, 0x343, 0, 2)), // mtval
        (0x40c, encode_csr(23, 0x300, 0, 2)), // mstatus
        (0x410, encode_addi(24, 24, 1)),
        (0x414, 0x3020_0073), // mret
    ]
    .into_iter()
    .chain(body)
    .collect()
}

#[test]
fn interrupts_precede_enable_successor_and_each_mret_rechecks_all_levels() {
    for config in [INTERRUPTS_CONFIG, 32_900] {
        let mut driver = CoreDriver::with_config(
            interrupt_program([
                (20, encode_sd(1, 0, 0x300)),
                (24, encode_addi(31, 0, 77)),
                (28, encode_jal(0, 0)),
            ]),
            config,
        );
        driver.set_interrupts(7);
        let mut causes = Vec::new();
        let mut entries = 0;
        let mut stores = 0;
        let mut old_request = 0;
        let mut done = false;
        for _ in 0..1200 {
            let entering = driver.value("interrupt_event") != 0;
            if entering {
                assert_eq!(driver.value("interrupt_resume_pc_q"), 20);
                assert_eq!(driver.value("muldiv_start"), 0);
                assert_eq!(driver.value("data_we"), 0);
                entries += 1;
            }
            let cycle = driver.step();
            if entering {
                driver.set_interrupts(match entries {
                    1 => 3, // External acknowledged; software outranks timer.
                    2 => 2,
                    3 => 0,
                    _ => panic!("interrupt repeated after its level was cleared"),
                });
            }
            let request = driver.value("data_request_sequence");
            if request != old_request {
                assert_eq!(
                    entries, 3,
                    "the successor store escaped an enable/MRET boundary"
                );
                old_request = request;
                stores += 1;
            }
            driver.acknowledge_data_request();
            if cycle.commit_valid {
                match cycle.commit_pc {
                    0x400 => causes.push(cycle.commit_data),
                    0x404 => assert_eq!(cycle.commit_data, 20),
                    0x408 => assert_eq!(cycle.commit_data, 0),
                    0x40c => assert_eq!(cycle.commit_data, 0x1880),
                    24 => {
                        assert_eq!(cycle.commit_data, 77);
                        done = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(done, "config {config}: no return from interrupts");
        assert_eq!(entries, 3);
        assert_eq!(stores, 1);
        assert_eq!(causes, [11, 3, 7].map(|cause| (1 << 63) | cause));
    }
}

#[test]
fn interrupt_arrival_sweep_preserves_load_divide_store_and_taken_compressed_trace() {
    // Every injected arrival must preserve this independent architectural
    // trace, including the taken-branch target at IALIGN=16. Delayed memory
    // responses keep both loads and stores owned by CM during interruption.
    let expected_pcs = [0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 48, 50];
    for config in [INTERRUPTS_CONFIG, 32_900] {
        for arrival in (0..180).step_by(3) {
            let mut driver = CoreDriver::with_config(
                interrupt_program([
                    (20, encode_addi(2, 0, 100)),
                    (24, encode_addi(3, 0, 3)),
                    (28, encode_ld(4, 0, 0x300)),
                    (32, encode_muldiv(5, 2, 3, 5)), // divu: 100 / 3
                    (36, encode_sd(5, 0, 0x308)),
                    (40, encode_branch(0, 0, 0, 8)),
                    (44, encode_sd(0, 0, 0x310)), // wrong path
                    (48, 0x0f85),                 // c.addi x31, 1
                    (50, encode_addi(6, 31, 6)),
                    (54, encode_jal(0, 0)),
                ]),
                config,
            );
            driver.set_data_rdata(84);
            let mut retired = 0;
            let mut commits = Vec::new();
            let mut expected_epc = None;
            let mut entries = 0;
            let mut returns = 0;
            let mut requests = Vec::new();
            let mut handled = 0;
            let mut response_at = 0;
            let mut done = false;
            for time in 0..1200 {
                if time == arrival {
                    driver.set_interrupts(4);
                }
                // Also exercise a frontend that cannot currently provide the
                // next instruction: MEPC cannot be taken from a fetch cursor.
                driver.set_instr_valid(!(58..68).contains(&time));
                let request = driver.value("data_request_sequence");
                if request != handled {
                    handled = request;
                    response_at = time + 10;
                    let address = driver.value("data_addr");
                    let write = driver.value("data_we") != 0;
                    if write {
                        assert_eq!(driver.value("data_wdata"), 33);
                    }
                    requests.push((address, write));
                }
                if time >= response_at {
                    driver.acknowledge_data_request();
                }
                let entering = driver.value("interrupt_event") != 0;
                if entering {
                    entries += 1;
                    let next_pc = expected_pcs.get(retired).copied().unwrap_or(54);
                    assert_eq!(driver.value("interrupt_resume_pc_q"), u64::from(next_pc));
                    expected_epc = Some(u64::from(next_pc));
                }
                let cycle = driver.step();
                if entering {
                    driver.set_interrupts(0);
                }
                if cycle.retire && cycle.cm_pc < 54 {
                    assert_eq!(
                        Some(&cycle.cm_pc),
                        expected_pcs.get(retired),
                        "config={config}, arrival={arrival}"
                    );
                    retired += 1;
                }
                if cycle.commit_valid {
                    if cycle.commit_pc < 54 {
                        commits.push(cycle.commit_pc);
                        match cycle.commit_pc {
                            28 => assert_eq!(cycle.commit_data, 84),
                            32 => assert_eq!(cycle.commit_data, 33),
                            48 => assert_eq!(cycle.commit_data, 1),
                            50 => assert_eq!(cycle.commit_data, 7),
                            _ => {}
                        }
                    }
                    match cycle.commit_pc {
                        0x400 => assert_eq!(cycle.commit_data, (1 << 63) | 11),
                        0x404 => assert_eq!(Some(cycle.commit_data), expected_epc),
                        0x408 => assert_eq!(cycle.commit_data, 0),
                        0x414 => returns += 1,
                        54 if returns == 1 && time > arrival => {
                            done = true;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            assert!(done, "config={config}, arrival={arrival}: no completion");
            assert_eq!(entries, 1);
            assert_eq!(commits, expected_pcs);
            assert_eq!(requests, [(0x300, false), (0x308, true)]);
        }
    }
}

#[test]
fn interrupt_acceptance_preserves_cause_and_pc_when_pending_levels_change() {
    for config in [
        INTERRUPTS_CONFIG,
        32_900,
        ACCESS_CONFIG,
        98_436,
    ] {
        for (pending, changed, cause) in [(7, 0, 11), (7, 2, 11), (2, 7, 7)] {
            let mut program =
                interrupt_program([(20, encode_sd(1, 0, 0x300)), (24, encode_jal(0, 0))]);
            program.insert(0x414, encode_jal(0, 0)); // Stay in the masked handler.
            let mut driver = CoreDriver::with_config(program, config);
            driver.set_interrupts(pending);
            let mut accepted = 0;
            let mut entered = 0;
            let mut inspected = false;
            for _ in 0..300 {
                let accepting = driver.value("interrupt_accept") != 0;
                if accepting {
                    accepted += 1;
                }
                if driver.value("interrupt_event") != 0 {
                    entered += 1;
                    assert_eq!(driver.value("interrupt_resume_pc_q"), 20);
                    assert_eq!(driver.value("ex_valid_q"), 0);
                    assert_eq!(driver.value("cm_valid_q"), 0);
                }
                assert_eq!(driver.value("data_request_sequence"), 0);
                let cycle = driver.step();
                if accepting {
                    // The interrupt has been accepted at an empty boundary.
                    // Withdrawal or a new, higher-priority level must not
                    // change the already selected trap at the following edge.
                    driver.set_interrupts(changed);
                }
                if cycle.commit_valid {
                    assert_ne!(cycle.commit_pc, 20);
                    match cycle.commit_pc {
                        0x400 => assert_eq!(cycle.commit_data, (1 << 63) | cause),
                        0x404 => assert_eq!(cycle.commit_data, 20),
                        0x408 => assert_eq!(cycle.commit_data, 0),
                        0x40c => {
                            assert_eq!(cycle.commit_data, 0x1880);
                            inspected = true;
                        }
                        _ => {}
                    }
                }
            }
            assert_eq!((accepted, entered, inspected), (1, 1, true));
        }
    }
}

#[test]
fn interrupt_withdrawn_at_empty_boundary_is_not_accepted() {
    for config in [
        INTERRUPTS_CONFIG,
        32_900,
        ACCESS_CONFIG,
        98_436,
    ] {
        let mut driver = CoreDriver::with_config(
            interrupt_program([(20, encode_addi(31, 0, 42)), (24, encode_jal(0, 0))]),
            config,
        );
        driver.set_interrupts(7);
        let mut withdrawn = false;
        let mut resumed = false;
        for _ in 0..300 {
            if driver.value("interrupt_accept") != 0 {
                assert!(!withdrawn);
                // Remove the level before the accepting edge, after draining.
                driver.set_interrupts(0);
                assert_eq!(driver.value("interrupt_accept"), 0);
                withdrawn = true;
            }
            assert_eq!(driver.value("interrupt_event"), 0);
            let cycle = driver.step();
            if cycle.commit_valid {
                assert!(cycle.commit_pc < 0x400);
                if cycle.commit_pc == 20 {
                    assert_eq!(cycle.commit_data, 42);
                    resumed = true;
                    break;
                }
            }
        }
        assert!(withdrawn && resumed);
    }
}

#[test]
fn withdrawn_interrupt_during_memory_wait_resumes_held_operands_without_a_trap() {
    let mut driver = CoreDriver::with_config(
        interrupt_program([
            (20, encode_addi(2, 0, 39)),
            (24, encode_ld(3, 0, 0x300)),
            (28, encode_addi(31, 2, 3)),
            (32, encode_jal(0, 0)),
        ]),
        INTERRUPTS_CONFIG,
    );
    driver.set_data_rdata(10);
    let mut waits = 0;
    let mut done = false;
    for _ in 0..500 {
        assert_eq!(driver.value("interrupt_event"), 0);
        let cycle = driver.step();
        if cycle.memory_wait {
            waits += 1;
            driver.set_interrupts(if (2..8).contains(&waits) { 2 } else { 0 });
            if waits == 12 {
                driver.acknowledge_data_request();
            }
        }
        assert!(!(cycle.commit_valid && cycle.commit_pc >= 0x400));
        if cycle.commit_valid && cycle.commit_pc == 28 {
            assert_eq!(cycle.commit_data, 42);
            done = true;
            break;
        }
    }
    assert!(done);
    assert!(waits >= 12);
}

#[test]
fn older_synchronous_exception_owns_trap_while_interrupt_drain_is_pending() {
    for config in [INTERRUPTS_CONFIG, 32_900] {
        let mut program = interrupt_program([
            (20, 0x0000_0073), // ecall
            (24, encode_sd(1, 0, 0x300)),
        ]);
        program.insert(0x414, encode_jal(0, 0)); // inspect the synchronous handler
        let mut driver = CoreDriver::with_config(program, config);
        let mut injected = false;
        let mut handler = false;
        for _ in 0..400 {
            if driver.value("ex_valid_q") != 0 && driver.value("ex_pc_q") == 20 {
                driver.set_interrupts(7);
                injected = true;
            }
            assert_eq!(driver.value("interrupt_event"), 0);
            let cycle = driver.step();
            assert!(!cycle.data_we);
            if cycle.commit_valid && cycle.commit_pc == 0x400 {
                assert_eq!(cycle.commit_data, 11);
                handler = true;
            }
            if cycle.commit_valid && cycle.commit_pc == 0x404 {
                assert_eq!(cycle.commit_data, 20);
            }
        }
        assert!(injected && handler);
        assert_eq!(driver.csr_value("mcause_interrupt_q"), 0);
        assert_eq!(driver.csr_value("mstatus_q") & 3, 2);
    }
}

#[test]
fn interrupt_predicate_is_rechecked_after_an_inflight_disabling_csr_write() {
    for config in [INTERRUPTS_CONFIG, 32_900] {
        for csr in [0x300, 0x304] {
            for (form, source) in [(1, 0), (3, 1), (5, 0), (7, 8)] {
                let mut driver = CoreDriver::with_config(
                    interrupt_program([
                        (20, encode_csr(0, csr, source, form)),
                        (24, encode_addi(31, 0, 42)),
                        (28, encode_jal(0, 0)),
                    ]),
                    config,
                );
                let mut injected = false;
                let mut done = false;
                for _ in 0..400 {
                    if driver.value("ex_valid_q") != 0 && driver.value("ex_pc_q") == 20 {
                        driver.set_interrupts(1);
                        assert_eq!(driver.value("interrupt_drain"), 1);
                        injected = true;
                    }
                    assert_eq!(driver.value("interrupt_event"), 0);
                    let cycle = driver.step();
                    assert!(!(cycle.commit_valid && cycle.commit_pc >= 0x400));
                    if cycle.commit_valid && cycle.commit_pc == 24 {
                        assert_eq!(cycle.commit_data, 42);
                        done = true;
                        break;
                    }
                }
                assert!(
                    injected && done,
                    "config={config}, CSR={csr:x}, form={form}"
                );
                assert_eq!(driver.value("interrupt_drain"), 0);
            }
        }
    }
}
