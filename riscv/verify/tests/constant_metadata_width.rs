use celox::SimulatorBuilder;

macro_rules! check_backend {
    ($test:ident, $build:ident) => {
        #[test]
        fn $test() {
            // Constants/defaults also publish values for later constant folds.
            for source in [
                "module Top (a: input logic<64>, o: output logic<64>) { const P: bit = 64'd3; assign o = a & {P repeat 64}; }",
                "module Top #(param P: bit = 64'd3) (a: input logic<64>, o: output logic<64>) { assign o = a & {P repeat 64}; }",
                "module Top (a: input logic<64>, o: output logic<64>) { const P: signed logic<8> = 64'd255; let value: logic<64> = P; assign o = a & value; }",
            ] {
                let mut sim = SimulatorBuilder::new(source, "Top").$build().unwrap();
                let a = sim.signal("a");
                let o = sim.signal("o");
                for input in [0_u64, 1, u64::MAX, 0x0123_4567_89ab_cdef, 1 << 63] {
                    sim.modify(|io| io.set(a, input)).unwrap();
                    assert_eq!(sim.get(o), input.into(), "typed constant: {source}");
                }
            }

            // This bypasses builder.param(): the declared type is already correct,
            // but constant metadata must also carry the truncated value.
            let mut sim = SimulatorBuilder::new(
                "module Child #(param P: bit = 0) (a: input logic<64>, o: output logic<64>) { assign o = a & {P repeat 64}; } module Top (a: input logic<64>, o: output logic<64>) { inst child: Child #(P: 64'd1) (a: a, o: o); }",
                "Top",
            ).$build().unwrap();
            let a = sim.signal("a");
            let o = sim.signal("o");
            for input in [0_u64, 1, u64::MAX, 0x0123_4567_89ab_cdef, 1 << 63] {
                sim.modify(|io| io.set(a, input)).unwrap();
                assert_eq!(sim.get(o), input.into(), "ordinary typed HDL override");
            }
        }
    };
}

check_backend!(native_constant_metadata, build_native);
check_backend!(cranelift_constant_metadata, build_cranelift);
check_backend!(wasm_constant_metadata, build_wasm);
check_backend!(interpreter_constant_metadata, build_interpreter);
