//! Reproduces Celox 0.3.1 native concat-width corruption through the public API.

use std::error::Error;

use celox::Simulator;
use celox::frontend_sdk::{BinaryOp, Edge, FrontendArtifact, ModuleBuilder, ValueType};

fn artifact(data_width: usize) -> Result<FrontendArtifact, Box<dyn Error>> {
    let bit = ValueType::bits(1)?;
    let mut builder = ModuleBuilder::new("NativeConcatWidth")?;
    let clock = builder.input("clock", bit)?;
    let data = builder.input("data", ValueType::bits(data_width)?)?;
    let select = builder.input("select", ValueType::bits(2)?)?;
    let result = builder.output("result", bit)?;
    let bank_width = data_width + 3;
    let bank = builder.internal("bank", ValueType::bits(bank_width)?)?;

    // Match the shape emitted for one atomic Struo FF bank: scalar next-state
    // expressions are concatenated from most-significant to least-significant
    // bit.  The final bit depends on the current bank while every other bit is
    // a direct top-port slice.
    let mut next_bits = Vec::with_capacity(bank_width);
    for lsb in 0..data_width {
        let slice = builder.slice(data, lsb, 1)?;
        next_bits.push(builder.read_slice(slice)?);
    }
    for lsb in 0..2 {
        let slice = builder.slice(select, lsb, 1)?;
        next_bits.push(builder.read_slice(slice)?);
    }
    let data_q0 = builder.read_slice(builder.slice(bank, 0, 1)?)?;
    let select_q0 = builder.read_slice(builder.slice(bank, data_width, 1)?)?;
    next_bits.push(builder.binary(BinaryOp::Xor, data_q0, select_q0, bit)?);
    next_bits.reverse();

    let next = builder.concat(next_bits)?;
    builder.register(builder.whole(bank)?, next, clock, Edge::Posedge, None, None)?;
    let result_q = builder.read_slice(builder.slice(bank, data_width + 2, 1)?)?;
    builder.assign(builder.whole(result)?, result_q)?;

    let artifact = builder.finish();
    artifact.validate()?;
    Ok(artifact)
}

fn main() -> Result<(), Box<dyn Error>> {
    let expect_fixed = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--expect-fixed" => true,
        _ => return Err("usage: native_concat_width [--expect-fixed]".into()),
    };

    let _wasm = Simulator::from_frontend(artifact(129)?).build_wasm()?;
    let _native_control = Simulator::from_frontend(artifact(128)?).build_native()?;
    eprintln!("129-bit data artifact.validate: PASS");
    eprintln!("129-bit data build_wasm: PASS");
    eprintln!("128-bit data build_native control: PASS");

    match (
        expect_fixed,
        Simulator::from_frontend(artifact(129)?).build_native(),
    ) {
        (true, Ok(_)) => {
            eprintln!("129-bit data build_native: PASS (fix verified)");
            Ok(())
        }
        (true, Err(error)) => Err(format!(
            "129-bit data build_native still fails while --expect-fixed was requested: {error}"
        )
        .into()),
        (false, Ok(_)) => {
            Err("129-bit data build_native unexpectedly passed; rerun with --expect-fixed".into())
        }
        (false, Err(error)) => {
            let message = error.to_string();
            if !message.contains("native merged-chain optimization")
                || !message.contains("TYPE.CONCAT_WIDTH")
                || !message.contains("concat operands total 4 bits")
                || !message.contains("width 132")
            {
                return Err(format!("build_native failed differently: {message}").into());
            }
            eprintln!("129-bit data build_native: EXPECTED FAILURE\n{message}");
            Ok(())
        }
    }
}
