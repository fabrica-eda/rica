use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};

struct Driver(Simulator<NativeBackend>);

impl Driver {
    fn new() -> Self {
        static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
        let image = IMAGE.get_or_init(|| {
            let source = fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/StaticBranchPredictor.veryl"),
            )
            .unwrap();
            SimulatorBuilder::new(&source, "StaticBranchPredictor")
                .compile_native()
                .unwrap()
                .into_program_image()
        });
        let simulator = SimulatorBuilder::new("", "StaticBranchPredictor")
            .build_native_from_image(image.clone())
            .unwrap();
        let mut driver = Self(simulator);
        for name in [
            "rst_n",
            "request_valid",
            "request_fallthrough",
            "response_accept",
            "retire_valid",
            "retire_kind",
            "retire_taken",
            "recover_valid",
            "discard_all",
        ] {
            driver.set(name, 0);
        }
        driver.tick();
        driver.set("rst_n", 1);
        driver
    }

    fn set(&mut self, name: &str, value: u32) {
        let signal = self.0.signal(name);
        self.0
            .modify(|io| {
                if name == "request_fallthrough" {
                    io.set(signal, value);
                } else {
                    io.set(signal, u8::try_from(value).unwrap());
                }
            })
            .unwrap();
    }

    fn value(&mut self, name: &str) -> u64 {
        let signal = self.0.signal(name);
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
}

#[test]
fn backpressure_preserves_compressed_fallthrough_and_allows_replacement() {
    let mut driver = Driver::new();
    assert_eq!(driver.value("ready"), 1);
    driver.set("request_valid", 1);
    driver.set("request_fallthrough", 0x1002);
    assert_eq!(driver.value("request_accept"), 1);
    driver.tick();
    driver.set("request_fallthrough", 0x2004);
    for _ in 0..4 {
        assert_eq!(driver.value("request_accept"), 0);
        assert_eq!(driver.value("response_valid"), 1);
        assert_eq!(driver.value("response_next_pc"), 0x1002);
        assert_eq!(driver.value("response_taken"), 0);
        driver.tick();
    }
    driver.set("response_accept", 1);
    assert_eq!(driver.value("request_accept"), 1);
    driver.tick();
    assert_eq!(driver.value("response_next_pc"), 0x2004);
    assert_eq!(driver.value("response_valid"), 1);
    driver.set("request_valid", 0);
    driver.tick();
    assert_eq!(driver.value("response_valid"), 0);
}

#[test]
fn redirect_and_discard_cancel_a_held_response_and_reject_same_edge_request() {
    for flush in ["recover_valid", "discard_all"] {
        let mut driver = Driver::new();
        driver.set("request_valid", 1);
        driver.set("request_fallthrough", 0x1004);
        driver.tick();
        driver.set("response_accept", 1);
        driver.set("request_fallthrough", 0x2004);
        driver.set(flush, 1);
        assert_eq!(driver.value("request_accept"), 0);
        assert_eq!(driver.value("response_valid"), 0);
        driver.tick();
        driver.set(flush, 0);
        assert_eq!(driver.value("response_valid"), 0);
        driver.set("request_fallthrough", 0x3002);
        driver.tick();
        assert_eq!(driver.value("response_valid"), 1);
        assert_eq!(driver.value("response_next_pc"), 0x3002);
    }
}

#[test]
fn performance_events_count_only_retired_control_flow() {
    let mut driver = Driver::new();
    driver.set("retire_kind", 1);
    driver.set("retire_taken", 1);
    assert_eq!(driver.value("conditional_resolved_event"), 0);
    assert_eq!(driver.value("direction_wrong_event"), 0);
    assert_eq!(driver.value("btb_miss_event"), 0);
    driver.set("retire_valid", 1);
    assert_eq!(driver.value("conditional_resolved_event"), 1);
    assert_eq!(driver.value("direction_wrong_event"), 1);
    assert_eq!(driver.value("btb_miss_event"), 1);
    driver.set("retire_taken", 0);
    assert_eq!(driver.value("direction_wrong_event"), 0);
    driver.set("retire_kind", 4);
    assert_eq!(driver.value("conditional_resolved_event"), 0);
    assert_eq!(driver.value("btb_miss_event"), 1);
    assert_eq!(driver.value("ras_fallback_event"), 0);
}
