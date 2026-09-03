//! Direct child regressions for malformed bank-valid transactions.
//!
//! The frontend normally drives all physical bank valids in lockstep.  These
//! tests deliberately violate that contract at the child boundary: a partial
//! lookup may still exercise a local BRAM read, but it must never publish a
//! response transaction or combine metadata from different lookups.  A later
//! complete lookup must be accepted and must replace the transaction cleanly.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, OptLevel, Simulator, SimulatorBuilder};

fn source(files: &[&str]) -> String {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    files
        .iter()
        .map(|file| fs::read_to_string(project.join("src").join(file)).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tage_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        SimulatorBuilder::new(
            &source(&["TageTaggedTableMemory.veryl", "TageDirectionPredictor.veryl"]),
            "TageDirectionPredictor",
        )
        .opt_level(OptLevel::O0)
        .compile_native()
        .unwrap()
        .into_program_image()
    })
}

fn btb_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        SimulatorBuilder::new(&source(&["BranchTargetBuffer.veryl"]), "BranchTargetBuffer")
            .opt_level(OptLevel::O0)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

struct TageDriver {
    simulator: Simulator<NativeBackend>,
}

impl TageDriver {
    fn new() -> Self {
        let mut driver = Self {
            simulator: SimulatorBuilder::new("", "TageDirectionPredictor")
                .build_native_from_image(tage_image().clone())
                .unwrap(),
        };
        for name in [
            "lookup_valid",
            "lookup_valid_base",
            "lookup_valid_tag0",
            "lookup_valid_tag1",
            "lookup_valid_tag2",
            "lookup_valid_tag3",
            "response_accept",
            "response_cancel",
            "update_valid",
            "release_valid",
            "squash_valid",
            "flush_slots",
        ] {
            driver.set_u8(name, 0);
        }
        for name in ["lookup_token", "snapshot_token", "update_token", "release_token", "squash_token"] {
            driver.set_u8(name, 0);
        }
        driver.set_u32("lookup_pc", 0);
        for name in [
            "lookup_index_0",
            "lookup_index_1",
            "lookup_index_2",
            "lookup_index_3",
            "lookup_tag_0",
            "lookup_tag_1",
            "lookup_tag_2",
            "lookup_tag_3",
        ] {
            driver.set_u8(name, 0);
        }
        driver.set_u8("update_taken", 0);
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

    fn get(&mut self, name: &str) -> u64 {
        self.simulator
            .get(self.simulator.signal(name))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
    }

    fn lookup(&mut self, token: u8, valid: [bool; 6]) {
        self.set_u8("lookup_valid", u8::from(valid[0]));
        self.set_u8("lookup_valid_base", u8::from(valid[1]));
        self.set_u8("lookup_valid_tag0", u8::from(valid[2]));
        self.set_u8("lookup_valid_tag1", u8::from(valid[3]));
        self.set_u8("lookup_valid_tag2", u8::from(valid[4]));
        self.set_u8("lookup_valid_tag3", u8::from(valid[5]));
        self.set_u8("lookup_token", token);
        self.set_u8("snapshot_token", token);
    }

    fn clear_lookup(&mut self) {
        for name in [
            "lookup_valid",
            "lookup_valid_base",
            "lookup_valid_tag0",
            "lookup_valid_tag1",
            "lookup_valid_tag2",
            "lookup_valid_tag3",
        ] {
            self.set_u8(name, 0);
        }
    }
}

struct BtbDriver {
    simulator: Simulator<NativeBackend>,
}

impl BtbDriver {
    fn new() -> Self {
        let mut driver = Self {
            simulator: SimulatorBuilder::new("", "BranchTargetBuffer")
                .build_native_from_image(btb_image().clone())
                .unwrap(),
        };
        for name in [
            "lookup_valid",
            "lookup_valid_way0",
            "lookup_valid_way1",
            "lookup_valid_replacement",
            "response_accept",
            "response_cancel",
            "rollback_valid",
            "update_valid",
            "invalidate_valid",
        ] {
            driver.set_u8(name, 0);
        }
        driver.set_u8("lookup_token", 0);
        driver.set_u8("update_kind", 0);
        driver.set_u8("update_way", 0);
        driver.set_u8("invalidate_way", 0);
        driver.set_u8("rollback_set", 0);
        driver.set_u8("rollback_data", 0);
        for name in [
            "lookup_pc_way0",
            "lookup_pc_way1",
            "lookup_pc_replacement",
            "lookup_pc",
            "update_pc",
            "update_target",
            "invalidate_pc",
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

    fn get(&mut self, name: &str) -> u64 {
        self.simulator
            .get(self.simulator.signal(name))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
    }

    fn lookup(&mut self, token: u8, valid: [bool; 4]) {
        self.set_u8("lookup_valid", u8::from(valid[0]));
        self.set_u8("lookup_valid_way0", u8::from(valid[1]));
        self.set_u8("lookup_valid_way1", u8::from(valid[2]));
        self.set_u8("lookup_valid_replacement", u8::from(valid[3]));
        self.set_u8("lookup_token", token);
        for name in [
            "lookup_pc_way0",
            "lookup_pc_way1",
            "lookup_pc_replacement",
            "lookup_pc",
        ] {
            self.set_u32(name, 0x4000 + u32::from(token) * 2);
        }
    }

    fn clear_lookup(&mut self) {
        for name in [
            "lookup_valid",
            "lookup_valid_way0",
            "lookup_valid_way1",
            "lookup_valid_replacement",
        ] {
            self.set_u8(name, 0);
        }
    }
}

#[test]
fn tage_partial_lookup_never_publishes_and_next_complete_lookup_replaces_it() {
    let mut driver = TageDriver::new();

    // Tag bank 0 is absent.  The other local enables may still read their
    // memories, but no response metadata may be captured for token 2.
    driver.lookup(2, [true, true, false, true, true, true]);
    driver.tick();
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 0);

    // A complete lookup must be accepted immediately and must publish its own
    // token, proving that no stale partial transaction was retained.
    driver.lookup(5, [true, true, true, true, true, true]);
    driver.tick();
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), 5);
}

#[test]
fn tage_training_without_lookup_does_not_disturb_a_held_response() {
    let mut driver = TageDriver::new();

    // First publish a response and leave it unaccepted.  The following edge
    // captures its metadata while no new lookup is present.
    driver.lookup(1, [true, true, true, true, true, true]);
    driver.tick();
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 1);
    let held_token = driver.get("response_token");
    let held_taken = driver.get("response_taken");

    // Present a matching training packet without a lookup.  The collision
    // select is allowed to change in this idle cycle, but it must not alter
    // the response being held.  In particular, this exercises the case where
    // the select is no longer gated by the all-bank lookup accept signal.
    driver.set_u32("lookup_pc", 0x20);
    driver.set_u8("classified_valid", 1);
    driver.set_u8("classified_base_index", 0x10);
    driver.set_u8("classified_base_entry", 0);
    driver.set_u8("classified_provider_oh", 0b00001);
    driver.set_u8("classified_alt_oh", 0b00001);
    driver.set_u8("classified_lm_pred", 0);
    driver.set_u8("classified_alt_pred", 0);
    driver.set_u8("classified_final_pred", 0);
    driver.set_u8("classified_hit_weak", 0);
    driver.set_u8("classified_taken", 1);
    assert_eq!(driver.get("train_valid"), 1);
    assert_eq!(driver.get("train_base_write"), 1);

    driver.tick();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), held_token);
    assert_eq!(driver.get("response_taken"), held_taken);
    assert_eq!(driver.get("response_metadata_captured_q"), 1);
    assert_eq!(
        driver.get("train_packet_base_lookup_collision_q"),
        1,
        "a matching training packet must capture its select even without a lookup",
    );

    // Once the held metadata is captured, both a later bubble and the
    // transient collision-select value are invisible at the response port.
    driver.set_u8("classified_valid", 0);
    driver.tick();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), held_token);
    assert_eq!(driver.get("response_taken"), held_taken);
}

#[test]
fn btb_partial_lookup_cannot_mix_with_held_or_replacement_metadata() {
    let mut driver = BtbDriver::new();

    // Capture a complete response first, then present a malformed partial
    // transaction while it is held.  The held token must remain immutable.
    driver.lookup(1, [true, true, true, true]);
    driver.tick();
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), 1);

    driver.lookup(2, [true, true, false, true]);
    driver.tick();
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), 1);

    // Accepting the held response and presenting a complete replacement on
    // the same edge is legal; its metadata must be token 6, never partial 2.
    driver.set_u8("response_accept", 1);
    driver.lookup(6, [true, true, true, true]);
    driver.tick();
    driver.set_u8("response_accept", 0);
    driver.clear_lookup();
    assert_eq!(driver.get("response_valid"), 1);
    assert_eq!(driver.get("response_token"), 6);
}
