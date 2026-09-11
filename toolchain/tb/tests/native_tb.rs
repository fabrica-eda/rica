use std::process::Command;

#[test]
fn veryl_testbenches() {
    // Read HDL at runtime: editing a TB does not recompile this Rust harness.
    for (backend, filter) in [
        ("native", ""),
        ("cranelift", "constant_metadata"),
        ("wasm", "constant_metadata"),
        ("interpreter", "constant_metadata"),
    ] {
        assert!(
            Command::new(env!("CARGO_BIN_EXE_rica-tb"))
                .args([backend, filter])
                .status()
                .expect("start Veryl TB runner")
                .success(),
            "{backend} {filter}"
        );
    }
}
