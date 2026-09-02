use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, OptLevel, Simulator, SimulatorBuilder};

const KIND_NONE: u8 = 0;
const KIND_DIRECT: u8 = 2;
const SCRUB_CYCLES: usize = 2048;
const BTB_SCRUB_CYCLES: usize = 128;

fn scrub_predictor_source() -> String {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let mut source = [
        "BranchTargetBuffer.veryl",
        "TageTaggedTableMemory.veryl",
        "TageDirectionPredictor.veryl",
        "BranchPredictor.veryl",
    ]
    .into_iter()
    .map(|file| fs::read_to_string(project.join("src").join(file)).unwrap())
    .collect::<Vec<_>>()
    .join("\n");

    // Compile the real predictor with reset scrubbing explicitly enabled. The
    // exact module marker prevents accidentally changing either child module's
    // own default instead of the parameter propagated by BranchPredictor.
    const DEFAULT: &str = "module BranchPredictor #(\n    param SCRUB_ON_RESET: bit = 0,";
    const SCRUBBED: &str = "module BranchPredictor #(\n    param SCRUB_ON_RESET: bit = 1,";
    assert_eq!(source.matches(DEFAULT).count(), 1);
    source = source.replacen(DEFAULT, SCRUBBED, 1);
    source
}

fn native_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        SimulatorBuilder::new(&scrub_predictor_source(), "BranchPredictor")
            .opt_level(OptLevel::O0)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

fn new_simulator() -> Simulator<NativeBackend> {
    SimulatorBuilder::new("", "BranchPredictor")
        .build_native_from_image(native_image().clone())
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
struct Response {
    token: u8,
    kind: u8,
    next_pc: u32,
    btb_hit: bool,
}

#[derive(Clone, Copy, Debug)]
struct TageCoordinates {
    base: u16,
    index: [u16; 4],
    tag: [u16; 4],
}

struct Driver {
    simulator: Simulator<NativeBackend>,
}

impl Driver {
    fn new_without_reset() -> Self {
        let mut driver = Self {
            simulator: new_simulator(),
        };
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
        driver.set_u8("rst_n", 1);

        // Celox initializes ordinary state to zero. Before the first reset that
        // is an idle, ready predictor, which lets this test create realistic
        // retained BRAM contents through the RTL's own write paths.
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

    fn get_tage(&mut self, name: &str) -> u64 {
        let signal = self.simulator.child_signal(&[("tage", 0)], name);
        self.simulator
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
    }

    fn request(&mut self, pc: u32, fallthrough: u32) -> Response {
        assert_eq!(self.get("ready"), 1);
        assert_eq!(self.get("response_valid"), 0);
        self.set_u32("request_pc", pc);
        self.set_u32("request_fallthrough", fallthrough);
        self.set_u8("request_is_conditional", 0);
        self.set_u8("request_valid", 1);
        // S0 is only an ingress enqueue.  No table may see the request on
        // this edge, and the public acceptance is the fetch-side handshake.
        assert_eq!(self.get("request_accept"), 1);
        assert_eq!(self.get("request_fire"), 0);
        self.tick();
        self.set_u8("request_valid", 0);
        assert_eq!(self.get("ingress_valid_q"), 1);
        assert_eq!(self.get("request_fire"), 1);
        assert_eq!(self.get("btb_response_valid"), 0);
        assert_eq!(self.get("tage_response_valid"), 0);

        // S1 launches the registered S0 packet into both synchronous tables.
        self.tick();
        assert_eq!(self.get("ingress_valid_q"), 0);
        assert_eq!(self.get("btb_response_valid"), 1);
        assert_eq!(self.get("tage_response_valid"), 1);
        assert_eq!(self.get("response_valid"), 0);

        // S2 captures the paired table response; a cold miss is now visible
        // through the fast-miss response path. A BTB hit takes one additional
        // classifier/output edge, so select the two legal response paths
        // explicitly instead of hiding the distinction behind extra ticks.
        self.tick();
        if self.get("decision_btb_hit_q") != 0 {
            assert_eq!(self.get("decision_valid_q"), 1);
            assert_eq!(self.get("response_valid"), 0);
            assert_eq!(self.get("decision_promote"), 1);
            self.tick();
        } else {
            assert_eq!(self.get("response_valid"), 1);
        }
        assert_eq!(self.get("response_valid"), 1);
        Response {
            token: self.get("response_token") as u8,
            kind: self.get("response_kind") as u8,
            next_pc: self.get("response_next_pc") as u32,
            btb_hit: self.get("response_btb_hit") != 0,
        }
    }

    fn accept(&mut self) {
        assert_eq!(self.get("response_valid"), 1);
        self.set_u8("response_accept", 1);
        self.tick();
        self.set_u8("response_accept", 0);
    }

    fn discard(&mut self) {
        self.set_u8("discard_all", 1);
        self.tick();
        self.set_u8("discard_all", 0);
        assert_eq!(self.get("response_valid"), 0);
    }

    fn learn_direct(&mut self, pc: u32, target: u32) {
        let fallthrough = pc + 4;
        let response = self.request(pc, fallthrough);
        assert!(!response.btb_hit, "test PC unexpectedly hit before poison");
        assert_eq!(response.kind, KIND_NONE);
        self.accept();

        self.set_u8("retire_token", response.token);
        self.set_u32("retire_pc", pc);
        self.set_u8("retire_kind", KIND_DIRECT);
        self.set_u8("retire_taken", 1);
        self.set_u32("retire_target", target);
        self.set_u32("retire_fallthrough", fallthrough);
        self.set_u8("recover_token", response.token);
        self.set_u8("recover_kind", KIND_DIRECT);
        self.set_u8("recover_taken", 1);
        self.set_u32("recover_fallthrough", fallthrough);
        self.set_u8("retire_valid", 1);
        self.set_u8("recover_valid", 1);
        self.tick();
        self.set_u8("retire_valid", 0);
        self.set_u8("recover_valid", 0);
    }

    fn tage_coordinates(&mut self, pc: u32) -> TageCoordinates {
        let _ = self.request(pc, pc + 4);
        let coordinates = TageCoordinates {
            base: self.get_tage("lookup_base_q") as u16,
            index: std::array::from_fn(|bank| {
                self.get_tage(&format!("lookup_index{bank}_q")) as u16
            }),
            tag: std::array::from_fn(|bank| self.get_tage(&format!("lookup_tag{bank}_q")) as u16),
        };
        self.discard();
        coordinates
    }

    fn poison_tage_row(&mut self, coordinates: TageCoordinates) -> [u16; 4] {
        self.set_tage_u8("train_packet_base_write_q", 1);
        self.set_tage_u16("train_packet_base_addr_q", coordinates.base);
        self.set_tage_u8("train_packet_base_data_q", 3);

        let words = std::array::from_fn(|bank| {
            0x8000 | (coordinates.tag[bank] << 6) | (0b011 << 3) | (0b10 << 1)
        });
        for (bank, word) in words.iter().copied().enumerate() {
            self.set_tage_u8(&format!("train_packet_tag_write{bank}_q"), 1);
            self.set_tage_u16(
                &format!("train_packet_tag_addr{bank}_q"),
                coordinates.index[bank],
            );
            self.set_tage_u16(&format!("train_packet_tag_data{bank}_q"), word);
        }
        self.tick();

        self.set_tage_u8("train_packet_base_write_q", 0);
        for bank in 0..4 {
            self.set_tage_u8(&format!("train_packet_tag_write{bank}_q"), 0);
        }
        words
    }

    fn assert_tage_words(&mut self, pc: u32, coordinates: TageCoordinates, words: [u16; 4]) {
        let _ = self.request(pc, pc + 4);
        assert_eq!(self.get_tage("lookup_base_q"), u64::from(coordinates.base));
        assert_eq!(self.get_tage("response_base_entry"), 3);
        for bank in 0..4 {
            assert_eq!(
                self.get_tage(&format!("response_entry{bank}")),
                u64::from(words[bank]),
            );
        }
        assert_eq!(self.get_tage("response_hit"), 1);
        self.discard();
    }

    fn assert_tage_scrubbed(&mut self, pc: u32, coordinates: TageCoordinates) {
        let _ = self.request(pc, pc + 4);
        assert_eq!(self.get_tage("lookup_base_q"), u64::from(coordinates.base));
        assert_eq!(self.get_tage("response_base_entry"), 1);
        for bank in 0..4 {
            assert_eq!(self.get_tage(&format!("response_entry{bank}")), 0);
        }
        assert_eq!(self.get_tage("response_hit"), 0);
        self.discard();
    }
}

#[test]
fn reset_scrub_blocks_requests_for_the_full_sweep_and_clears_poisoned_tables() {
    let mut driver = Driver::new_without_reset();

    // Poison both physical BTB ways at the first and last sets through normal
    // accepted prediction/retirement traffic.
    let btb_entries = [
        (0x4000, 0x3000),
        (0x4100, 0x3100),
        (0x40fe, 0x3200),
        (0x41fe, 0x3300),
    ];
    for (pc, target) in btb_entries {
        driver.learn_direct(pc, target);
    }
    for (pc, target) in btb_entries {
        let response = driver.request(pc, pc + 4);
        assert!(response.btb_hit);
        assert_eq!(response.kind, KIND_DIRECT);
        assert_eq!(response.next_pc, target);
        driver.discard();
    }

    // Poison the first and last base-table rows and both endpoint rows in every
    // tagged bank. Dynamic hash probes avoid duplicating TAGE hash arithmetic in
    // the test while still writing through the real registered packet ports.
    let first_pc = 0;
    let last_pc = 0x0ffe;
    let first = driver.tage_coordinates(first_pc);
    let last = driver.tage_coordinates(last_pc);
    assert_eq!(first.base, 0);
    assert_eq!(last.base, 2047);
    assert_eq!(first.index, [0; 4]);
    assert_eq!(last.index, [511; 4]);
    let first_words = driver.poison_tage_row(first);
    let last_words = driver.poison_tage_row(last);
    driver.assert_tage_words(first_pc, first, first_words);
    driver.assert_tage_words(last_pc, last, last_words);

    // Reset enables each child's scrub state. No BRAM reset value is assumed:
    // all assertions above proved that the selected physical rows were dirty.
    driver.set_u8("rst_n", 0);
    driver.tick();
    driver.set_u8("rst_n", 1);
    assert_eq!(driver.get("ready"), 0);
    assert_eq!(driver.get("btb_ready"), 0);
    assert_eq!(driver.get("tage_ready"), 0);
    assert_eq!(driver.get("response_valid"), 0);

    // Hold a request throughout initialization. It must not be accepted on any
    // scrub edge. BTB finishes after 128 clocks, but BranchPredictor remains
    // unready until the 2048-entry TAGE base sweep completes exactly.
    driver.set_u32("request_pc", 0x5000);
    driver.set_u32("request_fallthrough", 0x5004);
    driver.set_u8("request_valid", 1);
    for completed in 1..=SCRUB_CYCLES {
        assert_eq!(driver.get("ready"), 0);
        assert_eq!(driver.get("request_accept"), 0);
        assert_eq!(driver.get("request_fire"), 0);
        driver.tick();

        assert_eq!(
            driver.get("btb_ready"),
            u64::from(completed >= BTB_SCRUB_CYCLES),
        );
        assert_eq!(
            driver.get("tage_ready"),
            u64::from(completed == SCRUB_CYCLES),
        );
        assert_eq!(driver.get("ready"), u64::from(completed == SCRUB_CYCLES));
    }
    // ready/request_accept rise combinationally after the final scrub edge, but
    // the held request was never sampled on an edge while acceptance was high.
    assert_eq!(driver.get("request_accept"), 1);
    assert_eq!(driver.get("request_fire"), 0);
    assert_eq!(driver.get("ingress_valid_q"), 0);
    assert_eq!(driver.get("response_valid"), 0);
    driver.set_u8("request_valid", 0);

    for (pc, _) in btb_entries {
        let response = driver.request(pc, pc + 4);
        assert!(!response.btb_hit, "stale BTB entry survived reset scrub");
        assert_eq!(response.kind, KIND_NONE);
        assert_eq!(response.next_pc, pc + 4);
        driver.discard();
    }
    driver.assert_tage_scrubbed(first_pc, first);
    driver.assert_tage_scrubbed(last_pc, last);
}
