use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module CsrFileTestTop #(param ENABLE_INTERRUPTS: bit = 0, param FULL_ADDRESS: bit = 0) (
    clk                 : input  clock    ,
    rst_n               : input  reset    ,
    index               : input  logic<6> ,
    write_index         : input  logic<6> ,
    write_enable        : input  logic    ,
    write_op            : input  logic<2> ,
    write_source        : input  logic<64>,
    retire              : input  logic    ,
    stall_dependency    : input  logic    ,
    stall_memory_wait   : input  logic    ,
    stall_redirect      : input  logic    ,
    stall_frontend_empty: input  logic    ,
    predictor_conditional_resolved: input logic,
    predictor_direction_wrong     : input logic,
    predictor_btb_miss            : input logic,
    predictor_ras_fallback        : input logic,
    predictor_repair              : input logic,
    counter_freeze      : input  logic    ,
    debug_counter_index : input  logic<4> ,
    trap_enter          : input  logic    ,
    trap_pc             : input  logic<64>,
    trap_cause          : input  logic<64>,
    trap_value          : input  logic<64>,
    mret                : input  logic    ,
    machine_interrupts  : input  logic<3> ,
    interrupt_pending   : output logic,
    interrupt_cause     : output logic<4>,
    read_data           : output logic<64>,
    trap_vector         : output logic<64>,
    return_pc           : output logic<64>,
    debug_counter_data  : output logic<64>,
) {
    inst csr_bus : CsrBus;
    inst csr_file: CsrFile #(ENABLE_INTERRUPTS: ENABLE_INTERRUPTS, FULL_ADDRESS: FULL_ADDRESS) (
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
        csr_bus.predictor_conditional_resolved = predictor_conditional_resolved;
        csr_bus.predictor_direction_wrong      = predictor_direction_wrong;
        csr_bus.predictor_btb_miss             = predictor_btb_miss;
        csr_bus.predictor_ras_fallback         = predictor_ras_fallback;
        csr_bus.predictor_repair               = predictor_repair;
        csr_bus.counter_freeze        = counter_freeze;
        csr_bus.debug_counter_index   = debug_counter_index;
        csr_bus.trap_enter            = trap_enter;
        csr_bus.trap_pc               = trap_pc;
        csr_bus.trap_cause            = trap_cause;
        csr_bus.trap_value            = trap_value;
        csr_bus.mret                  = mret;
        csr_bus.sret                  = 1'b0;
        csr_bus.trap_supervisor       = 1'b0;
        csr_bus.machine_interrupts = machine_interrupts;
        interrupt_pending = csr_bus.interrupt_pending;
        interrupt_cause = csr_bus.interrupt_cause;
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

const PREDICTOR_EVENTS: [&str; 5] = [
    "predictor_conditional_resolved",
    "predictor_direction_wrong",
    "predictor_btb_miss",
    "predictor_ras_fallback",
    "predictor_repair",
];

struct CsrFileDriver {
    simulator: Simulator<NativeBackend>,
}

impl CsrFileDriver {
    fn new() -> Self {
        Self::with_interrupts(false)
    }

    fn with_interrupts(enabled: bool) -> Self {
        Self::with_backend(enabled, false)
    }

    fn with_backend(enabled: bool, mapped: bool) -> Self {
        Self::with_address_backend(enabled, false, mapped)
    }

    fn with_address_backend(enabled: bool, full_address: bool, mapped: bool) -> Self {
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
        let source = source.replace(
            "param FULL_ADDRESS: bit = 0",
            &format!("param FULL_ADDRESS: bit = {}", u8::from(full_address)),
        );
        let simulator = if mapped {
            let source = source.replace(
                "param ENABLE_INTERRUPTS: bit = 0",
                &format!("param ENABLE_INTERRUPTS: bit = {}", u8::from(enabled)),
            );
            let rtl =
                struo_frontend_veryl::analyze_and_lower(&source, "csr_test", "CsrFileTestTop")
                    .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                struo_target_ecp5::MappingOptions {
                    timing_goal_mhz: 175,
                    ..Default::default()
                },
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "CsrFileTestTop")
                .param("ENABLE_INTERRUPTS", u64::from(enabled))
                .build_native()
                .unwrap()
        };
        let mut driver = Self { simulator };

        for name in [
            "rst_n",
            "index",
            "write_index",
            "write_enable",
            "write_op",
            "retire",
            "stall_dependency",
            "stall_memory_wait",
            "stall_redirect",
            "stall_frontend_empty",
            "counter_freeze",
            "debug_counter_index",
            "trap_enter",
            "mret",
            "machine_interrupts",
        ]
        .into_iter()
        .chain(PREDICTOR_EVENTS)
        {
            driver.set_u8(name, 0);
        }
        for name in ["write_source", "trap_cause", "trap_value"] {
            driver.set_u64(name, 0);
        }
        driver.set_u32("trap_pc", 0);
        driver.tick();
        driver.set_u8("rst_n", 1);
        driver
    }

    fn set_u8(&mut self, name: &str, input: u8) {
        let signal = self.simulator.signal(name);
        self.simulator.modify(|io| io.set(signal, input)).unwrap();
    }

    fn set_u32(&mut self, name: &str, input: u32) {
        let signal = self.simulator.signal(name);
        self.simulator.modify(|io| io.set(signal, input)).unwrap();
    }

    fn set_u64(&mut self, name: &str, input: u64) {
        let signal = self.simulator.signal(name);
        self.simulator.modify(|io| io.set(signal, input)).unwrap();
    }

    fn get(&mut self, name: &str) -> u64 {
        value(&mut self.simulator, name)
    }

    fn tick(&mut self) {
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock).unwrap();
    }

    fn select_counter(&mut self, selector: u8) {
        self.set_u8("index", 14);
        self.set_u8("write_index", 14);
        self.set_u8("write_op", 0);
        self.set_u64("write_source", u64::from(selector));
        self.set_u8("write_enable", 1);
        self.tick();
        self.set_u8("write_enable", 0);
        self.set_u8("index", 15);
    }

    fn write_selected_counter(&mut self, value: u64) {
        self.set_u8("index", 15);
        self.set_u8("write_index", 15);
        self.set_u8("write_op", 0);
        self.set_u64("write_source", value);
        self.set_u8("write_enable", 1);
        self.tick();
        self.set_u8("write_enable", 0);
    }

    fn predictor_counter(&mut self, predictor_index: u8) -> u64 {
        self.set_u8("debug_counter_index", predictor_index + 6);
        self.get("debug_counter_data")
    }

    fn set_all_predictor_events(&mut self, value: u8) {
        for name in PREDICTOR_EVENTS {
            self.set_u8(name, value);
        }
    }
}

#[test]
fn full_address_native_and_mapped_csrs_preserve_upper_bits_and_trap_priority() {
    for mapped in [false, true] {
        let mut driver = CsrFileDriver::with_address_backend(true, true, mapped);
        for (index, bits, aligned) in [
            (3, 0xffff_ffff_8000_123b, 0xffff_ffff_8000_1238),
            (5, 0x0000_0001_8000_0123, 0x0000_0001_8000_0122),
            (7, 0xfedc_ba98_7654_3210, 0xfedc_ba98_7654_3210),
        ] {
            driver.set_u8("index", index);
            driver.set_u8("write_index", index);
            driver.set_u8("write_enable", 1);
            driver.set_u8("write_op", 0);
            driver.set_u64("write_source", bits);
            driver.tick();
            driver.set_u8("write_enable", 0);
            assert_eq!(driver.get("read_data"), aligned);
        }
        driver.set_u8("write_index", 5);
        driver.set_u64("write_source", 0x4320);
        driver.set_u8("write_enable", 1);
        driver.set_u8("trap_enter", 1);
        driver.set_u64("trap_pc", 0xffff_fffe_ffff_fffe);
        driver.set_u64("trap_value", 0xffff_ffff_0000_0000);
        driver.set_u64("trap_cause", 1);
        driver.tick();
        driver.set_u8("write_enable", 0);
        driver.set_u8("trap_enter", 0);
        driver.set_u8("index", 5);
        assert_eq!(driver.get("read_data"), 0xffff_fffe_ffff_fffe);
        assert_eq!(driver.get("return_pc"), 0xffff_fffe_ffff_fffe);
        driver.set_u8("index", 7);
        assert_eq!(driver.get("read_data"), 0xffff_ffff_0000_0000);
        assert_eq!(driver.get("trap_vector"), 0xffff_ffff_8000_1238);
    }
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
    let predictor_conditional_resolved = simulator.signal("predictor_conditional_resolved");
    let predictor_direction_wrong = simulator.signal("predictor_direction_wrong");
    let predictor_btb_miss = simulator.signal("predictor_btb_miss");
    let predictor_ras_fallback = simulator.signal("predictor_ras_fallback");
    let predictor_repair = simulator.signal("predictor_repair");
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
            io.set(predictor_conditional_resolved, 0_u8);
            io.set(predictor_direction_wrong, 0_u8);
            io.set(predictor_btb_miss, 0_u8);
            io.set(predictor_ras_fallback, 0_u8);
            io.set(predictor_repair, 0_u8);
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
    let predictor_conditional_resolved = simulator.signal("predictor_conditional_resolved");
    let predictor_direction_wrong = simulator.signal("predictor_direction_wrong");
    let predictor_btb_miss = simulator.signal("predictor_btb_miss");
    let predictor_ras_fallback = simulator.signal("predictor_ras_fallback");
    let predictor_repair = simulator.signal("predictor_repair");
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
            io.set(predictor_conditional_resolved, 0_u8);
            io.set(predictor_direction_wrong, 0_u8);
            io.set(predictor_btb_miss, 0_u8);
            io.set(predictor_ras_fallback, 0_u8);
            io.set(predictor_repair, 0_u8);
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
        .modify(|io| io.set(debug_counter_index, 6_u8))
        .unwrap();
    assert_eq!(
        value(&mut simulator, "debug_counter_data"),
        0,
        "predictor pulse bypassed its timing register"
    );
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

    // Predictor events are independent rather than mutually exclusive: one
    // resolved branch may be a direction miss, a cold BTB miss, and a repair
    // in the same cycle. Each late event passes through its own one-cycle
    // pulse register before reaching its counter CE.
    simulator
        .modify(|io| {
            io.set(predictor_conditional_resolved, 1_u8);
            io.set(predictor_direction_wrong, 1_u8);
            io.set(predictor_btb_miss, 1_u8);
            io.set(predictor_ras_fallback, 1_u8);
            io.set(predictor_repair, 1_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();
    simulator
        .modify(|io| {
            io.set(predictor_conditional_resolved, 0_u8);
            io.set(predictor_direction_wrong, 0_u8);
            io.set(predictor_btb_miss, 0_u8);
            io.set(predictor_ras_fallback, 0_u8);
            io.set(predictor_repair, 0_u8);
        })
        .unwrap();
    simulator.tick(clock).unwrap();

    for selector in 4_u64..=8 {
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
                io.set(write_enable, 0_u8);
            })
            .unwrap();
        assert_eq!(
            value(&mut simulator, "read_data"),
            1,
            "wrong predictor counter on CSR selector {selector}"
        );
    }

    for (debug_index, expected) in [
        (1_u8, 41_u64),
        (2, 11),
        (3, 0x11),
        (4, 0x21),
        (5, 0x31),
        (6, 1),
        (7, 1),
        (8, 1),
        (9, 1),
        (10, 1),
    ] {
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

#[test]
fn predictor_counter_csr_write_wins_over_delayed_same_counter_events() {
    let mut driver = CsrFileDriver::new();

    for (predictor_index, event_name) in PREDICTOR_EVENTS.into_iter().enumerate() {
        let predictor_index = predictor_index as u8;
        driver.select_counter(predictor_index + 4);

        // A write and an event originating in the same cycle must leave the
        // replacement value intact after the event passes through its timing
        // register. The delayed write marker suppresses that increment.
        let same_cycle_value = 0x100 + u64::from(predictor_index);
        driver.set_u8(event_name, 1);
        driver.set_u8("write_index", 15);
        driver.set_u8("write_op", 0);
        driver.set_u64("write_source", same_cycle_value);
        driver.set_u8("write_enable", 1);
        driver.tick();
        driver.set_u8(event_name, 0);
        driver.set_u8("write_enable", 0);
        driver.tick();
        assert_eq!(
            driver.predictor_counter(predictor_index),
            same_cycle_value,
            "same-cycle event escaped write priority for predictor selector {}",
            predictor_index + 4
        );

        // Also cover the other ordering: an event already in the delay FF is
        // presented directly to the counter on the edge carrying a CSR write.
        // PerformanceCounter64's replacement path must win on that edge.
        driver.set_u8(event_name, 1);
        driver.tick();
        driver.set_u8(event_name, 0);
        let delayed_event_value = 0x200 + u64::from(predictor_index);
        driver.set_u8("write_index", 15);
        driver.set_u8("write_op", 0);
        driver.set_u64("write_source", delayed_event_value);
        driver.set_u8("write_enable", 1);
        driver.tick();
        driver.set_u8("write_enable", 0);
        driver.tick();
        assert_eq!(
            driver.predictor_counter(predictor_index),
            delayed_event_value,
            "write lost to an already-delayed event for predictor selector {}",
            predictor_index + 4
        );

        // The priority marker is one-shot; a later ordinary event still
        // increments the selected counter exactly once.
        driver.set_u8(event_name, 1);
        driver.tick();
        driver.set_u8(event_name, 0);
        driver.tick();
        assert_eq!(
            driver.predictor_counter(predictor_index),
            delayed_event_value + 1,
            "write suppression leaked into a later event for predictor selector {}",
            predictor_index + 4
        );
    }
}

#[test]
fn invalid_predictor_selectors_read_zero_and_ignore_writes() {
    let mut driver = CsrFileDriver::new();

    for selector in 9_u8..=15 {
        driver.select_counter(selector);
        assert_eq!(
            driver.get("read_data"),
            0,
            "invalid predictor selector {selector} did not read as zero"
        );

        driver.write_selected_counter(0xdead_0000 + u64::from(selector));
        assert_eq!(
            driver.get("read_data"),
            0,
            "invalid predictor selector {selector} retained a CSR write"
        );
    }

    for predictor_index in 0_u8..5 {
        assert_eq!(
            driver.predictor_counter(predictor_index),
            0,
            "invalid selector write aliased predictor counter {predictor_index}"
        );
    }
}

#[test]
fn predictor_counter_freeze_includes_pending_events_and_drops_frozen_pulses() {
    let mut driver = CsrFileDriver::new();

    // Events accepted immediately before freeze are already in the timing
    // registers. The delayed freeze admits these terminal events exactly once.
    driver.set_all_predictor_events(1);
    driver.tick();
    driver.set_all_predictor_events(0);
    driver.set_u8("counter_freeze", 1);
    driver.tick();
    for predictor_index in 0_u8..5 {
        assert_eq!(
            driver.predictor_counter(predictor_index),
            1,
            "freeze lost pending predictor event {predictor_index}"
        );
    }

    // Pulses presented while frozen are discarded rather than queued for the
    // eventual release edge.
    driver.set_all_predictor_events(1);
    driver.tick();
    driver.set_all_predictor_events(0);
    driver.tick();
    for predictor_index in 0_u8..5 {
        assert_eq!(
            driver.predictor_counter(predictor_index),
            1,
            "frozen predictor event {predictor_index} was counted"
        );
    }

    // Freeze blocks automatic increments only. Explicit CSR replacement must
    // still update every predictor counter in a coherent snapshot.
    for predictor_index in 0_u8..5 {
        let replacement = 0x300 + u64::from(predictor_index);
        driver.select_counter(predictor_index + 4);
        driver.write_selected_counter(replacement);
        assert_eq!(
            driver.predictor_counter(predictor_index),
            replacement,
            "freeze blocked CSR write to predictor counter {predictor_index}"
        );
    }

    // The first event offered after release is captured while the delayed
    // freeze is still active, then counted on the following edge.
    driver.set_u8("counter_freeze", 0);
    driver.set_all_predictor_events(1);
    driver.tick();
    driver.set_all_predictor_events(0);
    for predictor_index in 0_u8..5 {
        assert_eq!(
            driver.predictor_counter(predictor_index),
            0x300 + u64::from(predictor_index),
            "predictor counter {predictor_index} changed on freeze-release capture edge"
        );
    }
    driver.tick();
    for predictor_index in 0_u8..5 {
        assert_eq!(
            driver.predictor_counter(predictor_index),
            0x301 + u64::from(predictor_index),
            "first post-freeze predictor event {predictor_index} was not counted"
        );
    }
}

impl CsrFileDriver {
    fn write(&mut self, index: u8, operation: u8, source: u64) {
        self.set_u8("write_index", index);
        self.set_u8("write_op", operation);
        self.set_u64("write_source", source);
        self.set_u8("write_enable", 1);
        self.tick();
        self.set_u8("write_enable", 0);
    }

    fn read(&mut self, index: u8) -> u64 {
        self.set_u8("index", index);
        self.get("read_data")
    }
}

fn interrupt_bits(levels: u8) -> u64 {
    u64::from(levels & 1) << 3
        | u64::from((levels >> 1) & 1) << 7
        | u64::from((levels >> 2) & 1) << 11
}

#[test]
fn machine_interrupt_masks_levels_and_priority_share_architectural_state() {
    for enabled in [false, true] {
        let mut driver = CsrFileDriver::with_interrupts(enabled);
        for global in [false, true] {
            driver.write(1, 0, if global { 8 } else { 0 });
            for mask in 0..8 {
                driver.write(16, 0, interrupt_bits(mask) | !0x888);
                assert_eq!(
                    driver.read(16),
                    if enabled { interrupt_bits(mask) } else { 0 }
                );
                for levels in 0..8 {
                    driver.set_u8("machine_interrupts", levels);
                    assert_eq!(
                        driver.read(17),
                        if enabled { interrupt_bits(levels) } else { 0 }
                    );
                    let active = mask & levels;
                    assert_eq!(
                        driver.get("interrupt_pending") != 0,
                        enabled && global && active != 0
                    );
                    if enabled && active != 0 {
                        let cause = if active & 4 != 0 {
                            11
                        } else if active & 1 != 0 {
                            3
                        } else {
                            7
                        };
                        assert_eq!(driver.get("interrupt_cause"), cause);
                    }
                    for op in 0..4 {
                        driver.write(17, op, u64::MAX);
                        assert_eq!(
                            driver.read(17),
                            if enabled { interrupt_bits(levels) } else { 0 }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn machine_interrupt_warl_updates_do_not_alias_status_or_counters() {
    let mut driver = CsrFileDriver::with_interrupts(true);
    driver.write(1, 0, 0x88);
    driver.write(16, 0, 0);
    driver.write(16, 1, 0x808);
    assert_eq!(driver.read(16), 0x808);
    driver.write(16, 2, 0x800);
    assert_eq!(driver.read(16), 8);
    driver.write(16, 3, u64::MAX);
    assert_eq!(driver.read(16), 8);
    assert_eq!(driver.read(1), 0x1888);
    for index in 18..32 {
        driver.write(index, 0, u64::MAX);
        assert_eq!(driver.read(index), 0);
    }
    assert_eq!(driver.read(1), 0x1888);
    assert_eq!(driver.read(16), 8);
    assert_eq!(driver.read(13), 0);
}

#[test]
fn interrupt_traps_preserve_xlen_cause_and_mret_rechecks_pending_level() {
    let mut driver = CsrFileDriver::with_interrupts(true);
    driver.write(1, 0, 8);
    driver.write(16, 0, 0x888);
    driver.set_u8("machine_interrupts", 2);
    assert_eq!(driver.get("interrupt_pending"), 1);
    driver.set_u32("trap_pc", 0x8000_0122);
    driver.set_u64("trap_cause", (1 << 63) | 7);
    driver.set_u64("trap_value", 0);
    driver.set_u8("trap_enter", 1);
    driver.tick();
    driver.set_u8("trap_enter", 0);
    assert_eq!(driver.read(6), (1 << 63) | 7);
    assert_eq!(driver.read(5), 0x8000_0122);
    assert_eq!(driver.read(7), 0);
    assert_eq!(driver.read(1), 0x1880);
    assert_eq!(driver.get("interrupt_pending"), 0);
    driver.set_u8("mret", 1);
    driver.tick();
    driver.set_u8("mret", 0);
    assert_eq!(driver.read(1), 0x1888);
    assert_eq!(driver.get("interrupt_pending"), 1);
    driver.set_u8("machine_interrupts", 0);
    assert_eq!(driver.get("interrupt_pending"), 0);

    driver.set_u8("trap_enter", 1);
    driver.set_u64("trap_cause", 2);
    driver.set_u64("trap_value", 0xdead_beef);
    driver.tick();
    driver.set_u8("trap_enter", 0);
    assert_eq!(
        driver.read(6),
        2,
        "a synchronous trap must clear mcause.Interrupt"
    );
    driver.write(6, 1, 1 << 63);
    assert_eq!(driver.read(6), (1 << 63) | 2);
    driver.write(6, 2, 1 << 63);
    assert_eq!(driver.read(6), 2);
}

#[test]
fn mapped_interrupt_bank_preserves_masks_priority_trap_state_and_return() {
    let mut driver = CsrFileDriver::with_backend(true, true);
    for mask in 1..8 {
        driver.write(16, 0, interrupt_bits(mask));
        for levels in 1..8 {
            driver.write(1, 0, 8);
            driver.set_u8("machine_interrupts", levels);
            assert_eq!(driver.read(16), interrupt_bits(mask));
            assert_eq!(driver.read(17), interrupt_bits(levels));
            let active = mask & levels;
            assert_eq!(driver.get("interrupt_pending") != 0, active != 0);
            if active == 0 {
                continue;
            }
            let cause = if active & 4 != 0 {
                11
            } else if active & 1 != 0 {
                3
            } else {
                7
            };
            assert_eq!(driver.get("interrupt_cause"), cause);
            driver.set_u32("trap_pc", 0x8000_0002);
            driver.set_u64("trap_cause", (1 << 63) | cause);
            driver.set_u64("trap_value", 0);
            driver.set_u8("trap_enter", 1);
            driver.tick();
            driver.set_u8("trap_enter", 0);
            assert_eq!(driver.read(1), 0x1880);
            assert_eq!(driver.read(5), 0x8000_0002);
            assert_eq!(driver.read(6), (1 << 63) | cause);
            assert_eq!(driver.read(7), 0);
            assert_eq!(driver.get("interrupt_pending"), 0);
            driver.write(17, 0, 0);
            assert_eq!(driver.read(17), interrupt_bits(levels));
            driver.set_u8("mret", 1);
            driver.tick();
            driver.set_u8("mret", 0);
            assert_eq!(driver.read(1), 0x1888);
            assert_eq!(driver.get("interrupt_pending"), 1);
        }
    }
}
