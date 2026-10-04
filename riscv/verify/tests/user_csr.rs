use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};
fn set(s: &mut Simulator<NativeBackend>, n: &str, v: u64) {
    let k = s.signal(n);
    s.modify(|io| {
        if n == "write_source" {
            io.set(k, v)
        } else {
            io.set(k, v as u8)
        }
    })
    .unwrap()
}
fn get(s: &mut Simulator<NativeBackend>, n: &str) -> u64 {
    s.get(s.signal(n))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}
fn tick(s: &mut Simulator<NativeBackend>) {
    let e = s.event("clk");
    s.tick(e).unwrap()
}
fn write(s: &mut Simulator<NativeBackend>, i: u64, v: u64) {
    set(s, "write_index", i);
    set(s, "write_source", v);
    set(s, "write_enable", 1);
    tick(s);
    set(s, "write_enable", 0);
}
fn read(s: &mut Simulator<NativeBackend>, i: u64) -> u64 {
    set(s, "index", i);
    get(s, "read_data")
}
#[test]
fn user_transition_mprv_interrupts_counter_control_and_pmp_locks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = [
        "PipelineInterfaces.veryl",
        "PerformanceCounter64.veryl",
        "PerformanceEventCounters4.veryl",
        "CsrFile.veryl",
    ]
    .map(|n| fs::read_to_string(root.join(n)).unwrap())
    .join("\n")
        + include_str!("fixtures/user_csr.veryl");
    for mapped in [false, true] {
        let mut s = if mapped {
            let rtl =
                struo_frontend_veryl::analyze_and_lower(&source, "usercsr", "UserCsrTest").unwrap();
            let net = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&net)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "UserCsrTest")
                .build_native()
                .unwrap()
        };
        for n in [
            "rst_n",
            "index",
            "write_index",
            "write_enable",
            "write_op",
            "write_source",
            "trap_enter",
            "mret",
            "machine_interrupts",
        ] {
            set(&mut s, n, 0)
        }
        tick(&mut s);
        set(&mut s, "rst_n", 1);
        assert_eq!(read(&mut s, 2) & (1 << 20), 1 << 20);
        write(&mut s, 1, 1 << 17);
        assert_eq!(get(&mut s, "user_mode"), 0);
        assert_eq!(get(&mut s, "data_user"), 1);
        write(&mut s, 16, 128);
        set(&mut s, "machine_interrupts", 2);
        assert_eq!(get(&mut s, "interrupt_pending"), 0);
        set(&mut s, "mret", 1);
        tick(&mut s);
        set(&mut s, "mret", 0);
        assert_eq!(get(&mut s, "user_mode"), 1);
        assert_eq!(read(&mut s, 1) & (1 << 17), 0);
        assert_eq!(get(&mut s, "interrupt_pending"), 1);
        set(&mut s, "trap_enter", 1);
        tick(&mut s);
        set(&mut s, "trap_enter", 0);
        assert_eq!(get(&mut s, "user_mode"), 0);
        assert_eq!(read(&mut s, 1) & 0x1800, 0);
        write(&mut s, 25, u64::MAX);
        assert_eq!(read(&mut s, 25), 5);
        write(&mut s, 20, 0x12345678);
        write(&mut s, 21, 0x23456789);
        write(&mut s, 18, 0x8900); // locked TOR entry 1 locks address 0 as well
        write(&mut s, 20, 0);
        write(&mut s, 21, 0);
        write(&mut s, 18, 0);
        assert_eq!(read(&mut s, 20), 0x12345678);
        assert_eq!(read(&mut s, 21), 0x23456789);
        assert_eq!(read(&mut s, 18), 0x8900);
        write(&mut s, 23, u64::MAX);
        assert_eq!(read(&mut s, 23), 0x3fffffff);
        write(&mut s, 24, u64::MAX);
        assert_eq!(read(&mut s, 24), 0);
        write(&mut s, 19, u64::MAX);
        assert_eq!(read(&mut s, 19), 0);
    }
}
