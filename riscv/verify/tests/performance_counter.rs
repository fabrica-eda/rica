use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module PerformanceCounterTestTop (
    clk         : input  clock    ,
    rst_n       : input  reset    ,
    write_enable: input  logic    ,
    write_value : input  logic<64>,
    increment   : input  logic    ,
    freeze      : input  logic    ,
    value       : output logic<64>,
) {
    inst counter: PerformanceCounter64 (
        clk         : clk         ,
        rst_n       : rst_n       ,
        write_enable: write_enable,
        write_value : write_value ,
        increment   : increment   ,
        freeze      : freeze      ,
        value       : value       ,
    );
}
"#;

fn value(simulator: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    simulator
        .get(simulator.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

#[test]
fn split_counter_is_exact_across_freeze_write_and_wrap() {
    let riscv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let source = [
        fs::read_to_string(riscv.join("src/PerformanceCounter64.veryl")).unwrap(),
        TEST_TOP.to_owned(),
    ]
    .join("\n");
    let mut simulator = SimulatorBuilder::new(&source, "PerformanceCounterTestTop")
        .build_native()
        .unwrap();
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let write_enable = simulator.signal("write_enable");
    let write_value = simulator.signal("write_value");
    let increment = simulator.signal("increment");
    let freeze = simulator.signal("freeze");

    simulator
        .modify(|io| {
            io.set(reset, 0_u8);
            io.set(write_enable, 0_u8);
            io.set(write_value, 0_u64);
            io.set(increment, 0_u8);
            io.set(freeze, 0_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(reset, 1_u8)).unwrap();

    simulator
        .modify(|io| {
            io.set(write_enable, 1_u8);
            io.set(write_value, 0x0000_0001_ffff_fffe_u64);
            io.set(increment, 1_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0x0000_0001_ffff_fffe);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0x0000_0001_ffff_ffff);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0x0000_0002_0000_0000);

    // Freeze must preserve the exact value immediately after a low-half wrap.
    simulator.modify(|io| io.set(freeze, 1_u8)).unwrap();
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0x0000_0002_0000_0000);
    simulator.modify(|io| io.set(freeze, 0_u8)).unwrap();
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0x0000_0002_0000_0001);

    simulator
        .modify(|io| {
            io.set(write_enable, 1_u8);
            io.set(write_value, u64::MAX);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "value"), u64::MAX);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 0);

    // A write has priority over the event at the 64-bit wrap boundary.
    simulator
        .modify(|io| {
            io.set(write_enable, 1_u8);
            io.set(write_value, 7_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(write_enable, 0_u8);
            io.set(increment, 0_u8);
        })
        .unwrap();
    assert_eq!(value(&mut simulator, "value"), 7);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "value"), 7);
}
