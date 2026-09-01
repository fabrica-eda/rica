use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module CsrFileTestTop (
    clk              : input  clock    ,
    rst_n            : input  reset    ,
    index            : input  logic<4> ,
    write_index      : input  logic<4> ,
    write_enable     : input  logic    ,
    write_op         : input  logic<2> ,
    write_source     : input  logic<64>,
    trap_enter       : input  logic    ,
    trap_pc          : input  logic<32>,
    trap_cause       : input  logic<64>,
    trap_value       : input  logic<64>,
    mret             : input  logic    ,
    read_data        : output logic<64>,
    trap_vector      : output logic<32>,
    return_pc        : output logic<32>,
) {
    inst csr_bus : CsrBus;
    inst csr_file: CsrFile (
        clk  : clk    ,
        rst_n: rst_n  ,
        csr  : csr_bus,
    );

    always_comb {
        csr_bus.index        = index;
        csr_bus.write_index  = write_index;
        csr_bus.write_enable = write_enable;
        csr_bus.write_op     = write_op;
        csr_bus.write_source = write_source;
        csr_bus.trap_enter   = trap_enter;
        csr_bus.trap_pc      = trap_pc;
        csr_bus.trap_cause   = trap_cause;
        csr_bus.trap_value   = trap_value;
        csr_bus.mret         = mret;
        read_data            = csr_bus.read_data;
        trap_vector          = csr_bus.trap_vector;
        return_pc            = csr_bus.return_pc;
    }
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
fn compressed_isa_csr_alignment_is_fixed_and_consistent() {
    let riscv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let source = [
        fs::read_to_string(riscv.join("src/PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/CsrFile.veryl")).unwrap(),
        TEST_TOP.to_owned(),
    ]
    .join("\n");
    let mut simulator = SimulatorBuilder::new(&source, "CsrFileTestTop")
        .build_native()
        .unwrap();
    let clock = simulator.event("clk");
    let reset = simulator.signal("rst_n");
    let index = simulator.signal("index");
    let write_index = simulator.signal("write_index");
    let write_enable = simulator.signal("write_enable");
    let write_op = simulator.signal("write_op");
    let write_source = simulator.signal("write_source");
    let trap_enter = simulator.signal("trap_enter");
    let trap_pc = simulator.signal("trap_pc");
    let trap_cause = simulator.signal("trap_cause");
    let trap_value = simulator.signal("trap_value");
    let mret = simulator.signal("mret");

    simulator
        .modify(|io| {
            io.set(reset, 0_u8);
            io.set(index, 2_u8);
            io.set(write_index, 0_u8);
            io.set(write_enable, 0_u8);
            io.set(write_op, 0_u8);
            io.set(write_source, 0_u64);
            io.set(trap_enter, 0_u8);
            io.set(trap_pc, 0_u32);
            io.set(trap_cause, 0_u64);
            io.set(trap_value, 0_u64);
            io.set(mret, 0_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(reset, 1_u8)).unwrap();

    assert_eq!(value(&mut simulator, "read_data"), 0x8000_0000_0000_0104);

    simulator
        .modify(|io| {
            io.set(index, 5_u8);
            io.set(write_index, 5_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 0x1237_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x1236);
    assert_eq!(value(&mut simulator, "return_pc"), 0x1236);

    simulator
        .modify(|io| {
            io.set(index, 3_u8);
            io.set(write_index, 3_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 0x567b_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x5678);
    assert_eq!(value(&mut simulator, "trap_vector"), 0x5678);

    simulator
        .modify(|io| {
            io.set(index, 5_u8);
            io.set(trap_enter, 1_u8);
            io.set(trap_pc, 0xabce_u32);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(trap_enter, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0xabce);
    assert_eq!(value(&mut simulator, "return_pc"), 0xabce);
}
