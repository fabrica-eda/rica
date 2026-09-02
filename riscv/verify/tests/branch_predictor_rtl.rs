use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, OptLevel, Simulator, SimulatorBuilder};

const KIND_NONE: u8 = 0;
const KIND_CONDITIONAL: u8 = 1;
const KIND_DIRECT: u8 = 2;
const KIND_CALL: u8 = 4;
const KIND_RETURN: u8 = 5;
const KIND_COROUTINE: u8 = 6;

fn predictor_source() -> String {
    predictor_source_inner(false)
}

fn fold_probe_source() -> String {
    predictor_source_inner(true)
}

fn predictor_source_inner(fold_probes: bool) -> String {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    [
        "BranchTargetBuffer.veryl",
        "TageTaggedTableMemory.veryl",
        "TageDirectionPredictor.veryl",
        "BranchPredictor.veryl",
    ]
    .into_iter()
    .map(|file| {
        let source = fs::read_to_string(project.join("src").join(file)).unwrap();
        if fold_probes && file == "BranchPredictor.veryl" {
            predictor_source_with_fold_probes(source)
        } else {
            source
        }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn predictor_source_with_fold_probes(mut source: String) -> String {
    const PORT_MARKER: &str = "    ras_fallback_event";
    const PROBE_PORTS: &str = concat!(
        "    test_committed_index_folds   : output logic<36>,\n",
        "    test_committed_tag_a_folds   : output logic<36>,\n",
        "    test_committed_tag_b_folds   : output logic<32>,\n",
        "    test_speculative_index_folds : output logic<36>,\n",
        "    test_speculative_tag_a_folds : output logic<36>,\n",
        "    test_speculative_tag_b_folds : output logic<32>,\n",
    );
    const PROBE_ASSIGNS: &str = concat!(
        "\n    // Test-only packed views expose all folded-history banks.\n",
        "    assign test_committed_index_folds = {committed_index_q.bank3, committed_index_q.bank2, committed_index_q.bank1, committed_index_q.bank0};\n",
        "    assign test_committed_tag_a_folds = {committed_tag_a_q.bank3, committed_tag_a_q.bank2, committed_tag_a_q.bank1, committed_tag_a_q.bank0};\n",
        "    assign test_committed_tag_b_folds = {committed_tag_b_q.bank3, committed_tag_b_q.bank2, committed_tag_b_q.bank1, committed_tag_b_q.bank0};\n",
        "    assign test_speculative_index_folds = {speculative_index_q.bank3, speculative_index_q.bank2, speculative_index_q.bank1, speculative_index_q.bank0};\n",
        "    assign test_speculative_tag_a_folds = {speculative_tag_a_q.bank3, speculative_tag_a_q.bank2, speculative_tag_a_q.bank1, speculative_tag_a_q.bank0};\n",
        "    assign test_speculative_tag_b_folds = {speculative_tag_b_q.bank3, speculative_tag_b_q.bank2, speculative_tag_b_q.bank1, speculative_tag_b_q.bank0};\n",
    );

    let port_start = source.find(PORT_MARKER).expect("ras_fallback_event port");
    let port_end = port_start
        + source[port_start..]
            .find('\n')
            .expect("ras_fallback_event line ending")
        + 1;
    source.insert_str(port_end, PROBE_PORTS);
    let module_end = source.rfind("\n}").expect("BranchPredictor module end");
    source.insert_str(module_end, PROBE_ASSIGNS);
    source
}

fn native_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        SimulatorBuilder::new(&predictor_source(), "BranchPredictor")
            .opt_level(OptLevel::O0)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

fn new_simulator() -> Simulator<NativeBackend> {
    // NativeProgramImage is pointer-free and cloneable.  Compiling it once
    // keeps the directed tests independent without recompiling the same RTL.
    SimulatorBuilder::new("", "BranchPredictor")
        .build_native_from_image(native_image().clone())
        .unwrap()
}

fn new_fold_probe_simulator() -> Simulator<NativeBackend> {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    let image = IMAGE.get_or_init(|| {
        SimulatorBuilder::new(&fold_probe_source(), "BranchPredictor")
            .opt_level(OptLevel::O0)
            .compile_native()
            .unwrap()
            .into_program_image()
    });
    SimulatorBuilder::new("", "BranchPredictor")
        .build_native_from_image(image.clone())
        .unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Response {
    token: u8,
    kind: u8,
    taken: bool,
    next_pc: u32,
    btb_hit: bool,
    tage_hit: bool,
    ras_fallback: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Events {
    conditional_resolved: bool,
    direction_wrong: bool,
    btb_miss: bool,
    ras_fallback: bool,
}

struct Driver {
    simulator: Simulator<NativeBackend>,
}

impl Driver {
    fn new() -> Self {
        Self::from_simulator(new_simulator())
    }

    fn new_with_fold_probes() -> Self {
        Self::from_simulator(new_fold_probe_simulator())
    }

    fn from_simulator(simulator: Simulator<NativeBackend>) -> Self {
        let mut driver = Self { simulator };
        for name in [
            "request_valid",
            "request_is_conditional",
            "response_accept",
            "retire_valid",
            "recover_valid",
            "discard_all",
        ] {
            driver.set_u8(name, 0);
        }
        for name in [
            "retire_token",
            "retire_kind",
            "retire_taken",
            "recover_token",
            "recover_kind",
            "recover_taken",
        ] {
            driver.set_u8(name, 0);
        }
        for name in [
            "request_pc",
            "request_fallthrough",
            "retire_pc",
            "retire_target",
            "retire_fallthrough",
            "recover_fallthrough",
        ] {
            driver.set_u32(name, 0);
        }

        driver.set_u8("rst_n", 0);
        driver.tick();
        driver.set_u8("rst_n", 1);
        assert_eq!(driver.get("ready"), 1);
        assert_eq!(driver.get("response_valid"), 0);
        driver
    }

    fn set_u8(&mut self, name: &str, value: u8) {
        let signal = self.simulator.signal(name);
        self.simulator.modify(|io| io.set(signal, value)).unwrap();
    }

    fn set_u32(&mut self, name: &str, value: u32) {
        let signal = self.simulator.signal(name);
        self.simulator.modify(|io| io.set(signal, value)).unwrap();
    }

    fn set_tage_u8(&mut self, name: &str, value: u8) {
        let signal = self.simulator.child_signal(&[("tage", 0)], name);
        self.simulator.modify(|io| io.set(signal, value)).unwrap();
    }

    fn set_tage_u16(&mut self, name: &str, value: u16) {
        let signal = self.simulator.child_signal(&[("tage", 0)], name);
        self.simulator.modify(|io| io.set(signal, value)).unwrap();
    }

    fn get(&mut self, name: &str) -> u64 {
        self.simulator
            .get(self.simulator.signal(name))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn get_u128(&mut self, name: &str) -> u128 {
        let digits = self
            .simulator
            .get(self.simulator.signal(name))
            .to_u64_digits();
        u128::from(digits.first().copied().unwrap_or(0))
            | (u128::from(digits.get(1).copied().unwrap_or(0)) << 64)
    }

    fn get_fold_array(&mut self, name: &str, width: usize) -> [u64; 4] {
        let packed = self.get(name);
        std::array::from_fn(|bank| (packed >> (bank * width)) & ((1_u64 << width) - 1))
    }

    fn get_tage(&mut self, name: &str) -> u64 {
        let signal = self.simulator.child_signal(&[("tage", 0)], name);
        self.simulator
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn get_tage_slot(&mut self, name: &str, token: u8, width: usize) -> u64 {
        (self.get_tage(name) >> (usize::from(token) * width)) & ((1_u64 << width) - 1)
    }

    fn tick(&mut self) {
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
    }

    fn response(&mut self) -> Response {
        assert_eq!(self.get("response_valid"), 1, "response is not valid");
        Response {
            token: self.get("response_token") as u8,
            kind: self.get("response_kind") as u8,
            taken: self.get("response_taken") != 0,
            next_pc: self.get("response_next_pc") as u32,
            btb_hit: self.get("response_btb_hit") != 0,
            tage_hit: self.get("response_tage_hit") != 0,
            ras_fallback: self.get("response_ras_fallback") != 0,
        }
    }

    fn request_with_hint(&mut self, pc: u32, fallthrough: u32, is_conditional: bool) -> Response {
        assert_eq!(self.get("ready"), 1);
        assert_eq!(self.get("response_valid"), 0, "outstanding response");
        self.set_u32("request_pc", pc);
        self.set_u32("request_fallthrough", fallthrough);
        self.set_u8("request_is_conditional", u8::from(is_conditional));
        self.set_u8("request_valid", 1);
        self.tick();
        self.set_u8("request_valid", 0);
        // The request first enters the one-entry ingress stage. The
        // synchronous BTB/TAGE result then enters the metadata queue,
        // followed by the fully registered classification/output stage.
        self.tick();
        self.tick();
        self.tick();
        self.response()
    }

    fn request(&mut self, pc: u32, fallthrough: u32) -> Response {
        self.request_with_hint(pc, fallthrough, false)
    }

    fn request_conditional(&mut self, pc: u32, fallthrough: u32) -> Response {
        self.request_with_hint(pc, fallthrough, true)
    }

    fn accept(&mut self) {
        assert_eq!(self.get("response_valid"), 1);
        self.set_u8("response_accept", 1);
        self.tick();
        self.set_u8("response_accept", 0);
    }

    fn events(&mut self) -> Events {
        Events {
            conditional_resolved: self.get("conditional_resolved_event") != 0,
            direction_wrong: self.get("direction_wrong_event") != 0,
            btb_miss: self.get("btb_miss_event") != 0,
            ras_fallback: self.get("ras_fallback_event") != 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn retire(
        &mut self,
        response: Response,
        pc: u32,
        kind: u8,
        taken: bool,
        target: u32,
        fallthrough: u32,
        recover: bool,
    ) -> Events {
        self.set_u8("retire_token", response.token);
        self.set_u32("retire_pc", pc);
        self.set_u8("retire_kind", kind);
        self.set_u8("retire_taken", u8::from(taken));
        self.set_u32("retire_target", target);
        self.set_u32("retire_fallthrough", fallthrough);
        self.set_u8("recover_token", response.token);
        self.set_u8("recover_kind", kind);
        self.set_u8("recover_taken", u8::from(taken));
        self.set_u32("recover_fallthrough", fallthrough);
        self.set_u8("retire_valid", 1);
        self.set_u8("recover_valid", u8::from(recover));
        let events = self.events();
        self.tick();
        self.set_u8("retire_valid", 0);
        self.set_u8("recover_valid", 0);
        events
    }

    fn learn_cold_control(&mut self, pc: u32, fallthrough: u32, kind: u8, target: u32) -> Response {
        let response = self.request_with_hint(pc, fallthrough, kind == KIND_CONDITIONAL);
        assert!(!response.btb_hit, "test PC unexpectedly hit in the BTB");
        assert_eq!(response.kind, KIND_NONE);
        assert!(!response.taken);
        assert_eq!(response.next_pc, fallthrough);
        self.accept();
        let events = self.retire(response, pc, kind, true, target, fallthrough, true);
        assert!(events.btb_miss);
        response
    }
}

fn assert_bank_local_valids(driver: &mut Driver, expected: u64, context: &str) {
    for bank in [
        "tage_base_valid_q",
        "tage_tag0_valid_q",
        "tage_tag1_valid_q",
        "tage_tag2_valid_q",
        "tage_tag3_valid_q",
        "btb_way0_valid_q",
        "btb_way1_valid_q",
        "btb_plru_valid_q",
    ] {
        assert_eq!(
            driver.get(bank),
            expected,
            "{context}: bank-local ingress {bank}"
        );
    }
}

#[test]
fn ingress_stage_adds_one_edge_and_keeps_table_pair_in_lockstep() {
    let mut driver = Driver::new();
    let first_pc = 0x4002;
    let second_pc = 0x4042;
    let third_pc = 0x4082;

    driver.set_u32("request_pc", first_pc);
    driver.set_u32("request_fallthrough", first_pc + 2);
    driver.set_u8("request_valid", 1);
    assert_eq!(
        driver.get("request_accept"),
        1,
        "empty ingress must enqueue"
    );
    assert_eq!(driver.get("request_fire"), 0, "enqueue must precede issue");
    driver.tick();
    driver.set_u8("request_valid", 0);

    // The first edge only fills S0. No table response can exist yet; the
    // registered packet is visible as the next issue candidate.
    assert_bank_local_valids(&mut driver, 1, "first enqueue");
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    assert_eq!(driver.get("request_fire"), 1);
    driver.tick();

    // S1 is the synchronous table response. The externally visible decision
    // is still one registered stage farther away for a cold miss.
    assert_eq!(driver.get("btb_response_valid"), 1);
    assert_eq!(driver.get("tage_response_valid"), 1);
    assert_eq!(driver.get("response_valid"), 0);

    // Fill a replacement request while the first raw response is captured.
    // Both table instances must launch the same registered transaction.
    driver.set_u32("request_pc", second_pc);
    driver.set_u32("request_fallthrough", second_pc + 2);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u32("request_pc", third_pc);
    driver.set_u32("request_fallthrough", third_pc + 2);
    assert_eq!(driver.get("request_accept"), 1);
    assert_eq!(driver.get("request_fire"), 1);
    assert_bank_local_valids(&mut driver, 1, "simultaneous pop/push");
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    driver.tick();
    driver.set_u8("request_valid", 0);
    assert_eq!(driver.get("btb_response_valid"), 1);
    assert_eq!(driver.get("tage_response_valid"), 1);
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.response().token, 0);
    driver.set_u8("response_accept", 1);
    assert_eq!(driver.get("request_fire"), 1);
    driver.tick();
    driver.set_u8("response_accept", 0);
    driver.set_u8("request_valid", 0);

    // The first decision was accepted while the second raw response was
    // captured. Its token and payload remain ordered despite the extra S0.
    let second = driver.response();
    assert_eq!(second.token, 1);
    assert_eq!(second.next_pc, second_pc + 2);
}

#[test]
fn recovery_and_discard_flush_all_bank_local_ingress_valids() {
    for discard in [false, true] {
        let mut driver = Driver::new();
        let pc = if discard { 0x4a02 } else { 0x4c02 };
        driver.set_u32("request_pc", pc);
        driver.set_u32("request_fallthrough", pc + 4);
        driver.set_u8("request_valid", 1);
        assert_eq!(driver.get("request_accept"), 1);
        driver.tick();
        driver.set_u8("request_valid", 0);
        assert_bank_local_valids(&mut driver, 1, "queued packet before flush");

        if discard {
            driver.set_u8("discard_all", 1);
        } else {
            driver.set_u8("recover_kind", KIND_NONE);
            driver.set_u8("recover_valid", 1);
        }
        driver.tick();
        driver.set_u8("discard_all", 0);
        driver.set_u8("recover_valid", 0);
        assert_bank_local_valids(&mut driver, 0, "recovery/discard flush");
        assert_eq!(driver.get("btb_response_valid"), 0);
        assert_eq!(driver.get("tage_response_valid"), 0);
    }
}

#[test]
fn ingress_issue_rebases_on_predecessor_conditional_shadow() {
    let mut driver = Driver::new_with_fold_probes();

    // Make the history non-zero so using the enqueue-edge fold is observably
    // different from the predecessor's not-taken shadow at the issue edge.
    driver.set_u8("recover_kind", KIND_CONDITIONAL);
    driver.set_u8("recover_taken", 1);
    driver.set_u8("recover_valid", 1);
    driver.tick();
    driver.set_u8("recover_valid", 0);
    assert_fold_frontier(&mut driver, false, 1);

    let first_pc = 0x1002;
    let second_pc = 0x1042;
    let third_pc = 0x1082;
    driver.set_u32("request_pc", first_pc);
    driver.set_u32("request_fallthrough", first_pc + 4);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();

    // The second request is accepted while the first request issues. It is
    // therefore carrying the old frontier in S0 when the first response is
    // still unknown.
    driver.set_u32("request_pc", second_pc);
    driver.set_u32("request_fallthrough", second_pc + 4);
    driver.set_u8("request_is_conditional", 1);
    assert_eq!(driver.get("request_accept"), 1);
    assert_eq!(driver.get("request_fire"), 1);
    driver.tick();

    // At this edge the first conditional response is captured and the
    // second packet issues. Its TAGE address must use first's not-taken fold
    // (history 1 -> 2), not the stale enqueue-edge fold (history 1).
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("request_fire"), 1);
    driver.set_u32("request_pc", third_pc);
    driver.set_u32("request_fallthrough", third_pc + 4);
    driver.set_u8("request_is_conditional", 1);
    driver.tick();
    assert_eq!(
        driver.get_tage("lookup_index0_q"),
        u64::from(((second_pc >> 1) & 0x1ff) ^ 2),
        "issue must rebase the queued request on its predecessor's NT shadow",
    );

    // C was enqueued while B issued. B's response is now the known
    // predecessor, so C must advance the preview once more (2 -> 4).
    driver.set_u8("request_valid", 0);
    driver.set_u8("response_accept", 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("request_fire"), 1);
    driver.tick();
    driver.set_u8("response_accept", 0);
    assert_eq!(
        driver.get_tage("lookup_index0_q"),
        u64::from(((third_pc >> 1) & 0x1ff) ^ 4),
        "continuous conditional issue must carry the NT preview forward",
    );
}

fn seed_full_btb_set_with_way_zero_next(driver: &mut Driver, first_pc: u32) {
    let second_pc = first_pc + 0x100;
    assert_eq!((first_pc >> 1) & 0x7f, (second_pc >> 1) & 0x7f);

    let first = driver.learn_cold_control(first_pc, first_pc + 4, KIND_DIRECT, first_pc + 0x800);
    let first_way = (driver.get("slot_btb_way_q") >> first.token) & 1;
    let second =
        driver.learn_cold_control(second_pc, second_pc + 4, KIND_DIRECT, second_pc + 0x800);
    let second_way = (driver.get("slot_btb_way_q") >> second.token) & 1;
    assert_eq!((first_way, second_way), (0, 1));
    // Accepting the way-one miss leaves way zero as the next victim.
}

fn launch_cold_btb_lookup(driver: &mut Driver, pc: u32) {
    driver.set_u32("request_pc", pc);
    driver.set_u32("request_fallthrough", pc + 4);
    driver.set_u8("request_is_conditional", 0);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u8("request_valid", 0);
    // The first edge enqueues the request; the following edge issues the
    // registered packet into both synchronous tables.
    driver.tick();
    assert_eq!(driver.get("btb_response_valid"), 1);
    assert_eq!(driver.get("tage_response_valid"), 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
}

fn assert_next_cold_miss_uses_way_zero(driver: &mut Driver, pc: u32) {
    let response = driver.request(pc, pc + 4);
    assert!(!response.btb_hit);
    assert_eq!(
        (driver.get("slot_btb_way_q") >> response.token) & 1,
        0,
        "a flushed lookup must not change the next victim",
    );
}

fn check_raw_miss_flush_preserves_btb_victim(discard: bool) {
    let mut driver = Driver::new();
    let set_pc = 0x6004;
    seed_full_btb_set_with_way_zero_next(&mut driver, set_pc);

    let recovery_anchor = if discard {
        None
    } else {
        let response = driver.request(set_pc + 2, set_pc + 6);
        driver.accept();
        Some(response)
    };

    launch_cold_btb_lookup(&mut driver, set_pc + 0x200);
    if let Some(anchor) = recovery_anchor {
        driver.set_u8("recover_token", anchor.token);
        driver.set_u8("recover_kind", KIND_NONE);
        driver.set_u32("recover_fallthrough", set_pc + 6);
        driver.set_u8("recover_valid", 1);
    } else {
        driver.set_u8("discard_all", 1);
    }
    assert_eq!(driver.get("table_response_cancel"), 1);
    assert_eq!(driver.get("table_response_accept"), 1);
    assert_eq!(driver.get("raw_response_capture"), 0);
    driver.tick();
    driver.set_u8("recover_valid", 0);
    driver.set_u8("discard_all", 0);

    assert_eq!(driver.get("decision_valid_q"), 0);
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    assert_eq!(
        driver.get_tage("slot_valid_q"),
        0,
        "a canceled raw response must not leave an orphan TAGE snapshot",
    );
    assert_next_cold_miss_uses_way_zero(&mut driver, set_pc + 0x300);
}

fn check_decision_miss_flush_restores_btb_victim(discard: bool) {
    let mut driver = Driver::new();
    let set_pc = 0x7004;
    seed_full_btb_set_with_way_zero_next(&mut driver, set_pc);

    let recovery_anchor = if discard {
        None
    } else {
        let response = driver.request(set_pc + 2, set_pc + 6);
        driver.accept();
        Some(response)
    };

    launch_cold_btb_lookup(&mut driver, set_pc + 0x200);
    driver.tick();
    assert_eq!(driver.get("decision_valid_q"), 1);
    assert_eq!(driver.get("decision_btb_plru_q"), 0);
    assert_eq!(driver.get("decision_fast_miss"), 1);

    if let Some(anchor) = recovery_anchor {
        driver.set_u8("recover_token", anchor.token);
        driver.set_u8("recover_kind", KIND_NONE);
        driver.set_u32("recover_fallthrough", set_pc + 6);
        driver.set_u8("recover_valid", 1);
    } else {
        driver.set_u8("discard_all", 1);
    }
    assert_eq!(driver.get("decision_replacement_rollback"), 1);
    driver.tick();
    driver.set_u8("recover_valid", 0);
    driver.set_u8("discard_all", 0);

    assert_eq!(driver.get("decision_valid_q"), 0);
    assert_eq!(driver.get("response_valid"), 0);
    assert_eq!(
        driver.get_tage("slot_valid_q"),
        0,
        "flushing a held decision must release its TAGE snapshot",
    );
    assert_next_cold_miss_uses_way_zero(&mut driver, set_pc + 0x300);
}

const HISTORY_BITS: usize = 108;
const HISTORY_LENGTHS: [usize; 4] = [4, 12, 36, 108];
const TAG_A_WIDTHS: [usize; 4] = [8, 8, 9, 9];
const TAG_B_WIDTHS: [usize; 4] = [7, 7, 8, 8];

fn append_history(history: u128, taken: bool) -> u128 {
    ((history << 1) | u128::from(taken)) & ((1_u128 << HISTORY_BITS) - 1)
}

fn recompute_fold(history: u128, length: usize, width: usize) -> u64 {
    let mut folded = 0_u64;
    for bit in 0..length {
        if ((history >> bit) & 1) != 0 {
            folded ^= 1_u64 << (bit % width);
        }
    }
    folded
}

fn assert_fold_frontier(driver: &mut Driver, committed: bool, history: u128) {
    let (history_name, index_name, tag_a_name, tag_b_name) = if committed {
        (
            "committed_history_q",
            "test_committed_index_folds",
            "test_committed_tag_a_folds",
            "test_committed_tag_b_folds",
        )
    } else {
        (
            "speculative_history_q",
            "test_speculative_index_folds",
            "test_speculative_tag_a_folds",
            "test_speculative_tag_b_folds",
        )
    };

    assert_eq!(driver.get_u128(history_name), history);
    let actual_index = driver.get_fold_array(index_name, 9);
    let actual_tag_a = driver.get_fold_array(tag_a_name, 9);
    let actual_tag_b = driver.get_fold_array(tag_b_name, 8);
    let expected_index: [u64; 4] =
        std::array::from_fn(|bank| recompute_fold(history, HISTORY_LENGTHS[bank], 9));
    let expected_tag_a: [u64; 4] = std::array::from_fn(|bank| {
        recompute_fold(history, HISTORY_LENGTHS[bank], TAG_A_WIDTHS[bank])
    });
    let expected_tag_b: [u64; 4] = std::array::from_fn(|bank| {
        recompute_fold(history, HISTORY_LENGTHS[bank], TAG_B_WIDTHS[bank])
    });
    assert_eq!(
        (actual_index, actual_tag_a, actual_tag_b),
        (expected_index, expected_tag_a, expected_tag_b),
        "index/tag-A/tag-B folds"
    );
}

#[test]
fn conditional_recover_without_retire_appends_actual_to_every_speculative_fold() {
    let mut driver = Driver::new_with_fold_probes();

    // Seed a non-zero committed frontier without allocating a predictor slot.
    // The following recovery is deliberately not coincident with retirement,
    // matching the early-conditional-repair API contract.
    driver.set_u8("retire_kind", KIND_CONDITIONAL);
    driver.set_u8("retire_taken", 1);
    driver.set_u8("retire_valid", 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    assert_fold_frontier(&mut driver, true, 1);

    driver.set_u8("recover_kind", KIND_CONDITIONAL);
    driver.set_u8("recover_taken", 0);
    driver.set_u8("recover_valid", 1);
    assert_eq!(driver.get("retire_valid"), 0);
    driver.tick();
    driver.set_u8("recover_valid", 0);

    // Appending the actual not-taken outcome rotates every lane from one to
    // two. Merely copying the committed folds would leave them at one.
    assert_fold_frontier(&mut driver, false, 2);
    assert_fold_frontier(&mut driver, true, 1);
}

#[test]
fn simultaneous_older_retire_and_younger_early_recover_use_committed_next_frontier() {
    let mut driver = Driver::new_with_fold_probes();
    let mut seeded_history = 0_u128;

    // Exercise every outgoing-history position before the priority case. The
    // requests below are cold non-control predictions, so accepting them does
    // not perturb this deliberately seeded speculative frontier.
    driver.set_u8("retire_kind", KIND_CONDITIONAL);
    driver.set_u8("retire_valid", 1);
    for step in 0..HISTORY_BITS {
        let taken = matches!(step % 11, 0 | 2 | 5 | 9);
        driver.set_u8("retire_taken", u8::from(taken));
        driver.tick();
        seeded_history = append_history(seeded_history, taken);
    }
    driver.set_u8("retire_valid", 0);
    assert_fold_frontier(&mut driver, true, seeded_history);

    let older_pc = 0x1800;
    let older_fallthrough = older_pc + 4;
    let older = driver.request_conditional(older_pc, older_fallthrough);
    assert_eq!(older.kind, KIND_NONE);
    driver.accept();
    let younger_pc = 0x1a00;
    let younger_fallthrough = younger_pc + 4;
    let younger = driver.request_conditional(younger_pc, younger_fallthrough);
    assert_eq!(younger.kind, KIND_NONE);
    driver.accept();
    assert_ne!(older.token, younger.token);

    // The older branch retires taken while a different, younger token repairs
    // early as not-taken on the same edge. Recovery must start from the
    // committed-next state, not committed_q or the stale speculative state.
    driver.set_u8("retire_token", older.token);
    driver.set_u32("retire_pc", older_pc);
    driver.set_u8("retire_kind", KIND_CONDITIONAL);
    driver.set_u8("retire_taken", 1);
    driver.set_u32("retire_target", older_pc - 4);
    driver.set_u32("retire_fallthrough", older_fallthrough);
    driver.set_u8("recover_token", younger.token);
    driver.set_u8("recover_kind", KIND_CONDITIONAL);
    driver.set_u8("recover_taken", 0);
    driver.set_u32("recover_fallthrough", younger_fallthrough);
    driver.set_u8("retire_valid", 1);
    driver.set_u8("recover_valid", 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    driver.set_u8("recover_valid", 0);

    let committed_history = append_history(seeded_history, true);
    let speculative_history = append_history(committed_history, false);
    assert_fold_frontier(&mut driver, true, committed_history);
    assert_fold_frontier(&mut driver, false, speculative_history);
}

#[test]
fn nonzero_tage_token_keeps_nine_bit_index_and_tag_snapshots() {
    let mut driver = Driver::new();
    let first_pc = 0x182;
    let first = driver.request(first_pc, first_pc + 2);
    let first_index = driver.get_tage("lookup_index0_q");
    let first_tag = driver.get_tage("lookup_tag0_q");
    driver.accept();
    let second_pc = 0x2c6;
    let second = driver.request(second_pc, second_pc + 2);
    let second_index = driver.get_tage("lookup_index0_q");
    let second_tag = driver.get_tage("lookup_tag0_q");
    driver.accept();
    assert_eq!(first.token, 0);
    assert_eq!(second.token, 1);
    assert_eq!(
        driver.get_tage_slot("slot_index0_q", first.token, 9),
        first_index
    );
    assert_eq!(
        driver.get_tage_slot("slot_index0_q", second.token, 9),
        second_index
    );
    assert_eq!(
        driver.get_tage_slot("slot_tag0_q", first.token, 9),
        first_tag
    );
    assert_eq!(
        driver.get_tage_slot("slot_tag0_q", second.token, 9),
        second_tag
    );
}

#[test]
fn cold_conditional_repair_and_repeated_taken_loop_learn_direction() {
    let mut driver = Driver::new();
    let pc = 0x1000;
    let fallthrough = pc + 4;
    let target = 0x0ff0;

    let cold = driver.request_conditional(pc, fallthrough);
    assert_eq!(
        cold,
        Response {
            token: 0,
            kind: KIND_NONE,
            taken: false,
            next_pc: fallthrough,
            btb_hit: false,
            tage_hit: false,
            ras_fallback: false,
        }
    );
    driver.accept();
    let cold_events = driver.retire(cold, pc, KIND_CONDITIONAL, true, target, fallthrough, true);
    assert!(cold_events.conditional_resolved);
    assert!(cold_events.direction_wrong);
    assert!(cold_events.btb_miss);

    // The cold BTB miss predicted no conditional at all.  CM recovery must
    // nevertheless append the actual outcome once to both history frontiers.
    assert_eq!(driver.get("committed_history_q") & 0xf, 0b0001);
    assert_eq!(driver.get("speculative_history_q") & 0xf, 0b0001);

    let second = driver.request_conditional(pc, fallthrough);
    assert!(second.btb_hit);
    assert_eq!(second.kind, KIND_CONDITIONAL);
    assert!(!second.taken);
    driver.accept();
    let second_events = driver.retire(
        second,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        true,
    );
    assert!(second_events.direction_wrong);
    assert!(!second_events.btb_miss);

    // Two direction misses raise the bimodal counter to taken.  The third
    // dynamic instance redirects at the frontend and needs no recovery. The
    // retire pipeline needs one idle edge to advance classification into its
    // registered write packet; lookup latency and throughput are unchanged.
    driver.tick();
    let third = driver.request_conditional(pc, fallthrough);
    assert!(third.btb_hit);
    assert_eq!(third.kind, KIND_CONDITIONAL);
    assert!(third.taken);
    assert_eq!(third.next_pc, target);
    driver.accept();
    let third_events = driver.retire(
        third,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        false,
    );
    assert!(!third_events.direction_wrong);

    let fourth = driver.request_conditional(pc, fallthrough);
    assert!(fourth.taken);
    assert_eq!(fourth.next_pc, target);
}

#[test]
fn overlapping_same_set_cold_misses_reserve_distinct_btb_ways() {
    let mut driver = Driver::new();
    let first_pc = 0x6004;
    let second_pc = first_pc + 0x100;
    let first_target = 0x7000;
    let second_target = 0x7100;

    assert_eq!((first_pc >> 1) & 0x7f, (second_pc >> 1) & 0x7f);

    // Accept both cold responses before either token retires.  The second
    // response must observe the first miss's reserved victim even though the
    // first entry has not reached a BTB write port yet.
    let first = driver.request(first_pc, first_pc + 4);
    assert!(!first.btb_hit);
    let first_reserved_way = (driver.get("slot_btb_way_q") >> first.token) & 1;
    driver.accept();

    let second = driver.request(second_pc, second_pc + 4);
    assert!(!second.btb_hit);
    let second_reserved_way = (driver.get("slot_btb_way_q") >> second.token) & 1;
    assert_ne!(
        first_reserved_way, second_reserved_way,
        "overlapping misses to one set must reserve different victims",
    );
    driver.accept();

    // Keep recovery out of this BTB-local scenario so both deliberately
    // outstanding snapshots can retire.  Recovery/flush behavior is covered
    // independently; here the direct kind makes each warm hit expose its
    // stored target through the public prediction response.
    driver.retire(
        first,
        first_pc,
        KIND_DIRECT,
        true,
        first_target,
        first_pc + 4,
        false,
    );
    driver.retire(
        second,
        second_pc,
        KIND_DIRECT,
        true,
        second_target,
        second_pc + 4,
        false,
    );

    let first_hit = driver.request(first_pc, first_pc + 4);
    assert!(first_hit.btb_hit);
    assert_eq!(first_hit.next_pc, first_target);
    let first_hit_way = (driver.get("slot_btb_way_q") >> first_hit.token) & 1;
    driver.accept();

    let second_hit = driver.request(second_pc, second_pc + 4);
    assert!(second_hit.btb_hit);
    assert_eq!(second_hit.next_pc, second_target);
    let second_hit_way = (driver.get("slot_btb_way_q") >> second_hit.token) & 1;
    assert_ne!(first_hit_way, second_hit_way);
}

#[test]
fn recovery_cancels_a_raw_miss_without_advancing_btb_plru() {
    check_raw_miss_flush_preserves_btb_victim(false);
}

#[test]
fn discard_cancels_a_raw_miss_without_advancing_btb_plru() {
    check_raw_miss_flush_preserves_btb_victim(true);
}

#[test]
fn recovery_rolls_back_a_decision_miss_btb_reservation() {
    check_decision_miss_flush_restores_btb_victim(false);
}

#[test]
fn discard_rolls_back_a_decision_miss_btb_reservation() {
    check_decision_miss_flush_restores_btb_victim(true);
}

#[test]
fn younger_stale_hit_cannot_invalidate_an_older_replacement() {
    let mut driver = Driver::new();
    let stale_pc = 0x6c04;
    let filler_pc = stale_pc + 0x100;
    let replacement_pc = stale_pc + 0x200;
    let replacement_target = 0x7c00;

    assert_eq!((stale_pc >> 1) & 0x7f, (filler_pc >> 1) & 0x7f);
    assert_eq!((stale_pc >> 1) & 0x7f, (replacement_pc >> 1) & 0x7f);

    // Fill both ways. The second fill leaves the first entry as the next
    // victim, so cold miss A reserves precisely the physical word containing
    // stale_pc without overwriting it yet.
    driver.learn_cold_control(stale_pc, stale_pc + 4, KIND_DIRECT, 0x7a00);
    driver.learn_cold_control(filler_pc, filler_pc + 4, KIND_DIRECT, 0x7b00);

    let older_a = driver.request(replacement_pc, replacement_pc + 4);
    assert!(!older_a.btb_hit);
    let replacement_way = (driver.get("slot_btb_way_q") >> older_a.token) & 1;
    driver.accept();

    // B still observes the old entry in A's reserved way. Both tokens remain
    // live until A retires and physically replaces that word.
    let younger_b = driver.request(stale_pc, stale_pc + 4);
    assert!(younger_b.btb_hit);
    assert_eq!(younger_b.next_pc, 0x7a00);
    let stale_way = (driver.get("slot_btb_way_q") >> younger_b.token) & 1;
    assert_eq!(stale_way, replacement_way);
    driver.accept();

    driver.retire(
        older_a,
        replacement_pc,
        KIND_DIRECT,
        true,
        replacement_target,
        replacement_pc + 4,
        false,
    );
    assert_eq!(
        (driver.get("slot_btb_identity_stale_q") >> younger_b.token) & 1,
        1,
        "A's physical overwrite did not stale B's saved identity",
    );

    // B resolves as non-control. Its prediction-time hit still has the same
    // event semantics, but its stale physical-way identity must suppress the
    // destructive invalidation command.
    driver.set_u8("retire_token", younger_b.token);
    driver.set_u32("retire_pc", stale_pc);
    driver.set_u8("retire_kind", KIND_NONE);
    driver.set_u8("retire_taken", 0);
    driver.set_u32("retire_target", stale_pc + 4);
    driver.set_u32("retire_fallthrough", stale_pc + 4);
    driver.set_u8("retire_valid", 1);
    assert_eq!(driver.get("btb_invalidate_candidate"), 1);
    assert_eq!(driver.get("btb_invalidate_valid"), 0);
    assert!(!driver.events().btb_miss);
    driver.tick();
    driver.set_u8("retire_valid", 0);

    let replacement = driver.request(replacement_pc, replacement_pc + 4);
    assert!(replacement.btb_hit, "B erased A's replacement entry");
    assert_eq!(replacement.next_pc, replacement_target);
}

#[test]
fn same_edge_old_hit_capture_and_older_replacement_marks_the_token_stale() {
    let mut driver = Driver::new();
    let stale_pc = 0x6e04;
    let filler_pc = stale_pc + 0x100;
    let replacement_pc = stale_pc + 0x200;
    let replacement_target = 0x7e00;

    assert_eq!((stale_pc >> 1) & 0x7f, (filler_pc >> 1) & 0x7f);
    assert_eq!((stale_pc >> 1) & 0x7f, (replacement_pc >> 1) & 0x7f);

    driver.learn_cold_control(stale_pc, stale_pc + 4, KIND_DIRECT, 0x7d00);
    driver.learn_cold_control(filler_pc, filler_pc + 4, KIND_DIRECT, 0x7d80);

    let older_a = driver.request(replacement_pc, replacement_pc + 4);
    assert!(!older_a.btb_hit);
    let replacement_way = (driver.get("slot_btb_way_q") >> older_a.token) & 1;
    driver.accept();

    // Launch B's lookup without using request(), so A can retire on the exact
    // edge that captures B's already-read old BTB word into its token slot.
    driver.set_u32("request_pc", stale_pc);
    driver.set_u32("request_fallthrough", stale_pc + 4);
    driver.set_u8("request_is_conditional", 0);
    driver.set_u8("request_valid", 1);
    driver.tick();
    driver.set_u8("request_valid", 0);
    // The enqueue edge is separate from the synchronous table issue edge.
    // Leave retirement for the later raw-response capture edge.
    driver.tick();

    driver.set_u8("retire_token", older_a.token);
    driver.set_u32("retire_pc", replacement_pc);
    driver.set_u8("retire_kind", KIND_DIRECT);
    driver.set_u8("retire_taken", 1);
    driver.set_u32("retire_target", replacement_target);
    driver.set_u32("retire_fallthrough", replacement_pc + 4);
    driver.set_u8("retire_valid", 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("btb_response_hit"), 1);
    assert_eq!(driver.get("btb_response_way"), replacement_way);
    assert_eq!(driver.get("btb_response_identity_mutated"), 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    driver.tick();

    let younger_b = driver.response();
    assert!(younger_b.btb_hit);
    assert_eq!(younger_b.next_pc, 0x7d00);
    assert_eq!(
        (driver.get("slot_btb_identity_stale_q") >> younger_b.token) & 1,
        1,
        "same-edge payload capture overwrote the physical-identity stale mark",
    );
    driver.accept();

    // Prediction-time hit/miss accounting remains unchanged. Only the stale
    // token's destructive physical-way invalidation is suppressed.
    driver.set_u8("retire_token", younger_b.token);
    driver.set_u32("retire_pc", stale_pc);
    driver.set_u8("retire_kind", KIND_NONE);
    driver.set_u8("retire_taken", 0);
    driver.set_u32("retire_target", stale_pc + 4);
    driver.set_u32("retire_fallthrough", stale_pc + 4);
    driver.set_u8("retire_valid", 1);
    assert_eq!(driver.get("btb_invalidate_candidate"), 1);
    assert_eq!(driver.get("btb_invalidate_valid"), 0);
    assert!(!driver.events().btb_miss);
    driver.tick();
    driver.set_u8("retire_valid", 0);

    let replacement = driver.request(replacement_pc, replacement_pc + 4);
    assert!(replacement.btb_hit, "B erased A's same-edge replacement");
    assert_eq!(replacement.next_pc, replacement_target);
}

#[test]
fn retiring_an_unrelated_control_cannot_steal_a_cold_miss_reservation() {
    let mut driver = Driver::new();
    let older_pc = 0x6208;
    let older_target = 0x6800;
    let first_pc = 0x6004;
    let second_pc = first_pc + 0x100;
    let first_target = 0x7000;
    let second_target = 0x7100;

    assert_ne!((older_pc >> 1) & 0x7f, (first_pc >> 1) & 0x7f);
    assert_eq!((first_pc >> 1) & 0x7f, (second_pc >> 1) & 0x7f);

    driver.learn_cold_control(older_pc, older_pc + 4, KIND_DIRECT, older_target);
    let older = driver.request(older_pc, older_pc + 4);
    assert!(older.btb_hit);
    driver.accept();

    // Launch miss A, then retire the unrelated older BTB hit exactly when A's
    // synchronous table response is captured.  Retirement still owns the BTB
    // entry write port, but it must not overwrite A's replacement-state
    // reservation for another set.
    driver.set_u32("request_pc", first_pc);
    driver.set_u32("request_fallthrough", first_pc + 4);
    driver.set_u8("request_is_conditional", 0);
    driver.set_u8("request_valid", 1);
    driver.tick();
    driver.set_u8("request_valid", 0);
    driver.tick();

    driver.set_u8("retire_token", older.token);
    driver.set_u32("retire_pc", older_pc);
    driver.set_u8("retire_kind", KIND_DIRECT);
    driver.set_u8("retire_taken", 1);
    driver.set_u32("retire_target", older_target);
    driver.set_u32("retire_fallthrough", older_pc + 4);
    driver.set_u8("retire_valid", 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("btb_update_valid"), 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    driver.tick();

    let first = driver.response();
    assert!(!first.btb_hit);
    let first_reserved_way = (driver.get("slot_btb_way_q") >> first.token) & 1;
    driver.accept();

    let second = driver.request(second_pc, second_pc + 4);
    assert!(!second.btb_hit);
    let second_reserved_way = (driver.get("slot_btb_way_q") >> second.token) & 1;
    assert_ne!(
        first_reserved_way, second_reserved_way,
        "an unrelated retirement must not steal miss A's PLRU write",
    );
    driver.accept();

    driver.retire(
        first,
        first_pc,
        KIND_DIRECT,
        true,
        first_target,
        first_pc + 4,
        false,
    );
    driver.retire(
        second,
        second_pc,
        KIND_DIRECT,
        true,
        second_target,
        second_pc + 4,
        false,
    );

    let first_hit = driver.request(first_pc, first_pc + 4);
    assert!(first_hit.btb_hit);
    assert_eq!(first_hit.next_pc, first_target);
    driver.accept();
    let second_hit = driver.request(second_pc, second_pc + 4);
    assert!(second_hit.btb_hit);
    assert_eq!(second_hit.next_pc, second_target);
}

#[test]
fn accepted_hit_makes_the_other_valid_way_next_victim() {
    let mut driver = Driver::new();
    let first_pc = 0x6404;
    let second_pc = first_pc + 0x100;
    let third_pc = second_pc + 0x100;

    driver.learn_cold_control(first_pc, first_pc + 4, KIND_DIRECT, 0x7400);
    driver.learn_cold_control(second_pc, second_pc + 4, KIND_DIRECT, 0x7500);

    let first_hit = driver.request(first_pc, first_pc + 4);
    assert!(first_hit.btb_hit);
    let first_way = (driver.get("slot_btb_way_q") >> first_hit.token) & 1;
    driver.accept();
    driver.retire(
        first_hit,
        first_pc,
        KIND_DIRECT,
        true,
        0x7400,
        first_pc + 4,
        false,
    );

    let third = driver.request(third_pc, third_pc + 4);
    assert!(!third.btb_hit);
    let third_way = (driver.get("slot_btb_way_q") >> third.token) & 1;
    assert_ne!(first_way, third_way, "the recently hit way was evicted");
    driver.accept();
    driver.retire(
        third,
        third_pc,
        KIND_DIRECT,
        true,
        0x7600,
        third_pc + 4,
        false,
    );

    assert!(driver.request(first_pc, first_pc + 4).btb_hit);
    driver.accept();
    assert!(!driver.request(second_pc, second_pc + 4).btb_hit);
}

#[test]
fn a_miss_uses_the_sole_invalid_way_even_when_plru_points_at_the_valid_way() {
    let mut driver = Driver::new();
    let first_pc = 0x6804;
    let second_pc = first_pc + 0x100;
    let third_pc = second_pc + 0x100;

    driver.learn_cold_control(first_pc, first_pc + 4, KIND_DIRECT, 0x7800);
    driver.learn_cold_control(second_pc, second_pc + 4, KIND_DIRECT, 0x7900);

    // Hitting the second entry points PLRU at the first, then retiring it as
    // non-control invalidates the second physical way.  The following cold
    // miss must choose that invalid way rather than evicting the valid first
    // entry selected by PLRU.
    let stale = driver.request(second_pc, second_pc + 4);
    assert!(stale.btb_hit);
    let invalidated_way = (driver.get("slot_btb_way_q") >> stale.token) & 1;
    driver.accept();
    driver.retire(
        stale,
        second_pc,
        KIND_NONE,
        false,
        second_pc + 4,
        second_pc + 4,
        true,
    );

    let third = driver.request(third_pc, third_pc + 4);
    assert!(!third.btb_hit);
    assert_eq!(
        (driver.get("slot_btb_way_q") >> third.token) & 1,
        invalidated_way,
        "the sole invalid physical way must override PLRU",
    );
    driver.accept();
    driver.retire(
        third,
        third_pc,
        KIND_DIRECT,
        true,
        0x7a00,
        third_pc + 4,
        false,
    );

    let first_hit = driver.request(first_pc, first_pc + 4);
    assert!(first_hit.btb_hit);
    assert_eq!(first_hit.next_pc, 0x7800);
    driver.accept();
    let third_hit = driver.request(third_pc, third_pc + 4);
    assert!(third_hit.btb_hit);
    assert_eq!(third_hit.next_pc, 0x7a00);
}

#[test]
fn cold_conditional_hint_appends_not_taken_before_actual_recovery() {
    let mut driver = Driver::new_with_fold_probes();

    // Give the all-zero and append-zero states distinct values, then make the
    // speculative and committed frontiers agree without allocating a token.
    driver.set_u8("retire_kind", KIND_CONDITIONAL);
    driver.set_u8("retire_taken", 1);
    driver.set_u8("retire_valid", 1);
    driver.set_u8("discard_all", 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    driver.set_u8("discard_all", 0);
    assert_fold_frontier(&mut driver, true, 1);
    assert_fold_frontier(&mut driver, false, 1);

    let pc = 0x2100;
    let fallthrough = pc + 4;
    let target = pc - 0x20;
    let cold = driver.request_conditional(pc, fallthrough);
    assert!(!cold.btb_hit);
    assert!(!cold.taken);

    // The instruction hint, not the missing BTB kind, contributes the
    // effective frontend direction to speculative history on acceptance.
    driver.accept();
    assert_fold_frontier(&mut driver, true, 1);
    assert_fold_frontier(&mut driver, false, append_history(1, false));

    let events = driver.retire(cold, pc, KIND_CONDITIONAL, true, target, fallthrough, true);
    assert!(events.direction_wrong);
    assert!(events.btb_miss);
    let repaired = append_history(1, true);
    assert_fold_frontier(&mut driver, true, repaired);
    assert_fold_frontier(&mut driver, false, repaired);
}

#[test]
fn direct_jal_stall_and_caller_suppression_obey_the_wrapper_contract() {
    let mut driver = Driver::new();
    let pc = 0x2202;
    let fallthrough = pc + 2;
    let target = 0x2800;
    driver.learn_cold_control(pc, fallthrough, KIND_DIRECT, target);

    let predicted = driver.request(pc, fallthrough);
    assert!(predicted.btb_hit);
    assert_eq!(predicted.kind, KIND_DIRECT);
    assert!(predicted.taken);
    assert_eq!(predicted.next_pc, target);

    // Both synchronous tables and wrapper metadata must hold their complete
    // response while the consumer is stalled.
    for _ in 0..4 {
        driver.tick();
        assert_eq!(driver.response(), predicted);
    }

    // A taken prediction suppresses the caller's fallthrough request by
    // leaving request_valid low.  Accepting it must not manufacture a lookup.
    driver.accept();
    assert_eq!(driver.get("response_valid"), 0);
    driver.tick();
    assert_eq!(driver.get("response_valid"), 0);
    let events = driver.retire(predicted, pc, KIND_DIRECT, true, target, fallthrough, false);
    assert!(!events.btb_miss);

    let following = driver.request(0x2280, 0x2284);
    assert_eq!(following.token, (predicted.token + 1) & 7);
    assert!(!following.btb_hit);
}

#[test]
fn fast_miss_holds_under_backpressure_and_pop_captures_on_accept() {
    let mut driver = Driver::new();
    let held = driver.request(0x2402, 0x2404);

    driver.set_u32("request_pc", 0x2602);
    driver.set_u32("request_fallthrough", 0x2604);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("decision_fast_miss"), 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u8("request_valid", 0);
    assert_eq!(driver.response(), held);
    for _ in 0..2 {
        assert_eq!(driver.get("request_accept"), 0);
        driver.tick();
        assert_eq!(driver.response(), held);
    }
    // A raw table result waits behind the held decision packet.  Accepting the
    // direct miss pops it and captures that result into the same register
    // without an output-stage bubble.
    driver.set_u8("response_accept", 1);
    assert_eq!(driver.get("fast_miss_accept"), 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    driver.tick();
    driver.set_u8("response_accept", 0);
    let following = driver.response();
    assert_eq!(following.token, (held.token + 1) & 7);
    assert_eq!(following.next_pc, 0x2604);
}

#[test]
fn taken_held_decision_cancels_an_already_pending_shadow() {
    let mut driver = Driver::new();
    let pc = 0x2e02;
    let fallthrough = pc + 2;
    let target = 0x2d00;

    driver.learn_cold_control(pc, fallthrough, KIND_CONDITIONAL, target);
    let second = driver.request_conditional(pc, fallthrough);
    driver.accept();
    driver.retire(
        second,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        true,
    );
    driver.tick();

    // Launch A, then keep the sequential frontend running until A reaches the
    // classified output.  At that point B occupies the metadata queue and C
    // occupies the raw table response: both are fall-through shadows.
    driver.set_u32("request_pc", pc);
    driver.set_u32("request_fallthrough", fallthrough);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();

    driver.set_u32("request_pc", fallthrough);
    driver.set_u32("request_fallthrough", fallthrough + 2);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();

    driver.set_u32("request_pc", fallthrough + 2);
    driver.set_u32("request_fallthrough", fallthrough + 4);
    // A is captured and B is issued on this edge; C replaces the ingress
    // packet. The following edge promotes A and issues C while C's table
    // response remains the cancelable shadow.
    driver.tick();
    assert_eq!(driver.get("decision_promote"), 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.set_u8("request_valid", 0);
    driver.tick();

    let taken = driver.response();
    assert!(taken.taken);
    let reusable_token = (taken.token + 1) & 7;
    assert_eq!(driver.get("decision_valid_q"), 1);
    assert_eq!(driver.get("btb_response_valid"), 1);
    assert_eq!(driver.get("tage_response_valid"), 1);
    assert_eq!(driver.get("decision_squash"), 1);
    assert_eq!(driver.get("table_response_cancel"), 1);
    assert_eq!(driver.get("request_accept"), 0);

    // Holding A does not delay cleanup: the registered taken bit squashes B,
    // cancels C, and rolls the token frontier back exactly once.
    driver.tick();
    assert_eq!(driver.response(), taken);
    assert_eq!(driver.get("decision_valid_q"), 0);
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    assert_eq!(driver.get("allocation_token_q"), u64::from(reusable_token));

    driver.accept();
    assert_eq!(driver.get("response_valid"), 0);
}

#[test]
fn held_tage_response_is_immutable_while_raw_slot_tracks_training() {
    let mut driver = Driver::new();
    let pc = 0x3402;
    let held = driver.request(pc, pc + 4);
    assert!(!held.tage_hit);
    // The table response has already moved into the wrapper's decision
    // register. Its mutable training snapshot remains live by token.
    assert_ne!(driver.get_tage("slot_valid_q") & (1 << held.token), 0);

    // Model a physical B-table write to the held response's row. The raw slot
    // must observe it, but the already-visible prediction may not change.
    driver.set_tage_u8("train_packet_base_write_q", 1);
    driver.set_tage_u16("train_packet_base_addr_q", ((pc >> 1) & 0x7ff) as u16);
    driver.set_tage_u8("train_packet_base_data_q", 3);
    assert_eq!(driver.response(), held);
    driver.tick();

    assert_eq!(driver.response(), held);
    assert_eq!(driver.get_tage_slot("slot_base_entry_q", held.token, 2), 3);
}

#[test]
fn held_provider_replacement_does_not_train_the_new_tag() {
    let mut driver = Driver::new();
    let old_pc = 0x3802;
    let new_pc = old_pc ^ (1 << 16);

    // Probe both hashes without changing speculative conditional history.
    let _ = driver.request(old_pc, old_pc + 4);
    let index = driver.get_tage("lookup_index0_q") as u16;
    let old_tag = driver.get_tage("lookup_tag0_q") as u16;
    driver.set_u8("discard_all", 1);
    driver.tick();
    driver.set_u8("discard_all", 0);
    let _ = driver.request(new_pc, new_pc + 4);
    assert_eq!(driver.get_tage("lookup_index0_q") as u16, index);
    let new_tag = driver.get_tage("lookup_tag0_q") as u16;
    assert_ne!(new_tag, old_tag);
    driver.set_u8("discard_all", 1);
    driver.tick();
    driver.set_u8("discard_all", 0);

    let old_word = 0x8000 | (old_tag << 6) | (0b011 << 3) | (0b10 << 1);
    driver.set_tage_u8("train_packet_tag_write0_q", 1);
    driver.set_tage_u16("train_packet_tag_addr0_q", index);
    driver.set_tage_u16("train_packet_tag_data0_q", old_word);
    driver.tick();

    let held = driver.request(old_pc, old_pc + 4);
    assert_eq!(driver.get_tage("response_hit"), 1);
    assert_eq!(driver.get_tage("response_taken"), 1);
    driver.tick();
    assert_eq!(driver.get_tage("response_metadata_captured_q"), 1);

    // Replace the prediction-time provider row while its response is held.
    let replacement = 0x8000 | (new_tag << 6) | (0b100 << 3) | (0b10 << 1);
    driver.set_tage_u8("train_packet_tag_write0_q", 1);
    driver.set_tage_u16("train_packet_tag_addr0_q", index);
    driver.set_tage_u16("train_packet_tag_data0_q", replacement);
    driver.tick();
    assert_eq!(driver.get_tage("response_hit"), 1);
    assert_eq!(driver.get_tage("response_taken"), 1);
    assert_eq!(
        driver.get_tage_slot("slot_entry0_q", held.token, 16),
        u64::from(replacement)
    );

    driver.accept();
    driver.retire(
        held,
        old_pc,
        KIND_CONDITIONAL,
        true,
        old_pc - 4,
        old_pc + 4,
        false,
    );
    for _ in 0..4 {
        driver.tick();
    }

    // The prediction-time source-identity mismatch suppresses provider
    // training, leaving the replacement tag and counter untouched.
    let _ = driver.request(new_pc, new_pc + 4);
    assert_eq!(driver.get_tage("response_hit"), 1);
    assert_eq!(driver.get_tage("response_taken"), 0);
    assert_eq!(driver.get_tage("response_entry0"), u64::from(replacement));
}

#[test]
fn aging_rmw_commits_the_alternating_usefulness_clear_to_memory() {
    let mut driver = Driver::new();
    let pc = 0x3c02;
    let _ = driver.request(pc, pc + 4);
    let index = driver.get_tage("lookup_index0_q") as u16;
    let tag = driver.get_tage("lookup_tag0_q") as u16;
    driver.set_u8("discard_all", 1);
    driver.tick();
    driver.set_u8("discard_all", 0);

    let original = 0x8000 | (tag << 6) | (0b011 << 3) | (0b11 << 1);
    let aged = original & !0b100;
    driver.set_tage_u8("train_packet_tag_write0_q", 1);
    driver.set_tage_u16("train_packet_tag_addr0_q", index);
    driver.set_tage_u16("train_packet_tag_data0_q", original);
    driver.tick();

    // Complete one real Port-B aging read/modify/write for bank zero. Mark the
    // other banks done only to terminate the directed row promptly.
    driver.set_tage_u8("age_active_q", 1);
    driver.set_tage_u16("age_address_q", index);
    driver.set_tage_u8("age_clear_msb_q", 1);
    driver.set_tage_u8("age_pending0_q", 0);
    driver.set_tage_u8("age_done0_q", 0);
    driver.set_tage_u8("age_done1_q", 1);
    driver.set_tage_u8("age_done2_q", 1);
    driver.set_tage_u8("age_done3_q", 1);
    assert_eq!(driver.get_tage("age_read_enable0"), 1);
    driver.tick();
    assert_eq!(driver.get_tage("age_write_enable0"), 1);
    assert_eq!(driver.get_tage("tag_portb_data0"), u64::from(aged));
    driver.tick();
    driver.set_tage_u8("age_active_q", 0);

    let _ = driver.request(pc, pc + 4);
    assert_eq!(driver.get_tage("response_entry0"), u64::from(aged));
    assert_eq!(driver.get_tage("response_hit"), 1);
}

#[test]
fn recovery_and_discard_reject_a_same_edge_request() {
    // Recovery clears a younger held response, restores the token frontier to
    // the resolved older instruction, and must not admit the concurrent PC.
    let mut recovering = Driver::new();
    let older = recovering.request(0x2802, 0x2804);
    recovering.accept();
    let younger = recovering.request(0x2a02, 0x2a04);
    assert_eq!(younger.token, (older.token + 1) & 7);
    recovering.set_u8("recover_token", older.token);
    recovering.set_u8("recover_kind", KIND_NONE);
    recovering.set_u32("request_pc", 0x2c02);
    recovering.set_u32("request_fallthrough", 0x2c04);
    recovering.set_u8("request_valid", 1);
    // Exercise the S0 flush itself: the request is accepted while the older
    // miss response is held, then recovery must invalidate the queued packet
    // before it can issue to either table.
    assert_eq!(recovering.get("request_accept"), 1);
    recovering.tick();
    recovering.set_u8("recover_valid", 1);
    assert_eq!(recovering.get("ingress_valid_q"), 1);
    assert_eq!(recovering.get("request_accept"), 0);
    recovering.tick();
    recovering.set_u8("request_valid", 0);
    recovering.set_u8("recover_valid", 0);
    assert_eq!(recovering.get("response_valid"), 0);
    assert_eq!(recovering.get("decision_valid_q"), 0);
    assert_eq!(recovering.get("output_valid_q"), 0);
    assert_eq!(recovering.get("btb_response_valid"), 0);
    assert_eq!(recovering.get("tage_response_valid"), 0);
    assert_eq!(recovering.get("slot_valid_q") & (1 << younger.token), 0);
    assert_eq!(recovering.get_tage("slot_valid_q"), 0);
    let recovered_allocation = recovering.get("allocation_token_q");
    for _ in 0..3 {
        recovering.tick();
        assert_eq!(recovering.get("response_valid"), 0);
        assert_eq!(recovering.get("raw_response_capture"), 0);
        assert_eq!(recovering.get("decision_valid_q"), 0);
        assert_eq!(recovering.get("allocation_token_q"), recovered_allocation);
    }
    let after_recovery = recovering.request(0x2e02, 0x2e04);
    assert_eq!(after_recovery.token, (older.token + 1) & 7);
    assert_eq!(after_recovery.next_pc, 0x2e04);

    // A predictor-wide discard has the same request-suppression rule and
    // resets allocation to token zero.
    let mut discarding = Driver::new();
    let held = discarding.request(0x3002, 0x3004);
    assert_eq!(held.token, 0);
    discarding.set_u32("request_pc", 0x3202);
    discarding.set_u32("request_fallthrough", 0x3204);
    discarding.set_u8("request_valid", 1);
    assert_eq!(discarding.get("request_accept"), 1);
    discarding.tick();
    discarding.set_u8("discard_all", 1);
    assert_eq!(discarding.get("ingress_valid_q"), 1);
    assert_eq!(discarding.get("request_accept"), 0);
    discarding.tick();
    discarding.set_u8("request_valid", 0);
    discarding.set_u8("discard_all", 0);
    assert_eq!(discarding.get("response_valid"), 0);
    assert_eq!(discarding.get("decision_valid_q"), 0);
    assert_eq!(discarding.get("output_valid_q"), 0);
    assert_eq!(discarding.get("btb_response_valid"), 0);
    assert_eq!(discarding.get("tage_response_valid"), 0);
    assert_eq!(discarding.get("slot_valid_q"), 0);
    assert_eq!(discarding.get_tage("slot_valid_q"), 0);
    for _ in 0..3 {
        discarding.tick();
        assert_eq!(discarding.get("response_valid"), 0);
        assert_eq!(discarding.get("raw_response_capture"), 0);
        assert_eq!(discarding.get("decision_valid_q"), 0);
        assert_eq!(discarding.get("allocation_token_q"), 0);
    }
    let after_discard = discarding.request(0x3402, 0x3404);
    assert_eq!(after_discard.token, 0);
    assert_eq!(after_discard.next_pc, 0x3404);
}

#[test]
fn recovery_flushes_tage_slots_only_when_no_future_training_snapshot_is_needed() {
    // Current EX recovery is a non-conditional JAL/JALR. Its wrapper token is
    // retained for BTB retirement metadata, but TAGE will never train it, so
    // every TAGE snapshot (including wrong-path younger tokens) can be freed.
    let mut direct = Driver::new();
    let older = direct.request(0x3502, 0x3504);
    direct.accept();
    let resolved = direct.request(0x3602, 0x3604);
    direct.accept();
    let younger = direct.request(0x3702, 0x3704);
    direct.accept();
    assert_eq!(direct.get_tage("slot_valid_q") & 0b111, 0b111);

    direct.set_u8("retire_token", older.token);
    direct.set_u8("retire_kind", KIND_NONE);
    direct.set_u8("recover_token", resolved.token);
    direct.set_u8("recover_kind", KIND_DIRECT);
    direct.set_u8("recover_taken", 1);
    direct.set_u32("recover_fallthrough", 0x3604);
    direct.set_u8("retire_valid", 1);
    direct.set_u8("recover_valid", 1);
    assert_eq!(direct.get("tage_flush_slots"), 1);
    assert_eq!(direct.get_tage("flush_slots"), 1);
    direct.tick();
    direct.set_u8("retire_valid", 0);
    direct.set_u8("recover_valid", 0);
    let tage_valid = direct.get_tage("slot_valid_q") & 0b111;
    let wrapper_valid = direct.get("slot_valid_q") & 0b111;
    assert_eq!(
        (tage_valid, wrapper_valid),
        (0, 1 << resolved.token),
        "recovery slot cleanup"
    );
    assert_ne!(resolved.token, younger.token);

    // The API also supports a future conditional resolver before retirement.
    // That resolved token still needs its prediction-time TAGE snapshot when
    // it eventually trains, so this recovery must not globally flush slots.
    let mut conditional = Driver::new();
    let resolved = conditional.request(0x3802, 0x3804);
    conditional.accept();
    let _younger = conditional.request(0x3902, 0x3904);
    conditional.accept();
    conditional.set_u8("recover_token", resolved.token);
    conditional.set_u8("recover_kind", KIND_CONDITIONAL);
    conditional.set_u8("recover_taken", 0);
    conditional.set_u32("recover_fallthrough", 0x3804);
    conditional.set_u8("recover_valid", 1);
    conditional.tick();
    conditional.set_u8("recover_valid", 0);
    assert_ne!(
        conditional.get_tage("slot_valid_q") & (1 << resolved.token),
        0
    );
}

#[test]
fn call_return_ras_uses_compressed_link_instead_of_stale_btb_target() {
    let mut driver = Driver::new();
    let first_call = 0x3002;
    let first_link = first_call + 2;
    let second_call = 0x3482;
    let second_link = second_call + 2;
    let return_pc = 0x3842;
    let callee = 0x3c00;

    // The first call gives the return instruction a deliberately stale BTB
    // fallback target, then returns and empties the RAS.
    driver.learn_cold_control(first_call, first_link, KIND_CALL, callee);
    driver.learn_cold_control(return_pc, return_pc + 2, KIND_RETURN, first_link);

    // A different compressed call pushes a different PC+2 link.  The learned
    // return entry still contains first_link, so selecting second_link proves
    // that the RAS, not the BTB target, supplied the prediction.
    driver.learn_cold_control(second_call, second_link, KIND_CALL, callee);
    let predicted_return = driver.request(return_pc, return_pc + 2);
    assert!(predicted_return.btb_hit);
    assert_eq!(predicted_return.kind, KIND_RETURN);
    assert!(predicted_return.taken);
    assert_eq!(predicted_return.next_pc, second_link);
    assert_ne!(predicted_return.next_pc, first_link);
    assert!(!predicted_return.ras_fallback);
    driver.accept();
    let return_events = driver.retire(
        predicted_return,
        return_pc,
        KIND_RETURN,
        true,
        second_link,
        return_pc + 2,
        false,
    );
    assert!(!return_events.ras_fallback);

    let predicted_call = driver.request(second_call, second_link);
    assert!(predicted_call.btb_hit);
    assert_eq!(predicted_call.kind, KIND_CALL);
    assert_eq!(predicted_call.next_pc, callee);
}

#[test]
fn ras_recovery_applies_older_retire_before_younger_coroutine() {
    let mut driver = Driver::new();
    let call_pc = 0x6202;
    let call_link = call_pc + 2;
    let return_pc = 0x6402;
    let return_fallthrough = return_pc + 2;
    let callee = 0x6800;
    let fallback = 0x6c00;

    // Learn both BTB entries, then consume the training call so that both RAS
    // frontiers start empty.
    driver.learn_cold_control(return_pc, return_fallthrough, KIND_RETURN, fallback);
    driver.learn_cold_control(call_pc, call_link, KIND_CALL, callee);
    let cleanup = driver.request(return_pc, return_fallthrough);
    assert_eq!(cleanup.next_pc, call_link);
    driver.accept();
    driver.retire(
        cleanup,
        return_pc,
        KIND_RETURN,
        true,
        call_link,
        return_fallthrough,
        false,
    );

    // The older call has updated speculative state but has not retired when a
    // younger cold control transfer resolves as a coroutine.  Recovery must
    // copy committed state *after* the older retirement, then perform the
    // younger pop+push, leaving younger_link at the speculative top.
    let older = driver.request(call_pc, call_link);
    assert_eq!(older.kind, KIND_CALL);
    driver.accept();
    let younger_pc = 0x7002;
    let younger_link = younger_pc + 2;
    let younger = driver.request(younger_pc, younger_link);
    assert_eq!(younger.kind, KIND_NONE);
    driver.accept();

    driver.set_u8("retire_token", older.token);
    driver.set_u32("retire_pc", call_pc);
    driver.set_u8("retire_kind", KIND_CALL);
    driver.set_u8("retire_taken", 1);
    driver.set_u32("retire_target", callee);
    driver.set_u32("retire_fallthrough", call_link);
    driver.set_u8("recover_token", younger.token);
    driver.set_u8("recover_kind", KIND_COROUTINE);
    driver.set_u8("recover_taken", 1);
    driver.set_u32("recover_fallthrough", younger_link);
    driver.set_u8("retire_valid", 1);
    driver.set_u8("recover_valid", 1);
    driver.tick();
    driver.set_u8("retire_valid", 0);
    driver.set_u8("recover_valid", 0);

    let repaired_return = driver.request(return_pc, return_fallthrough);
    assert_eq!(repaired_return.kind, KIND_RETURN);
    assert_eq!(repaired_return.next_pc, younger_link);
    assert!(!repaired_return.ras_fallback);
}

#[test]
fn ras_overlay_owner_preserves_younger_same_word_coroutine() {
    let mut driver = Driver::new();
    let call_pc = 0x8202;
    let coroutine_pc = 0x8402;
    let return_pc = 0x8602;
    let return_fallthrough = return_pc + 2;
    let target = 0x8800;

    driver.learn_cold_control(return_pc, return_fallthrough, KIND_RETURN, 0x8a00);
    driver.learn_cold_control(call_pc, call_pc + 2, KIND_CALL, target);
    let pop_call_training = driver.request(return_pc, return_fallthrough);
    driver.accept();
    driver.retire(
        pop_call_training,
        return_pc,
        KIND_RETURN,
        true,
        call_pc + 2,
        return_fallthrough,
        false,
    );
    driver.learn_cold_control(coroutine_pc, coroutine_pc + 2, KIND_COROUTINE, target);
    let pop_coroutine_training = driver.request(return_pc, return_fallthrough);
    driver.accept();
    driver.retire(
        pop_coroutine_training,
        return_pc,
        KIND_RETURN,
        true,
        coroutine_pc + 2,
        return_fallthrough,
        false,
    );

    // Both predictions write logical word zero.  The coroutine is younger and
    // replaces the call's overlay owner before the call retires.  Committing
    // the older call must update the base word without clearing the younger
    // overlay that is still architecturally speculative.
    let older_link = 0x9002;
    let older_call = driver.request(call_pc, older_link);
    driver.accept();
    let younger_link = 0x9202;
    let younger_coroutine = driver.request(coroutine_pc, younger_link);
    assert_eq!(younger_coroutine.kind, KIND_COROUTINE);
    driver.accept();
    driver.retire(
        older_call, call_pc, KIND_CALL, true, target, older_link, false,
    );

    let predicted_return = driver.request(return_pc, return_fallthrough);
    assert_eq!(predicted_return.next_pc, younger_link);
    assert!(!predicted_return.ras_fallback);
}

#[test]
fn ras_depth_saturates_as_a_circular_stack_then_falls_back() {
    let mut driver = Driver::new();
    let call_pc = 0x7202;
    let return_pc = 0x7402;
    let return_fallthrough = return_pc + 2;
    let callee = 0x7800;
    let fallback = 0x7c00;

    driver.learn_cold_control(return_pc, return_fallthrough, KIND_RETURN, fallback);
    driver.learn_cold_control(call_pc, call_pc + 2, KIND_CALL, callee);
    let cleanup = driver.request(return_pc, return_fallthrough);
    driver.accept();
    driver.retire(
        cleanup,
        return_pc,
        KIND_RETURN,
        true,
        call_pc + 2,
        return_fallthrough,
        false,
    );

    // Seventeen pushes overwrite only the oldest entry of the depth-16
    // circular stack.  The most recent sixteen links must still pop in order.
    let links = (0..17).map(|index| 0x8002 + index * 2).collect::<Vec<_>>();
    for &link in &links {
        let call = driver.request(call_pc, link);
        assert_eq!(call.kind, KIND_CALL);
        driver.accept();
        driver.retire(call, call_pc, KIND_CALL, true, callee, link, false);
    }

    for &expected in links[1..].iter().rev() {
        let return_prediction = driver.request(return_pc, return_fallthrough);
        assert_eq!(return_prediction.next_pc, expected);
        assert!(!return_prediction.ras_fallback);
        driver.accept();
        driver.retire(
            return_prediction,
            return_pc,
            KIND_RETURN,
            true,
            expected,
            return_fallthrough,
            false,
        );
    }

    let empty_return = driver.request(return_pc, return_fallthrough);
    // Every retired return refreshes the BTB fallback; after the final pop it
    // therefore contains that return's actual target, links[1].
    assert_eq!(empty_return.next_pc, links[1]);
    assert!(empty_return.ras_fallback);
}

#[test]
fn false_btb_type_is_invalidated_and_recovery_restores_frontend_state() {
    let mut driver = Driver::new();
    let pc = 0x4242;
    let fallthrough = pc + 2;
    let stale_target = 0x4800;
    driver.learn_cold_control(pc, fallthrough, KIND_DIRECT, stale_target);

    let false_hit = driver.request(pc, fallthrough);
    assert!(false_hit.btb_hit);
    assert_eq!(false_hit.kind, KIND_DIRECT);
    assert_eq!(false_hit.next_pc, stale_target);
    driver.accept();
    driver.retire(
        false_hit,
        pc,
        KIND_NONE,
        false,
        fallthrough,
        fallthrough,
        true,
    );

    let repaired = driver.request(pc, fallthrough);
    assert!(!repaired.btb_hit);
    assert_eq!(repaired.kind, KIND_NONE);
    assert!(!repaired.taken);
    assert_eq!(repaired.next_pc, fallthrough);
}

#[test]
fn ras_fallback_event_requires_an_actual_return_or_coroutine() {
    let mut driver = Driver::new();
    let pc = 0x4a42;
    let fallthrough = pc + 2;
    let stale_target = 0x4c00;

    // Learn a return BTB type while the RAS is empty. The next lookup must use
    // the BTB fallback, but resolution proves that the instruction is not a
    // return. That stale prediction must not pollute the retired-RAS metric.
    driver.learn_cold_control(pc, fallthrough, KIND_RETURN, stale_target);
    let false_return = driver.request(pc, fallthrough);
    assert_eq!(false_return.kind, KIND_RETURN);
    assert!(false_return.ras_fallback);
    driver.accept();
    let events = driver.retire(
        false_return,
        pc,
        KIND_NONE,
        false,
        fallthrough,
        fallthrough,
        true,
    );
    assert!(!events.ras_fallback);
}

#[test]
fn not_taken_conditional_launches_a_visible_successor_with_nt_history() {
    let mut driver = Driver::new_with_fold_probes();
    let pc = 0x5102;
    let fallthrough = pc + 2;
    let target = 0x5000;
    let successor_pc = fallthrough;
    let successor_fallthrough = successor_pc + 2;

    // A cold taken resolution installs the conditional BTB entry, while its
    // first warm prediction remains not-taken.
    driver.learn_cold_control(pc, fallthrough, KIND_CONDITIONAL, target);
    let response_a = driver.request_conditional(pc, fallthrough);
    assert_eq!(response_a.kind, KIND_CONDITIONAL);
    assert!(!response_a.taken);

    // A's history effect is already committed at classifier promotion. Present
    // B while A is stalled; accepting A launches a normal visible response
    // from the registered NT preview without applying A a second time.
    driver.set_u32("request_pc", successor_pc);
    driver.set_u32("request_fallthrough", successor_fallthrough);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    let lookup_index = driver.get("tage_lookup_index_0") as u16;
    let old_history = driver.get_u128("speculative_history_q");
    driver.set_u8("response_accept", 1);
    assert_eq!(driver.get("tage_lookup_index_0") as u16, lookup_index);

    // This edge consumes A and launches B.  A cold B crosses the synchronous
    // table read into the registered decision packet, where the miss fast path
    // becomes visible without entering the classifier output queue.
    driver.tick();
    driver.set_u8("request_valid", 0);
    driver.set_u8("response_accept", 0);
    driver.tick();
    // B was accepted into the ingress on A's acceptance edge; issue and
    // response capture are separate edges now.
    driver.tick();
    assert_eq!(driver.get("decision_fast_miss"), 1);
    assert_fold_frontier(&mut driver, false, old_history);
    let response_b = driver.response();
    assert_eq!(response_b.token, (response_a.token + 1) & 7);
    assert!(!response_b.btb_hit);
    assert_eq!(response_b.next_pc, successor_fallthrough);

    // B's exact conditional hint was captured even though its BTB missed.  Its
    // NT history effect is applied exactly once when the held fast response is
    // accepted.
    driver.accept();
    assert_fold_frontier(&mut driver, false, append_history(old_history, false));
}

#[test]
fn fast_miss_path_sustains_direct_accept_pop_capture_and_push() {
    let mut driver = Driver::new();
    let pcs = [0x5202_u32, 0x5222, 0x5242, 0x5262, 0x5282, 0x52a2, 0x52c2];

    // Launch A, then capture its raw miss while launching B.  A is now held in
    // the decision register and B occupies the synchronous table response.
    driver.set_u32("request_pc", pcs[0]);
    driver.set_u32("request_fallthrough", pcs[0] + 2);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u32("request_pc", pcs[1]);
    driver.set_u32("request_fallthrough", pcs[1] + 2);
    driver.tick();
    driver.set_u32("request_pc", pcs[2]);
    driver.set_u32("request_fallthrough", pcs[2] + 2);
    // A is captured while B issues and C replaces the ingress on this edge.
    driver.tick();
    driver.set_u8("request_valid", 0);

    // Four cold misses are now accepted on consecutive edges.  Every edge
    // directly accepts the current decision, captures the next raw result,
    // and launches its successor; none enters the classified output stage.
    for cycle in 0..4 {
        let response = driver.response();
        let expected_token = cycle as u8;
        assert_eq!(response.token, expected_token);
        assert!(!response.btb_hit);
        assert!(!response.taken);
        assert_eq!(response.next_pc, pcs[cycle] + 2);
        assert_ne!(driver.get("slot_valid_q") & (1 << expected_token), 0);
        assert_ne!(driver.get_tage("slot_valid_q") & (1 << expected_token), 0);
        assert_eq!(
            driver.get_tage_slot("slot_index0_q", expected_token, 9),
            u64::from((pcs[cycle] >> 1) & 0x1ff),
        );
        driver.set_u32("request_pc", pcs[cycle + 3]);
        driver.set_u32("request_fallthrough", pcs[cycle + 3] + 2);
        driver.set_u8("request_valid", 1);
        driver.set_u8("response_accept", 1);
        assert_eq!(driver.get("decision_fast_miss"), 1);
        assert_eq!(driver.get("fast_miss_accept"), 1);
        assert_eq!(driver.get("decision_promote"), 0);
        assert_eq!(driver.get("raw_response_capture"), 1);
        assert_eq!(driver.get("request_accept"), 1);
        driver.tick();
    }
    driver.set_u8("request_valid", 0);
    driver.set_u8("response_accept", 0);
}

#[test]
fn taken_prediction_cancels_conditional_shadow_and_reuses_its_token() {
    let mut driver = Driver::new_with_fold_probes();
    let pc = 0x5502;
    let fallthrough = pc + 2;
    // Give the target and canceled sequential shadow the same BTB set.  This
    // makes the target's chosen way expose any accidental replacement-state
    // update by the shadow.
    let target = 0x5404;
    assert_eq!((fallthrough >> 1) & 0x7f, (target >> 1) & 0x7f);

    // Drive the base predictor to taken, allowing its registered training
    // packet to reach the physical memory before response A is requested.
    driver.learn_cold_control(pc, fallthrough, KIND_CONDITIONAL, target);
    let second = driver.request_conditional(pc, fallthrough);
    assert!(!second.taken);
    driver.accept();
    driver.retire(
        second,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        true,
    );
    driver.tick();

    let old_history = driver.get_u128("speculative_history_q");

    // Keep the sequential frontend full while A crosses table, metadata, and
    // classifier stages.  When A becomes visible, B is in the metadata queue
    // and C is the outstanding raw response.
    driver.set_u32("request_pc", pc);
    driver.set_u32("request_fallthrough", fallthrough);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u32("request_pc", fallthrough);
    driver.set_u32("request_fallthrough", fallthrough + 2);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u32("request_pc", fallthrough + 2);
    driver.set_u32("request_fallthrough", fallthrough + 4);
    // A is captured and B is issued on this edge; C replaces the ingress
    // packet. The following edge promotes A and issues C while C's table
    // response remains the cancelable shadow.
    driver.tick();
    assert_eq!(driver.get("decision_promote"), 1);
    assert_eq!(driver.get("raw_response_capture"), 1);
    assert_eq!(driver.get("request_accept"), 1);
    driver.set_u8("request_valid", 0);
    driver.tick();

    let response_a = driver.response();
    assert_eq!(response_a.kind, KIND_CONDITIONAL);
    assert!(response_a.taken);
    assert_eq!(response_a.next_pc, target);
    let reusable_token = (response_a.token + 1) & 7;
    let taken_history = append_history(old_history, true);
    assert_fold_frontier(&mut driver, false, taken_history);
    assert_eq!(driver.get("decision_valid_q"), 1);
    assert_eq!(driver.get("btb_response_valid"), 1);
    assert_eq!(driver.get("tage_response_valid"), 1);
    assert_eq!(driver.get("decision_squash"), 1);
    assert_eq!(driver.get("table_response_cancel"), 1);
    assert_eq!(driver.get("raw_response_capture"), 0);

    // A target presented while the taken output is held cannot race the
    // shadow cleanup.  This edge squashes B, cancels C, and restores both the
    // token frontier and B's provisional BTB replacement claim.
    driver.set_u32("request_pc", target);
    driver.set_u32("request_fallthrough", target + 4);
    driver.set_u8("request_is_conditional", 0);
    driver.set_u8("request_valid", 1);
    assert_eq!(driver.get("request_accept"), 0);
    driver.tick();
    assert_eq!(driver.response(), response_a);
    assert_fold_frontier(&mut driver, false, taken_history);
    assert_eq!(driver.get("decision_valid_q"), 0);
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    assert_eq!(driver.get("allocation_token_q"), u64::from(reusable_token));
    assert_eq!(driver.get("slot_valid_q") & (1 << reusable_token), 0);
    assert_eq!(driver.get_tage("slot_valid_q") & (1 << reusable_token), 0);

    // Even A's acceptance edge remains closed to requests because ready is a
    // function of the currently registered output.  The cold target launches
    // on the following edge and becomes visible from the decision miss path.
    driver.set_u8("response_accept", 1);
    assert_eq!(driver.get("request_accept"), 0);
    driver.tick();
    driver.set_u8("response_accept", 0);
    assert_eq!(driver.get("request_accept"), 1);
    driver.tick();
    driver.set_u8("request_valid", 0);
    driver.tick();
    driver.tick();
    assert_eq!(driver.get("decision_fast_miss"), 1);

    let target_response = driver.response();
    assert_eq!(target_response.token, reusable_token);
    assert_eq!(target_response.next_pc, target + 4);
    assert_eq!(
        (driver.get("slot_btb_way_q") >> target_response.token) & 1,
        0,
        "a canceled shadow must not consume its BTB set's initial victim",
    );
    assert_fold_frontier(&mut driver, false, taken_history);
    assert_ne!(
        driver.get_tage("slot_valid_q") & (1 << reusable_token),
        0,
        "the target lookup, rather than the canceled shadow, owns the reused token",
    );

    // Accepting the non-conditional target proves that the shadow's captured
    // conditional hint was overwritten rather than appended one cycle late.
    driver.accept();
    assert_fold_frontier(&mut driver, false, taken_history);
}

#[test]
fn cancelled_shadow_self_consumes_without_a_replacement_request() {
    let mut driver = Driver::new();
    let pc = 0x5802;
    let fallthrough = pc + 2;
    let target = 0x5700;

    // Make the base prediction taken, then let its registered training packet
    // reach memory before launching the response under test.
    driver.learn_cold_control(pc, fallthrough, KIND_CONDITIONAL, target);
    let second = driver.request_conditional(pc, fallthrough);
    assert!(!second.taken);
    driver.accept();
    driver.retire(
        second,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        true,
    );
    driver.tick();

    // Fill A/B/C exactly as a sequential frontend does before A's registered
    // taken direction is available.
    driver.set_u32("request_pc", pc);
    driver.set_u32("request_fallthrough", fallthrough);
    driver.set_u8("request_is_conditional", 1);
    driver.set_u8("request_valid", 1);
    driver.tick();
    driver.set_u32("request_pc", fallthrough);
    driver.set_u32("request_fallthrough", fallthrough + 2);
    driver.tick();
    driver.set_u32("request_pc", fallthrough + 2);
    driver.set_u32("request_fallthrough", fallthrough + 4);
    driver.tick();
    driver.set_u8("request_valid", 0);
    // The classifier/output register adds one edge after the third request
    // has been issued; keep the already-enqueued C packet from being replaced
    // by a duplicate while A is promoted.
    driver.tick();

    let taken = driver.response();
    assert!(taken.taken);
    let reusable_token = (taken.token + 1) & 7;
    assert_eq!(driver.get("decision_squash"), 1);
    assert_eq!(driver.get("table_response_cancel"), 1);

    // Accept A without presenting a target.  The decision shadow and raw
    // shadow self-consume on this edge and no delayed cancel state remains.
    driver.set_u8("response_accept", 1);
    driver.tick();
    driver.set_u8("response_accept", 0);
    assert_eq!(driver.get("btb_response_valid"), 0);
    assert_eq!(driver.get("tage_response_valid"), 0);
    assert_eq!(driver.get("decision_valid_q"), 0);
    assert_eq!(driver.get("response_valid"), 0);
    assert_eq!(driver.get("allocation_token_q"), u64::from(reusable_token));

    let after_gap = driver.request(target, target + 4);
    assert_eq!(after_gap.token, reusable_token);
}

#[test]
fn back_to_back_same_index_training_forwards_between_pipeline_packets() {
    let mut driver = Driver::new();
    let pc = 0x5a02;
    let fallthrough = pc + 2;
    let target = 0x5900;

    // The cold update leaves B weak-not-taken. Take two further snapshots of
    // that same word before either token retires.
    driver.learn_cold_control(pc, fallthrough, KIND_CONDITIONAL, target);
    let older = driver.request_conditional(pc, fallthrough);
    assert!(!older.taken);
    driver.accept();
    let younger = driver.request_conditional(pc, fallthrough);
    assert!(!younger.taken);
    driver.accept();

    driver.retire(
        older,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        false,
    );
    driver.retire(
        younger,
        pc,
        KIND_CONDITIONAL,
        true,
        target,
        fallthrough,
        false,
    );
    // S1 packet forwarding must compose both increments (1 -> 2 -> 3).
    let third = driver.request_conditional(pc, fallthrough);
    assert!(third.taken);
    driver.accept();
    driver.retire(
        third,
        pc,
        KIND_CONDITIONAL,
        false,
        target,
        fallthrough,
        true,
    );
    // Correct composition leaves 3 -> 2 (taken); a lost increment gives
    // 2 -> 1 (not taken).
    let final_response = driver.request_conditional(pc, fallthrough);
    assert!(final_response.taken);
}

#[test]
fn interleaved_a_b_a_training_keeps_every_live_token_snapshot_coherent() {
    let mut driver = Driver::new();
    let a_pc = 0x6202;
    let a_fallthrough = a_pc + 2;
    let a_target = 0x6100;
    let b_pc = 0x6c42;
    let b_fallthrough = b_pc + 2;
    let b_target = 0x6b00;

    driver.learn_cold_control(a_pc, a_fallthrough, KIND_CONDITIONAL, a_target);
    driver.learn_cold_control(b_pc, b_fallthrough, KIND_CONDITIONAL, b_target);

    let older_a = driver.request_conditional(a_pc, a_fallthrough);
    assert!(!older_a.taken);
    driver.accept();
    let middle_b = driver.request_conditional(b_pc, b_fallthrough);
    assert!(!middle_b.taken);
    driver.accept();
    let younger_a = driver.request_conditional(a_pc, a_fallthrough);
    assert!(!younger_a.taken);
    driver.accept();

    // No recovery is injected in this predictor-local ordering test: all three
    // tokens remain live and retire in order while B evicts a one-entry cache.
    driver.retire(
        older_a,
        a_pc,
        KIND_CONDITIONAL,
        true,
        a_target,
        a_fallthrough,
        false,
    );
    driver.retire(
        middle_b,
        b_pc,
        KIND_CONDITIONAL,
        true,
        b_target,
        b_fallthrough,
        false,
    );
    driver.retire(
        younger_a,
        a_pc,
        KIND_CONDITIONAL,
        true,
        a_target,
        a_fallthrough,
        false,
    );

    let a_probe = driver.request_conditional(a_pc, a_fallthrough);
    assert!(a_probe.taken);
    driver.accept();
    driver.retire(
        a_probe,
        a_pc,
        KIND_CONDITIONAL,
        false,
        a_target,
        a_fallthrough,
        true,
    );
    assert!(driver.request_conditional(a_pc, a_fallthrough).taken);
}

#[test]
fn s2_stalls_same_edge_aging_write_and_restarts_the_stale_bank() {
    let mut driver = Driver::new();

    // Put bank zero's aging RMW in its write phase and inject a valid S2
    // allocation for the same row.  Current training stalls every aging bank,
    // so S2 consumes its registered classified snapshot without returning
    // through the final Port-B arbitration mux.
    driver.set_tage_u8("age_active_q", 1);
    driver.set_tage_u8("age_pending0_q", 1);
    driver.set_tage_u16("age_address_q", 7);
    driver.set_tage_u16("age_read0_q", 0x803e);
    driver.set_tage_u8("classified_valid", 1);
    driver.set_tage_u16("classified_index0", 7);
    driver.set_tage_u16("classified_tag0", 0);
    driver.set_tage_u16("classified_entry0", 0x803e);
    driver.set_tage_u8("classified_provider_oh", 0b00001);
    driver.set_tage_u8("classified_alt_oh", 0b00001);
    driver.set_tage_u8("classified_lm_pred", 0);
    driver.set_tage_u8("classified_alt_pred", 0);
    driver.set_tage_u8("classified_final_pred", 0);
    driver.set_tage_u8("classified_hit_weak", 0);
    driver.set_tage_u8("classified_taken", 1);

    assert_eq!(driver.get_tage("train_valid"), 1);
    assert_eq!(driver.get_tage("train_tag_write0"), 1);
    assert_eq!(driver.get_tage("age_write_enable0"), 0);
    assert_eq!(driver.get_tage("tag_portb_write0"), 0);
    assert_eq!(driver.get_tage("tag_portb_data0"), 0);
    assert_eq!(driver.get_tage("train_entry0"), 0x803c);

    driver.tick();
    assert_eq!(driver.get_tage("train_packet_tag_write0_q"), 1);
    assert_eq!(driver.get_tage("age_write_enable0"), 0);
    assert_eq!(driver.get_tage("tag_portb_write0"), 1);
    assert_eq!(driver.get_tage("tag_portb_data0"), 0x803c);

    // The registered training write owns Port B on this edge.  Because it
    // targets the saved aging row, the stale RMW word is discarded; the bank
    // resumes by reading the newly trained row rather than overwriting it.
    driver.tick();
    assert_eq!(driver.get_tage("age_pending0_q"), 0);
    assert_eq!(driver.get_tage("age_done0_q"), 0);
    assert_eq!(driver.get_tage("age_read_enable0"), 1);
}
