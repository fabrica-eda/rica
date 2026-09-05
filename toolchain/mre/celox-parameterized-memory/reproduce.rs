//! Compare an unchanged memory with an otherwise-unused module parameter.
use std::{error::Error, fs, path::Path};

use celox::SimulatorBuilder;

#[path = "../../../riscv/verify/tests/hardware_memory_debug_stream.rs"]
mod stream;

fn main() -> Result<(), Box<dyn Error>> {
    let expect_unfixed = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--expect-unfixed" => true,
        _ => return Err("usage: celox-parameterized-memory [--expect-unfixed]".into()),
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../toolchain/mre/celox-parameterized-memory/HardwareMemory.veryl");
    let original = fs::read_to_string(path)?;
    assert_eq!(original.matches("module HardwareMemory (").count(), 1);
    let parameterized = original.replace(
        "module HardwareMemory (",
        "module HardwareMemory #(param FAST_LSU: u32 = 0,) (",
    );
    let mut native_results = Vec::new();
    let mut all_ok = true;
    for (label, source) in [
        ("original", &original),
        ("unused-parameter", &parameterized),
    ] {
        let native =
            stream::check_stream(SimulatorBuilder::new(source, "HardwareMemory").build_native()?);
        println!("native {label}: {native:?}");
        all_ok &= native.is_ok();
        native_results.push(native);
        let wasm =
            stream::check_stream(SimulatorBuilder::new(source, "HardwareMemory").build_wasm()?);
        println!("wasm {label}: {wasm:?}");
        all_ok &= wasm.is_ok();
    }
    if expect_unfixed {
        if native_results[0].is_ok()
            && native_results[1].as_ref().is_err_and(|error| {
                error
                    == "data base=0x0 offset=0: got=0x453e373029221b14, expected=0x3e373029221b140d"
            })
        {
            Ok(())
        } else {
            Err("the recorded native counterexample was not reproduced".into())
        }
    } else if all_ok {
        Ok(())
    } else {
        Err("memory stream semantics differed; use --expect-unfixed only to reproduce the recorded defect".into())
    }
}
