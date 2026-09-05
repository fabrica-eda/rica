//! JTAGG boundary contract observed on ECP5: falling-edge JTDI trails the raw
//! shift control by one TCK. Exercise RTL and mapped primitives with real frame
//! boundaries, all bit positions, unrelated chains, and CPU-only reset.
use celox::{NativeBackend, Simulator, SimulatorBuilder};
use struo_celox::ecp5_simulator;
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{Ecp5Cell, map_to_ecp5};

const SOURCE: &str = include_str!("../../src/JtagDebugTransport.veryl");

enum Model {
    Rtl(Simulator<NativeBackend>),
    Mapped(celox03::Simulator<celox03::NativeBackend>),
}

impl Model {
    fn set(&mut self, name: &str, value: u8) {
        match self {
            Self::Rtl(s) => {
                let signal = s.signal(name);
                s.modify(|io| io.set(signal, value)).unwrap();
            }
            Self::Mapped(s) => {
                let signal = s.signal(name);
                s.modify(|io| io.set(signal, value)).unwrap();
            }
        }
    }
    fn tick(&mut self, name: &str) {
        match self {
            Self::Rtl(s) => {
                let event = s.event(name);
                s.tick(event).unwrap();
            }
            Self::Mapped(s) => {
                let event = s.event(name);
                s.tick(event).unwrap();
            }
        }
    }
    fn get(&mut self, name: &str) -> u64 {
        let words = match self {
            Self::Rtl(s) => {
                let signal = s.signal(name);
                s.get(signal).to_u64_digits()
            }
            Self::Mapped(s) => {
                let signal = s.signal(name);
                s.get(signal).to_u64_digits()
            }
        };
        words.first().copied().unwrap_or(0)
    }
}

struct Tap {
    model: Model,
    received: Vec<u128>,
}

impl Tap {
    fn new(model: Model) -> Self {
        let mut tap = Self {
            model,
            received: Vec::new(),
        };
        for name in [
            "rst_n",
            "jtag_rst_n",
            "jtag_tdi",
            "jtag_shift",
            "jtag_ce1",
            "jtag_ce2",
            "jtag_update",
            "response_bit",
            "status_bit",
        ] {
            tap.model.set(name, 0);
        }
        tap.model.tick("jtag_tck");
        tap.cpu(4);
        tap.model.set("rst_n", 1);
        tap.model.set("jtag_rst_n", 1);
        tap.cpu(4);
        tap
    }
    fn cpu(&mut self, count: usize) {
        for _ in 0..count {
            self.model.tick("clk");
            if self.model.get("command_valid") != 0 {
                let low = self.model.get("command_frame_low");
                let high = self.model.get("command_frame_high");
                self.received.push(u128::from(high) << 64 | u128::from(low));
            }
        }
    }
    fn falling(&mut self, tdi: u8, chain: u8, shift: u8, update: u8) {
        self.model.set("jtag_tdi", tdi);
        self.model.set("jtag_ce1", u8::from(chain == 1));
        self.model.set("jtag_ce2", u8::from(chain == 2));
        self.model.set("jtag_shift", shift);
        self.model.set("jtag_update", update);
        self.model.tick("jtag_tck");
        self.cpu(5);
    }
    fn scan(&mut self, frame: u128, chain: u8, update_width: usize) {
        self.falling(0, chain, 0, 0);
        let mut delayed = 0;
        for bit in 0..128 {
            self.falling(delayed, chain, 1, 0);
            delayed = ((frame >> bit) & 1) as u8;
        }
        // CE need not survive UPDATE. The delayed shift enable delivers the
        // final bit here, concurrently with sampling the raw UPDATE level.
        self.falling(delayed, 0, 0, 1);
        for _ in 1..update_width {
            self.falling(0, 0, 0, 1);
        }
        self.falling(0, 0, 0, 0);
        self.falling(0, 0, 0, 0);
    }
}

fn exercise(model: Model) {
    let mut tap = Tap::new(model);
    let mut expected = vec![0, u128::MAX, 0xaaaa_5555_ffff_0000_0123_4567_89ab_cdef];
    expected.extend((0..128).map(|bit| 1_u128 << bit));
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..64 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        expected.push(u128::from(state) << 64 | u128::from(state.rotate_left(23)));
    }
    for (i, &frame) in expected.iter().enumerate() {
        tap.scan(frame, 1, if i % 3 == 0 { 4 } else { 1 });
        assert_eq!(tap.received, expected[..=i], "ER1 frame {i}");
        tap.scan(!frame, 2, 1);
        tap.scan(frame.rotate_left(1), 0, 2);
        assert_eq!(tap.received, expected[..=i], "stale replay after frame {i}");
    }
    tap.model.set("rst_n", 0);
    tap.cpu(4);
    tap.model.set("rst_n", 1);
    tap.cpu(12);
    assert_eq!(
        tap.received, expected,
        "CPU relock replayed the retained mailbox"
    );
    tap.scan(0xfedc_ba98_7654_3210_ffff_ffff_0000_0001, 1, 1);
    expected.push(0xfedc_ba98_7654_3210_ffff_ffff_0000_0001);
    assert_eq!(tap.received, expected);
}

#[test]
fn falling_jtag_preserves_every_frame_bit_and_update_boundary() {
    exercise(Model::Rtl(
        SimulatorBuilder::new(SOURCE, "JtagDebugTransport")
            .build_native()
            .unwrap(),
    ));
}

#[test]
fn mapped_falling_jtag_preserves_every_frame_bit_and_update_boundary() {
    let design = analyze_and_lower(SOURCE, "rica_riscv", "JtagDebugTransport").unwrap();
    let netlist = synthesize(&design).unwrap();
    let mapped = map_to_ecp5(&netlist.netlist).unwrap();
    // Event-driven functional ticks alone cannot detect a lost edge polarity.
    // Independently require the 128 shift + 128 mailbox + 5 control FFs to
    // retain their falling-edge configuration through technology mapping.
    let falling = mapped.cells().iter().filter(|cell| {
        matches!(cell, Ecp5Cell::FlipFlop { edge, .. } if format!("{edge:?}") == "Falling")
    }).count();
    assert_eq!(falling, 261);
    exercise(Model::Mapped(
        ecp5_simulator(&mapped).unwrap().build_native().unwrap(),
    ));
}
