use celox::SimulatorBuilder;

const SOURCE: &str = r#"
module ParameterRepeat #(param ENABLE: bit = 0) (
    a: input logic<64>, raw: output logic<64>, boolean: output logic<64>,
) {
    assign raw = a & {ENABLE repeat 64};
    assign boolean = a & {(ENABLE != 1'b0) repeat 64};
}
"#;

fn main() {
    let expect_unfixed = std::env::args().any(|a| a == "--expect-unfixed");
    let mut failures = 0;
    for enable in [0_u64, 1] {
        let mut sim = SimulatorBuilder::new(SOURCE, "ParameterRepeat")
            .param("ENABLE", enable).build_native().unwrap();
        let a_port = sim.signal("a");
        let raw_port = sim.signal("raw");
        let bool_port = sim.signal("boolean");
        for a in [0_u64, 1, u64::MAX, 0x0123456789abcdef, 1 << 63] {
            sim.modify(|io| io.set(a_port, a)).unwrap();
            let raw = sim.get(raw_port).to_u64_digits().first().copied().unwrap_or(0);
            let boolean = sim.get(bool_port).to_u64_digits().first().copied().unwrap_or(0);
            let expected = if enable == 1 { a } else { 0 };
            println!("enable={enable} a={a:016x} raw={raw:016x} boolean={boolean:016x} expected={expected:016x}");
            assert_eq!(boolean, expected, "explicit boolean control failed");
            if raw != expected { failures += 1; }
        }
    }
    println!("raw_mismatches={failures}");
    if expect_unfixed { assert_eq!(failures, 3, "the recorded defect must reproduce"); }
    else { assert_eq!(failures, 0, "parameter repetition lost high bits"); }
}
