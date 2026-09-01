use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module CsrFileTestTop (
    clk                 : input  clock    ,
    rst_n               : input  reset    ,
    index               : input  logic<4> ,
    write_index         : input  logic<4> ,
    write_enable        : input  logic    ,
    write_op            : input  logic<2> ,
    write_source        : input  logic<64>,
    retire              : input  logic    ,
    stall_dependency    : input  logic    ,
    stall_memory_wait   : input  logic    ,
    stall_redirect      : input  logic    ,
    stall_frontend_empty: input  logic    ,
    counter_freeze      : input  logic    ,
    debug_counter_index : input  logic<3> ,
    trap_enter          : input  logic    ,
    trap_pc             : input  logic<32>,
    trap_cause          : input  logic<64>,
    trap_value          : input  logic<64>,
    mret                : input  logic    ,
    read_data           : output logic<64>,
    trap_vector         : output logic<32>,
    return_pc           : output logic<32>,
    debug_counter_data  : output logic<64>,
) {
    inst csr_bus : CsrBus;
    inst csr_file: CsrFile (
        clk  : clk    ,
        rst_n: rst_n  ,
        csr  : csr_bus,
    );

    always_comb {
        csr_bus.index                = index;
        csr_bus.write_index          = write_index;
        csr_bus.write_enable         = write_enable;
        csr_bus.write_op             = write_op;
        csr_bus.write_source         = write_source;
        csr_bus.retire               = retire;
        csr_bus.stall_dependency     = stall_dependency;
        csr_bus.stall_memory_wait    = stall_memory_wait;
        csr_bus.stall_redirect       = stall_redirect;
        csr_bus.stall_frontend_empty = stall_frontend_empty;
        csr_bus.counter_freeze        = counter_freeze;
        csr_bus.debug_counter_index   = debug_counter_index;
        csr_bus.trap_enter            = trap_enter;
        csr_bus.trap_pc               = trap_pc;
        csr_bus.trap_cause            = trap_cause;
        csr_bus.trap_value            = trap_value;
        csr_bus.mret                  = mret;
        read_data                     = csr_bus.read_data;
        trap_vector                   = csr_bus.trap_vector;
        return_pc                     = csr_bus.return_pc;
        debug_counter_data            = csr_bus.debug_counter_data;
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
        fs::read_to_string(riscv.join("src/PerformanceCounter64.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/PerformanceEventCounters4.veryl")).unwrap(),
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
    let retire = simulator.signal("retire");
    let stall_dependency = simulator.signal("stall_dependency");
    let stall_memory_wait = simulator.signal("stall_memory_wait");
    let stall_redirect = simulator.signal("stall_redirect");
    let stall_frontend_empty = simulator.signal("stall_frontend_empty");
    let counter_freeze = simulator.signal("counter_freeze");
    let debug_counter_index = simulator.signal("debug_counter_index");
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
            io.set(retire, 0_u8);
            io.set(stall_dependency, 0_u8);
            io.set(stall_memory_wait, 0_u8);
            io.set(stall_redirect, 0_u8);
            io.set(stall_frontend_empty, 0_u8);
            io.set(counter_freeze, 0_u8);
            io.set(debug_counter_index, 0_u8);
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

#[test]
fn architectural_and_stall_counters_increment_and_write_exactly() {
    let riscv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let source = [
        fs::read_to_string(riscv.join("src/PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/PerformanceCounter64.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/PerformanceEventCounters4.veryl")).unwrap(),
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
    let retire = simulator.signal("retire");
    let stall_dependency = simulator.signal("stall_dependency");
    let stall_memory_wait = simulator.signal("stall_memory_wait");
    let stall_redirect = simulator.signal("stall_redirect");
    let stall_frontend_empty = simulator.signal("stall_frontend_empty");
    let counter_freeze = simulator.signal("counter_freeze");
    let debug_counter_index = simulator.signal("debug_counter_index");
    let trap_enter = simulator.signal("trap_enter");
    let trap_pc = simulator.signal("trap_pc");
    let trap_cause = simulator.signal("trap_cause");
    let trap_value = simulator.signal("trap_value");
    let mret = simulator.signal("mret");

    simulator
        .modify(|io| {
            io.set(reset, 0_u8);
            io.set(index, 12_u8);
            io.set(write_index, 0_u8);
            io.set(write_enable, 0_u8);
            io.set(write_op, 0_u8);
            io.set(write_source, 0_u64);
            io.set(retire, 0_u8);
            io.set(stall_dependency, 0_u8);
            io.set(stall_memory_wait, 0_u8);
            io.set(stall_redirect, 0_u8);
            io.set(stall_frontend_empty, 0_u8);
            io.set(counter_freeze, 0_u8);
            io.set(debug_counter_index, 0_u8);
            io.set(trap_enter, 0_u8);
            io.set(trap_pc, 0_u32);
            io.set(trap_cause, 0_u64);
            io.set(trap_value, 0_u64);
            io.set(mret, 0_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(reset, 1_u8)).unwrap();

    assert_eq!(value(&mut simulator, "read_data"), 0);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 1);

    simulator
        .modify(|io| {
            io.set(index, 13_u8);
            io.set(retire, 1_u8);
            io.set(stall_dependency, 1_u8);
        })
        .unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0);
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(retire, 0_u8);
            io.set(stall_dependency, 0_u8);
        })
        .unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 1);

    // CSR writes win over an event in the same cycle. The following event is
    // then counted normally.
    simulator
        .modify(|io| {
            io.set(write_index, 13_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 40_u64);
            io.set(retire, 1_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(write_enable, 0_u8);
            io.set(retire, 0_u8);
        })
        .unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 40);
    simulator.modify(|io| io.set(retire, 1_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 40);
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(retire, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 41);

    simulator
        .modify(|io| {
            io.set(index, 12_u8);
            io.set(write_index, 12_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 0x100_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x100);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x101);

    // Selector zero addresses the dependency counter, which already observed
    // the pulse above. Replace wins over a simultaneous pulse, then increment.
    simulator
        .modify(|io| {
            io.set(index, 15_u8);
            io.set(write_index, 15_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 10_u64);
            io.set(stall_dependency, 1_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 10);
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| io.set(stall_dependency, 0_u8))
        .unwrap();
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 11);

    // Selector one: data writes implement the same replace/set/clear CSR ops.
    simulator
        .modify(|io| {
            io.set(index, 14_u8);
            io.set(write_index, 14_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 1_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(index, 15_u8);
            io.set(write_index, 15_u8);
            io.set(write_source, 0x10_u64);
            io.set(write_op, 1_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x10);
    simulator
        .modify(|io| {
            io.set(write_source, 1_u64);
            io.set(write_op, 2_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(write_enable, 0_u8);
            io.set(stall_memory_wait, 1_u8);
        })
        .unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x10);
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| io.set(stall_memory_wait, 0_u8))
        .unwrap();
    // Pipeline-control predicates are registered once before driving the
    // wide counter CE cone, so the event becomes visible on the next edge.
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x11);

    // Selectors two and three address independent storage and count only their
    // corresponding event inputs.
    for (selector, event_name, expected) in [(2_u64, "redirect", 0x21_u64), (3, "frontend", 0x31)] {
        simulator
            .modify(|io| {
                io.set(index, 14_u8);
                io.set(write_index, 14_u8);
                io.set(write_enable, 1_u8);
                io.set(write_op, 0_u8);
                io.set(write_source, selector);
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        simulator
            .modify(|io| {
                io.set(index, 15_u8);
                io.set(write_index, 15_u8);
                io.set(write_source, selector << 4);
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        simulator
            .modify(|io| {
                io.set(write_enable, 0_u8);
                if event_name == "redirect" {
                    io.set(stall_redirect, 1_u8);
                } else {
                    io.set(stall_frontend_empty, 1_u8);
                }
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        simulator
            .modify(|io| {
                io.set(stall_redirect, 0_u8);
                io.set(stall_frontend_empty, 0_u8);
            })
            .unwrap();
        simulator.tick(clock).unwrap();
        assert_eq!(value(&mut simulator, "read_data"), expected);
    }

    for (debug_index, expected) in [(1_u8, 41_u64), (2, 11), (3, 0x11), (4, 0x21), (5, 0x31)] {
        simulator
            .modify(|io| io.set(debug_counter_index, debug_index))
            .unwrap();
        assert_eq!(
            value(&mut simulator, "debug_counter_data"),
            expected,
            "wrong counter on debug selector {debug_index}"
        );
    }

    // Freeze stops every automatic increment but never blocks an explicit CSR
    // write. This lets the hardware transport return a coherent snapshot.
    simulator
        .modify(|io| {
            io.set(index, 12_u8);
            io.set(debug_counter_index, 0_u8);
            io.set(counter_freeze, 1_u8);
            io.set(retire, 1_u8);
            io.set(stall_frontend_empty, 1_u8);
        })
        .unwrap();
    let frozen_cycle = value(&mut simulator, "debug_counter_data");
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), frozen_cycle);
    assert_eq!(value(&mut simulator, "debug_counter_data"), frozen_cycle);
    simulator
        .modify(|io| {
            io.set(write_index, 12_u8);
            io.set(write_enable, 1_u8);
            io.set(write_source, 0x55_u64);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator.modify(|io| io.set(write_enable, 0_u8)).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x55);
    simulator.tick(clock).unwrap();
    assert_eq!(value(&mut simulator, "read_data"), 0x55);
}
