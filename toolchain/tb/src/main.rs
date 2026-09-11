use celox::testbench::{compile_initial_testbench, run_compiled_testbench_with_tick_limit};
use celox::{SimBackend, Simulator, SimulatorBuilder, TestResult};
use std::{error::Error, fs, path::PathBuf};

fn run<B: SimBackend>(mut sim: Simulator<B>) -> Result<(), Box<dyn Error>> {
    let tb = compile_initial_testbench(&sim).ok_or("no native testbench initial block")?;
    let result = run_compiled_testbench_with_tick_limit(&mut sim, &tb, 1_000_000);
    if result.tick_limit_reached || result.result != TestResult::Pass {
        return Err(format!("{result:?}").into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let backend = args.next().unwrap_or_else(|| "native".into());
    if !["native", "cranelift", "wasm", "interpreter"].contains(&backend.as_str()) {
        return Err("usage: rica-tb [native|cranelift|wasm|interpreter] [test-name]".into());
    }
    let filter = args.next().unwrap_or_default();
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let suite = fs::read_to_string(root.join("riscv/tests/suite.txt"))?;
    let mut count = 0;
    for line in suite
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut fields = line.split_whitespace();
        let top = fields.next().ok_or("missing test name")?;
        if !top.contains(&filter) {
            continue;
        }
        let source = fields
            .map(|path| fs::read_to_string(root.join(path)))
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        let builder = SimulatorBuilder::new(&source, top);
        let result = match backend.as_str() {
            "native" => run(builder.build_native()?),
            "cranelift" => run(builder.build_cranelift()?),
            "wasm" => run(builder.build_wasm()?),
            "interpreter" => run(builder.build_interpreter()?),
            _ => unreachable!(),
        };
        result.map_err(|error| format!("{top}: {error}"))?;
        println!("PASS {top} ({backend})");
        count += 1;
    }
    if count == 0 {
        return Err("no matching testbenches".into());
    }
    println!("{count} testbenches passed");
    Ok(())
}
