use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use celox::{BigUint, NativeBackend, NativeProgramImage, OptLevel, Simulator, SimulatorBuilder};

const NOP: u32 = 0x0000_0013;
const KIND_CONDITIONAL: u64 = 1;
const KIND_DIRECT: u64 = 2;
const KIND_CALL: u64 = 4;

fn project_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn native_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let source = verify_riscv::core_source(&project_path()).unwrap();
        SimulatorBuilder::new(&source, "Core")
            .param("SYNC_DATA_MEMORY", 1)
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
    data_we: bool,
    redirect_pending: bool,
    branch_redirect: bool,
    ex_mismatch: bool,
    ex_exception: bool,
    recover: bool,
    maintenance_recover: bool,
    discard: bool,
    retire: bool,
    predicted_kind: u64,
    predicted_taken: bool,
    predicted_next_pc: u32,
    actual_taken: bool,
    actual_next_pc: u32,
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
        let simulator = SimulatorBuilder::new("", "Core")
            .build_native_from_image(native_image().clone())
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
                io.set(debug_counter_index, 0_u8);
            })
            .unwrap();
        let clock = driver.simulator.event("clk");
        driver.simulator.tick(clock).unwrap();
        driver.simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
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

    fn predictor_value(&mut self, name: &str) -> BigUint {
        let signal = self
            .simulator
            .child_signal(&[("branch_predictor", 0)], name);
        self.simulator.get(signal)
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

    fn counter(&mut self, index: u8) -> u64 {
        let selector = self.simulator.signal("debug_counter_index");
        self.simulator.modify(|io| io.set(selector, index)).unwrap();
        self.value("debug_counter_data")
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
            data_we: self.value("data_we") != 0,
            redirect_pending: self.value("redirect_pending_q") != 0,
            branch_redirect: self.value("branch_redirect_valid") != 0,
            ex_mismatch: self.value("ex_prediction_mismatch") != 0,
            ex_exception: self.value("ex_exception") != 0,
            recover: self.value("predictor_recover_valid") != 0,
            maintenance_recover: self.value("predictor_recover_valid_q") != 0,
            discard: self.value("predictor_discard_all") != 0,
            retire: self.value("cm_retire_event") != 0,
            predicted_kind: self.value("cm_predicted_kind_q"),
            predicted_taken: self.value("cm_predicted_taken_q") != 0,
            predicted_next_pc: self.value("cm_predicted_next_pc_q") as u32,
            actual_taken: self.value("cm_branch_taken") != 0,
            actual_next_pc: self.value("cm_actual_next_pc") as u32,
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

    fn assert_predictor_frontiers_equal(&mut self) {
        assert_eq!(
            self.predictor_value("speculative_history_q"),
            self.predictor_value("committed_history_q"),
            "speculative GHR did not recover to the committed GHR"
        );
        for fold in ["index", "tag_a", "tag_b"] {
            assert_eq!(
                self.predictor_value(&format!("speculative_{fold}_q")),
                self.predictor_value(&format!("committed_{fold}_q")),
                "{fold} folds did not recover"
            );
        }
    }
}

fn direct_trained_driver() -> CoreDriver {
    let mut driver = CoreDriver::new([(0, encode_jal(0, 4)), (4, encode_jal(0, -4))]);
    for _ in 0..300 {
        let cycle = driver.step();
        if cycle.cm_valid && cycle.cm_pc == 0 && cycle.predicted_kind == KIND_DIRECT {
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
fn invalid_fetch_refreshes_masked_payload_without_issuing_or_advancing() {
    let instruction = encode_addi(1, 0, 7);
    let mut driver = CoreDriver::new([(0, instruction)]);
    driver.set_instr_valid(false);
    let initial_lookup_sequence = driver.predictor_value("lookup_sequence_q");

    for _ in 0..3 {
        let cycle = driver.step();
        assert_eq!(cycle.fetch_pc, 0);
        assert!(!cycle.predictor_request);
        assert!(!cycle.predictor_request_accept);
        assert_eq!(driver.value("pc"), 0, "invalid fetch advanced the cursor");
        assert_eq!(
            driver.value("fetch_q") & 1,
            0,
            "invalid payload became visible"
        );
        assert_eq!(
            (driver.value("fetch_q") >> 2) as u32,
            instruction,
            "masked payload did not refresh independently of instr_valid",
        );
        assert_eq!(
            driver.predictor_value("lookup_sequence_q"),
            initial_lookup_sequence,
            "invalid payload capture launched a semantic predictor lookup",
        );
    }

    assert_eq!(driver.value("fetch_payload_capture"), 1);
    assert_eq!(driver.value("predictor_request_ready"), 1);
    driver.set_instr_valid(true);
    let accepted = driver.step();
    assert_eq!(accepted.fetch_pc, 0);
    assert!(accepted.predictor_request);
    assert!(accepted.predictor_request_accept);
    assert_eq!(
        driver.value("pc"),
        4,
        "valid retry did not advance exactly once"
    );
    assert_eq!(driver.value("fetch_q") & 1, 1);
    assert_eq!((driver.value("fetch_q") >> 2) as u32, instruction);
}

#[test]
fn learned_taken_branch_to_fallthrough_repairs_direction_history() {
    // beq x0,x0,+4 is taken, but its target is numerically identical to the
    // ordinary 32-bit fallthrough.  The JAL closes the two-instruction loop.
    let mut driver = CoreDriver::new([(0, encode_branch(0, 0, 0, 4)), (4, encode_jal(0, -4))]);

    let mut cold_branch_recoveries = 0;
    for _ in 0..300 {
        let cycle = driver.step();
        if cycle.recover && cycle.branch_redirect && cycle.cm_valid && cycle.cm_pc == 0 {
            if cycle.predicted_kind != KIND_CONDITIONAL {
                cold_branch_recoveries += 1;
                continue;
            }

            assert_eq!(cold_branch_recoveries, 1);
            assert_eq!(cycle.predicted_kind, KIND_CONDITIONAL);
            assert!(!cycle.predicted_taken);
            assert!(cycle.actual_taken);
            assert_eq!(cycle.predicted_next_pc, 4);
            assert_eq!(cycle.actual_next_pc, 4);
            let next = driver.step();
            assert!(
                !next.recover,
                "conditional recovery lasted more than one cycle"
            );
            assert!(
                next.maintenance_recover && next.redirect_pending,
                "queued predictor recovery did not occupy the redirect window"
            );
            driver.assert_predictor_frontiers_equal();
            assert!(driver.counter(6) > 0, "conditional counter did not advance");
            assert!(
                driver.counter(7) > 0,
                "direction-wrong counter did not advance"
            );
            assert!(driver.counter(8) > 0, "BTB-miss counter did not advance");
            assert_eq!(
                driver.counter(9),
                0,
                "branch incorrectly used the RAS metric"
            );
            assert!(driver.counter(10) > 0, "repair counter did not advance");
            return;
        }
    }
    panic!("the learned direction-only conditional mismatch was not observed");
}

#[test]
fn pending_cold_jal_redirect_drops_the_wrong_path_jal_ras_action() {
    let mut driver = CoreDriver::new([
        (0, encode_jal(1, 16)),
        (4, encode_jal(1, 28)),
        (16, NOP),
        (20, NOP),
    ]);

    let mut recoveries = 0;
    let mut wrong_path_jal_seen = false;
    let mut call_retired = false;
    for _ in 0..100 {
        let cycle = driver.step();
        recoveries += usize::from(cycle.recover);
        if cycle.redirect_pending && cycle.ex_valid && cycle.ex_pc == 4 {
            wrong_path_jal_seen = true;
            assert_eq!(cycle.ex_kind, KIND_CALL);
            assert!(cycle.ex_mismatch);
            assert!(!cycle.recover, "wrong-path JAL enqueued a second recovery");
            assert!(!cycle.discard);
        }
        if cycle.retire && cycle.cm_pc == 0 {
            call_retired = true;
        }
        if wrong_path_jal_seen && call_retired && cycle.fetch_pc >= 16 && !cycle.redirect_pending {
            break;
        }
    }

    assert!(
        wrong_path_jal_seen,
        "the younger JAL never occupied EX during redirect_pending"
    );
    assert!(call_retired, "the architectural JAL did not retire");
    assert_eq!(
        recoveries, 1,
        "cold JAL plus wrong-path JAL recovered more than once"
    );
    assert_eq!(driver.predictor_value("committed_count_q"), 1_u8.into());
    assert_eq!(driver.predictor_value("speculative_count_q"), 1_u8.into());
    assert_eq!(
        driver.predictor_value("speculative_ras_valid_q"),
        0_u8.into(),
        "wrong-path JAL left a speculative RAS overlay"
    );
}

#[test]
fn memory_wait_defers_ex_exception_discard_until_the_pipeline_advances() {
    let mut driver = CoreDriver::new([(0, encode_ld(2, 0, 0)), (4, 0xffff_ffff)]);
    driver.set_data_response_sequence(0);

    let mut held_cycles = 0;
    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.memory_wait && cycle.ex_valid && cycle.ex_pc == 4 && cycle.ex_exception {
            held_cycles += 1;
            assert!(!cycle.recover);
            assert!(
                !cycle.discard,
                "held exception discarded predictor state early"
            );
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
fn memory_wait_defers_ex_mismatch_recovery_until_the_pipeline_advances() {
    let mut driver = CoreDriver::new([(0, encode_ld(2, 0, 0)), (4, encode_jal(1, 12))]);
    driver.set_data_response_sequence(0);

    let mut held_cycles = 0;
    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.memory_wait && cycle.ex_valid && cycle.ex_pc == 4 && cycle.ex_mismatch {
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
    assert!(release.ex_mismatch);
    assert!(
        release.recover,
        "mismatch release did not recover speculative state"
    );
    assert!(!release.discard);

    let following = driver.step();
    assert!(
        !following.recover,
        "mismatch recovery pulsed more than once"
    );
    assert!(!following.discard);
}

#[test]
fn stale_taken_prediction_on_a_load_does_not_bypass_memory_wait() {
    // First learn a direct-control BTB entry at PC zero. Replacing that
    // instruction with a load models stale predictor state after executable
    // memory changes: the load itself owns the next-PC repair, but it must
    // still wait for its synchronous data response and retire that response.
    let mut driver = direct_trained_driver();
    driver.program.insert(0, encode_ld(31, 0, 0));
    let load_value = 0x0123_4567_89ab_cdef;
    let mut acknowledged = false;

    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.redirect_pending && cycle.cm_valid && cycle.cm_pc == 0 && cycle.cm_is_load {
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

#[test]
fn stale_taken_prediction_on_a_store_preserves_the_memory_side_effect() {
    let mut driver = direct_trained_driver();
    driver.program.insert(0, encode_sd(0, 0, 0));

    let mut acknowledged = false;
    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.redirect_pending && cycle.cm_valid && cycle.cm_pc == 0 && cycle.cm_is_store {
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

#[test]
fn stale_taken_prediction_on_a_csr_preserves_the_csr_write() {
    let mut driver = direct_trained_driver();
    driver.program.insert(0, encode_csrrwi(0, 0x340, 7));

    for _ in 0..100 {
        let cycle = driver.step();
        if cycle.redirect_pending && cycle.cm_valid && cycle.cm_pc == 0 && cycle.cm_is_csr {
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
