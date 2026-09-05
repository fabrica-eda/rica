use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};

fn project_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn native_image() -> &'static NativeProgramImage {
    static IMAGE: OnceLock<NativeProgramImage> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let source =
            fs::read_to_string(project_path().join("src/InstructionBuffer.veryl")).unwrap();
        SimulatorBuilder::new(&source, "InstructionBuffer")
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

struct Driver {
    simulator: Simulator<NativeBackend>,
}

impl Driver {
    fn new() -> Self {
        let mut simulator = SimulatorBuilder::new("", "InstructionBuffer")
            .build_native_from_image(native_image().clone())
            .unwrap();
        let clock = simulator.event("clk");
        let rst_n = simulator.signal("rst_n");
        let invalidate = simulator.signal("invalidate");
        let pc = simulator.signal("pc");
        let response_address = simulator.signal("memory_response_address");
        let response_line = simulator.signal("memory_response_line");
        let response_valid = simulator.signal("memory_response_valid");
        simulator
            .modify(|io| {
                io.set(rst_n, 0_u8);
                io.set(invalidate, 0_u8);
                io.set(pc, 0_u32);
                io.set(response_address, 0_u32);
                io.set(response_line, 0_u64);
                io.set(response_valid, 0_u8);
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        simulator.modify(|io| io.set(rst_n, 1_u8)).unwrap();
        Self { simulator }
    }

    fn fill(&mut self, address: u32, line: u64) {
        let clock = self.simulator.event("clk");
        let response_address = self.simulator.signal("memory_response_address");
        let response_line = self.simulator.signal("memory_response_line");
        let response_valid = self.simulator.signal("memory_response_valid");
        self.simulator
            .modify(|io| {
                io.set(response_address, address);
                io.set(response_line, line);
                io.set(response_valid, 1_u8);
            })
            .unwrap();
        self.simulator.tick(clock).unwrap();
        self.simulator
            .modify(|io| io.set(response_valid, 0_u8))
            .unwrap();
        self.simulator.tick(clock).unwrap();
    }

    fn fetch(&mut self, pc_value: u32) -> (u32, bool, bool) {
        let pc = self.simulator.signal("pc");
        self.simulator.modify(|io| io.set(pc, pc_value)).unwrap();
        (
            self.value("instruction") as u32,
            self.value("instruction_compressed") != 0,
            self.value("instruction_valid") != 0,
        )
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
}

#[test]
fn slot_zero_to_slot_one_cross_line_instruction_is_valid() {
    let mut driver = Driver::new();
    driver.fill(0x0000_1000, 0x00b3_0000_0000_0013);
    driver.fill(0x0000_1008, 0x0000_0000_0000_a55a);
    assert_eq!(driver.fetch(0x0000_1006), (0xa55a_00b3, false, true));
}

#[test]
fn slot_one_to_slot_zero_cross_line_instruction_is_valid() {
    let mut driver = Driver::new();
    driver.fill(0x0000_2008, 0x00f3_0000_0000_0013);
    driver.fill(0x0000_2010, 0x0000_0000_0000_5aa5);
    assert_eq!(driver.fetch(0x0000_200e), (0x5aa5_00f3, false, true));
}

#[test]
fn replacing_the_other_bank_with_a_non_successor_invalidates_immediately() {
    let mut driver = Driver::new();
    driver.fill(0x0000_3008, 0x00d3_0000_0000_0013);
    driver.fill(0x0000_3010, 0x0000_0000_0000_cafe);
    assert!(driver.fetch(0x0000_300e).2);

    driver.fill(0x0000_4010, 0x0000_0000_0000_beef);
    assert!(!driver.fetch(0x0000_300e).2);
}

#[test]
fn compressed_final_parcel_does_not_need_a_successor() {
    let mut driver = Driver::new();
    driver.fill(0x0000_5008, 0x0001_0000_0000_0013);
    assert_eq!(driver.fetch(0x0000_500e), (0x0000_0001, true, true));
}

#[test]
fn slot_one_successor_relation_wraps_at_the_32_bit_address_boundary() {
    let mut driver = Driver::new();
    driver.fill(0xffff_fff8, 0x00e3_0000_0000_0013);
    driver.fill(0x0000_0000, 0x0000_0000_0000_1234);
    assert_eq!(driver.fetch(0xffff_fffe), (0x1234_00e3, false, true));
}

#[test]
fn invalidation_drains_old_memory_responses_before_refilling() {
    let mut driver = Driver::new();
    driver.fill(0x6000, 0x0010_0513_0010_0513);
    assert!(driver.fetch(0x6000).2);
    let invalidate = driver.simulator.signal("invalidate");
    let response_valid = driver.simulator.signal("memory_response_valid");
    let clock = driver.simulator.event("clk");
    driver
        .simulator
        .modify(|io| {
            io.set(invalidate, 1_u8);
            io.set(response_valid, 1_u8);
        })
        .unwrap();
    assert!(!driver.fetch(0x6000).2);
    driver.simulator.tick(clock).unwrap();
    driver
        .simulator
        .modify(|io| io.set(invalidate, 0_u8))
        .unwrap();
    for _ in 0..7 {
        assert!(!driver.fetch(0x6000).2);
        driver.simulator.tick(clock).unwrap();
    }
    assert!(!driver.fetch(0x6000).2);
    driver.fill(0x6000, 0x0020_0513_0020_0513);
    assert_eq!(driver.fetch(0x6000), (0x0020_0513, false, true));
}
