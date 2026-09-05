use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};

const SOURCE: &str = include_str!("../../src/CompactBranchPredictor.veryl");

struct Driver(Simulator<NativeBackend>);

#[derive(Debug)]
struct Prediction {
    token: u32,
    kind: u64,
    taken: u64,
    target: u64,
    hit: u64,
}

impl Driver {
    fn new(stack: bool, bits: u64) -> Self {
        static BTB64: OnceLock<NativeProgramImage> = OnceLock::new();
        static BTB32: OnceLock<NativeProgramImage> = OnceLock::new();
        static RAS: OnceLock<NativeProgramImage> = OnceLock::new();
        let (top, image) = if stack {
            ("CompactReturnStack", &RAS)
        } else if bits == 5 {
            ("CompactBranchPredictor", &BTB32)
        } else {
            ("CompactBranchPredictor", &BTB64)
        };
        let image = image.get_or_init(|| {
            let mut builder = SimulatorBuilder::new(SOURCE, top);
            if !stack {
                builder = builder.param("INDEX_BITS", bits);
            }
            builder.compile_native().unwrap().into_program_image()
        });
        let simulator = SimulatorBuilder::new("", top)
            .build_native_from_image(image.clone())
            .unwrap();
        let mut driver = Self(simulator);
        let inputs = if stack {
            &[
                "predict_valid",
                "predict_kind",
                "predict_fallthrough",
                "retire_valid",
                "retire_kind",
                "retire_fallthrough",
                "restore",
            ][..]
        } else {
            &[
                "request_valid",
                "request_pc",
                "request_fallthrough",
                "response_accept",
                "retire_valid",
                "retire_token",
                "retire_pc",
                "retire_kind",
                "retire_taken",
                "retire_target",
                "retire_fallthrough",
                "recover_valid",
                "discard_all",
            ][..]
        };
        driver.set("rst_n", 0);
        for input in inputs {
            driver.set(input, 0);
        }
        driver.tick();
        driver.set("rst_n", 1);
        if !stack {
            for _ in 0..(1 << bits) {
                assert_eq!(driver.get("request_ready"), 0);
                driver.tick();
            }
            assert_eq!(driver.get("ready"), 1);
        }
        driver
    }

    fn set(&mut self, name: &str, value: u32) {
        let signal = self.0.signal(name);
        self.0
            .modify(|io| {
                if name.ends_with("pc") || name.ends_with("target") || name.ends_with("fallthrough")
                {
                    io.set(signal, value);
                } else {
                    io.set(signal, u8::try_from(value).unwrap());
                }
            })
            .unwrap();
    }

    fn get(&mut self, name: &str) -> u64 {
        let signal = if let Some((child, signal)) = name.split_once('.') {
            self.0.child_signal(&[(child, 0)], signal)
        } else {
            self.0.signal(name)
        };
        self.0
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let clock = self.0.event("clk");
        self.0.tick(clock).unwrap();
    }

    fn offer(&mut self, pc: u32, fallthrough: u32) {
        self.set("request_pc", pc);
        self.set("request_fallthrough", fallthrough);
        self.set("request_valid", 1);
        assert_eq!(self.get("request_accept"), 1);
        self.tick();
        self.set("request_valid", 0);
    }

    fn accept(&mut self) -> Prediction {
        for _ in 0..3 {
            if self.get("response_valid") != 0 {
                break;
            }
            self.tick();
        }
        assert_eq!(self.get("response_valid"), 1);
        let result = Prediction {
            token: self.get("response_token") as u32,
            kind: self.get("response_kind"),
            taken: self.get("response_taken"),
            target: self.get("response_next_pc"),
            hit: self.get("response_btb_hit"),
        };
        self.set("response_accept", 1);
        self.tick();
        self.set("response_accept", 0);
        result
    }

    fn predict(&mut self, pc: u32, fallthrough: u32) -> Prediction {
        self.offer(pc, fallthrough);
        self.accept()
    }

    fn retiring(
        &mut self,
        token: u32,
        pc: u32,
        kind: u32,
        taken: u32,
        target: u32,
        fallthrough: u32,
    ) {
        for (name, value) in [
            ("retire_valid", 1),
            ("retire_token", token),
            ("retire_pc", pc),
            ("retire_kind", kind),
            ("retire_taken", taken),
            ("retire_target", target),
            ("retire_fallthrough", fallthrough),
        ] {
            self.set(name, value);
        }
    }

    fn retire(
        &mut self,
        prediction: Prediction,
        pc: u32,
        kind: u32,
        taken: u32,
        target: u32,
        fallthrough: u32,
    ) {
        self.retiring(prediction.token, pc, kind, taken, target, fallthrough);
        self.tick();
        self.set("retire_valid", 0);
    }

    fn stack_action(&mut self, committed: bool, kind: u32, pc: u32) {
        let (valid, kind_name, address) = if committed {
            ("retire_valid", "retire_kind", "retire_fallthrough")
        } else {
            ("predict_valid", "predict_kind", "predict_fallthrough")
        };
        self.set(valid, 1);
        self.set(kind_name, kind);
        self.set(address, pc);
        self.tick();
        self.set(valid, 0);
    }
}

#[test]
fn held_miss_is_stable_and_sequential_requests_can_replace_it_each_clock() {
    for bits in [5, 6] {
        let mut d = Driver::new(false, bits);
        d.offer(0x1000, 0x1002);
        d.offer(0x8000, 0x8004);
        for _ in 0..4 {
            d.set("request_pc", 0x8000);
            d.set("request_fallthrough", 0x8004);
            assert_eq!(d.get("request_ready"), 0);
            assert_eq!(d.get("response_next_pc"), 0x1002);
            assert_eq!(d.get("response_btb_hit"), 0);
            d.tick();
        }
        d.set("response_accept", 1);
        d.set("request_valid", 1);
        assert_eq!(d.get("request_accept"), 1);
        d.tick();
        assert_eq!(d.get("response_next_pc"), 0x8004);
        assert_eq!(d.get("response_valid"), 1);
    }
}

#[test]
fn direction_training_and_same_edge_read_write_forwarding_preserve_tags() {
    for bits in [5, 6] {
        let mut d = Driver::new(false, bits);
        let initial = d.predict(0x1002, 0x1004);
        assert_eq!(initial.hit, 0);
        d.retiring(initial.token, 0x1002, 1, 1, 0x0800, 0x1004);
        d.offer(0x1002, 0x1004);
        d.set("retire_valid", 0);
        let trained = d.accept();
        assert_eq!(
            (trained.kind, trained.taken, trained.target),
            (1, 1, 0x0800)
        );
        d.retire(trained, 0x1002, 1, 0, 0x0800, 0x1004);
        let untaken = d.predict(0x1002, 0x1004);
        assert_eq!(
            (untaken.kind, untaken.taken, untaken.target),
            (1, 0, 0x1004)
        );
        d.retire(untaken, 0x1002, 1, 0, 0x0800, 0x1004);
        let alias_pc = 0x1002 + (2 << bits);
        let alias = d.predict(alias_pc, alias_pc + 4);
        assert_eq!(alias.hit, 0);
        d.retire(alias, alias_pc, 2, 1, 0x4000, alias_pc + 4);
        assert_eq!(d.predict(0x1002, 0x1004).hit, 0);
    }
}

#[test]
fn full_token_window_waits_for_retirement_without_overwriting_predictions() {
    let mut d = Driver::new(false, 6);
    for i in 0..8 {
        assert_eq!(d.predict(0x2000 + i * 4, 0x2004 + i * 4).token, i);
    }
    d.offer(0x4000, 0x4004);
    d.tick(); // Registered decode fills the held response while tokens are full.
    assert_eq!(d.get("response_valid"), 0);
    d.retiring(0, 0x2000, 0, 0, 0, 0x2004);
    assert_eq!(d.get("response_valid"), 0);
    d.tick();
    d.set("retire_valid", 0);
    assert_eq!(d.get("response_valid"), 1);
    let reused = d.accept();
    assert_eq!((reused.token, reused.target), (0, 0x4004));
    d.set("retire_valid", 0);
    assert_eq!(d.get("slot_valid_q"), 0xff);
}

#[test]
fn taken_response_cancels_the_younger_sequential_lookup() {
    let mut d = Driver::new(false, 6);
    let initial = d.predict(0x1000, 0x1004);
    d.retire(initial, 0x1000, 2, 1, 0x8000, 0x1004);
    d.offer(0x1000, 0x1004);
    d.offer(0x1004, 0x1008);
    assert_eq!(d.get("response_taken"), 1);
    assert_eq!(d.get("response_next_pc"), 0x8000);
    let taken = d.accept();
    assert_eq!(taken.target, 0x8000);
    assert_eq!(d.get("lookup_valid_q"), 0);
    assert_eq!(d.get("response_valid"), 0);
    let target = d.predict(0x8000, 0x8004);
    assert_eq!(target.target, 0x8004);
}

#[test]
fn cm_recovery_trains_retiring_owner_and_cancels_younger_tokens_and_lookup() {
    let mut d = Driver::new(false, 6);
    let owner = d.predict(0x1002, 0x1004);
    let _younger = d.predict(0x2000, 0x2004);
    d.offer(0x3000, 0x3004);
    d.retiring(owner.token, 0x1002, 4, 1, 0x2000, 0x1004);
    d.set("recover_valid", 1);
    assert_eq!(d.get("response_valid"), 0);
    d.tick();
    d.set("recover_valid", 0);
    d.set("retire_valid", 0);
    assert_eq!(d.get("slot_valid_q"), 0);
    d.offer(0x4000, 0x4004);
    d.set("discard_all", 1);
    d.tick();
    d.set("discard_all", 0);
    assert_eq!(d.get("response_valid"), 0);
    assert_eq!(d.get("slot_valid_q"), 0);
}

#[test]
fn return_prediction_uses_committed_compressed_link_after_restore() {
    let mut d = Driver::new(false, 6);
    let ret = d.predict(0x200a, 0x200c);
    d.retire(ret, 0x200a, 5, 1, 0xdead, 0x200c);
    let call = d.predict(0x1002, 0x1004);
    d.retiring(call.token, 0x1002, 4, 1, 0x200a, 0x1004);
    d.set("recover_valid", 1);
    d.tick();
    d.set("retire_valid", 0);
    d.set("recover_valid", 0);
    assert_eq!(d.get("slot_valid_q"), 0);
    let ret = d.predict(0x200a, 0x200c);
    assert_eq!((ret.kind, ret.taken, ret.target), (5, 1, 0x1004));
    d.retire(ret, 0x200a, 5, 1, 0x1004, 0x200c);
    // The retiring/recovering call must not have been pushed twice.
    let underflow = d.predict(0x200a, 0x200c);
    assert_eq!(underflow.target, 0x1004);
    assert_eq!(d.get("return_stack.empty"), 1);
}

#[test]
fn ras_overflow_underflow_and_coroutine_replace_top() {
    let mut d = Driver::new(true, 0);
    assert_eq!(d.get("empty"), 1);
    d.stack_action(false, 5, 0);
    assert_eq!(d.get("empty"), 1);
    for i in 0..10 {
        d.stack_action(false, 4, 0x1002 + i * 4);
    }
    d.stack_action(false, 6, 0x8002);
    assert_eq!(d.get("top"), 0x8002);
    d.stack_action(false, 5, 0);
    for i in (2..9).rev() {
        assert_eq!(d.get("top"), 0x1002 + i * 4);
        d.stack_action(false, 5, 0);
    }
    assert_eq!(d.get("empty"), 1);
}

#[test]
fn ras_restore_recovers_overwritten_entries_and_includes_current_retirement() {
    let mut d = Driver::new(true, 0);
    d.stack_action(true, 4, 0x1002);
    d.stack_action(true, 4, 0x2004);
    d.set("restore", 1);
    d.tick();
    d.set("restore", 0);
    for i in 0..12 {
        d.stack_action(false, 4, 0x9000 + i * 4);
    }
    d.set("retire_valid", 1);
    d.set("retire_kind", 5);
    d.set("restore", 1);
    d.tick();
    d.set("retire_valid", 0);
    d.set("restore", 0);
    assert_eq!(d.get("top"), 0x1002);
    d.stack_action(false, 5, 0);
    assert_eq!(d.get("empty"), 1);
}

#[test]
fn ras_matches_architectural_stack_through_back_to_back_actions() {
    fn action(stack: &mut Vec<u32>, valid: bool, kind: u32, pc: u32) {
        if !valid {
            return;
        }
        if kind == 5 || kind == 6 {
            stack.pop();
        }
        if kind == 4 || kind == 6 {
            if stack.len() == 8 {
                stack.remove(0);
            }
            stack.push(pc);
        }
    }
    let mut d = Driver::new(true, 0);
    let mut committed = Vec::new();
    let mut speculative = Vec::new();
    let mut random = 0xf54d_230a_u32;
    for cycle in 0..4096_u32 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let retire_valid = random & 1 != 0;
        let predict_valid = random & 2 != 0;
        let restore = random & 0x1c == 0;
        let retire_kind = 3 + ((random >> 5) & 3);
        let predict_kind = 3 + ((random >> 7) & 3);
        let retired_pc = 0x1002 + cycle * 4;
        let predicted_pc = 0x8002 + cycle * 4;
        for (name, value) in [
            ("retire_valid", u32::from(retire_valid)),
            ("predict_valid", u32::from(predict_valid)),
            ("restore", u32::from(restore)),
            ("retire_kind", retire_kind),
            ("predict_kind", predict_kind),
            ("retire_fallthrough", retired_pc),
            ("predict_fallthrough", predicted_pc),
        ] {
            d.set(name, value);
        }
        action(&mut committed, retire_valid, retire_kind, retired_pc);
        if restore {
            speculative.clone_from(&committed);
        } else {
            action(&mut speculative, predict_valid, predict_kind, predicted_pc);
        }
        d.tick();
        assert_eq!(
            d.get("empty"),
            u64::from(speculative.is_empty()),
            "cycle {cycle}"
        );
        if let Some(&top) = speculative.last() {
            assert_eq!(d.get("top"), u64::from(top), "cycle {cycle}");
        }
    }
}
