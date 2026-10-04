//! Minimal diagnostic for value-generic module identity across hierarchy.
use celox::SimulatorBuilder;
fn main() {
    let source = r#"
module Leaf::<FLAG: u32> (a: input logic, y: output logic) {
    always_comb { y = 1'b0; if FLAG != 0 { y = a; } }
}
module Wrapped(a: input logic, y: output logic) {
    inst leaf: Leaf::<1>(a, y);
}
module Repro(a: input logic, one: output logic, zero: output logic) {
    inst wrapped: Wrapped(a, y: one);
    inst disabled: Leaf::<0>(a, y: zero);
}
"#;
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(source, "generic_identity", "Repro")
                .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(source, "Repro")
                .build_native()
                .unwrap()
        };
        let input = sim.signal("a");
        sim.modify(|io| io.set(input, 1u8)).unwrap();
        let one = sim.get(sim.signal("one")).to_u64_digits();
        let zero = sim.get(sim.signal("zero")).to_u64_digits();
        println!("mapped={mapped}: one={one:?}, zero={zero:?}; expected one=1, zero=0");
    }
}
