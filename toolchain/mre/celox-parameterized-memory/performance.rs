//! Compile once, then compare trusted native images with identical host code.
use std::{error::Error, fs, hint::black_box, path::Path, time::Instant};

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};
use serde_json::json;

const FEEDBACK: &str = r#"
module Top #(param UNUSED: u32 = 0,) (
    clk: input clock, rst_n: input reset_async_low,
    a: output logic<64>, b: output logic<64>,
) {
    always_ff (clk, rst_n) {
        if_reset { a = 64'h13; } else { a = b ^ 64'h9e3779b97f4a7c15; }
    }
    always_ff (clk) {
        if !rst_n { b = 64'h57; } else { b = a + 64'd1; }
    }
}
"#;

fn build(destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let memory = fs::read_to_string(project.join("src/HardwareMemory.veryl"))?.replace(
        "module HardwareMemory (",
        "module HardwareMemory #(param FAST_LSU: u32 = 0,) (",
    );
    let cpu = [
        verify_riscv::core_source(project)?,
        fs::read_to_string(project.join("src/InstructionBuffer.veryl"))?,
        fs::read_to_string(project.join("src/InstructionCache.veryl"))?,
        memory.clone(),
        fs::read_to_string(project.join("src/HardwareCoreTestTop.veryl"))?,
    ]
    .join("\n");
    let mut records = Vec::new();
    for (name, top, source) in [
        ("feedback", "Top", FEEDBACK),
        ("memory", "HardwareMemory", &memory),
        ("cpu", "HardwareCoreTestTop", &cpu),
    ] {
        let output = destination.join(format!("{name}.bin"));
        assert!(!output.exists(), "use a fresh image directory");
        fs::write(destination.join(format!("{name}.veryl")), source)?;
        let mut builder = SimulatorBuilder::new(source, top);
        if name == "cpu" {
            builder = builder
                .param("PREDICTOR_MODE", 4u64)
                .param("ICACHE_INDEX_BITS", 9u64);
        }
        let start = Instant::now();
        let (compiled, trace) = builder.compile_native_with_trace()?;
        let compile_seconds = start.elapsed().as_secs_f64();
        let image = compiled.into_program_image();
        fs::write(&output, image.append_to_runtime(b"celox-ff-performance\n")?)?;
        fs::write(
            destination.join(format!("{name}.sir")),
            trace.native_optimized_sir.unwrap_or_default(),
        )?;
        fs::write(
            destination.join(format!("{name}.mir")),
            trace.mir.unwrap_or_default(),
        )?;
        records.push(json!({"name": name, "compile_seconds": compile_seconds,
            "code_bytes": image.code_image().len(), "state_bytes": image.layout().merged_total_size,
            "entries": image.code_entries()}));
        println!(
            "built {name}: {compile_seconds:.3}s, {} code bytes",
            image.code_image().len()
        );
    }
    fs::write(
        destination.join("build.json"),
        serde_json::to_vec_pretty(&records)?,
    )?;
    Ok(())
}

fn load(path: &Path) -> Result<Simulator<NativeBackend>, Box<dyn Error>> {
    let image = NativeProgramImage::discover_appended(&fs::read(path)?)?
        .ok_or("missing Celox native image")?
        .image;
    // Only load images compiled by this harness in the current trusted workspace.
    Ok(SimulatorBuilder::new("", "").build_native_from_image(image)?)
}

fn initialize(sim: &mut Simulator<NativeBackend>, name: &str) -> Result<(), Box<dyn Error>> {
    let reset = sim.signal("rst_n");
    if name == "feedback" {
        sim.modify(|io| io.set(reset, 0u8))?;
        sim.tick(sim.event("clk"))?;
        sim.modify(|io| io.set(reset, 1u8))?;
    } else if name == "memory" {
        let ports = [
            "debug_write",
            "debug_address",
            "debug_data",
            "cpu_instr_address",
            "cpu_data_address",
            "cpu_data_request_sequence",
            "cpu_data_write",
            "cpu_data_mask",
            "cpu_data_we",
            "cpu_instr_request_sequence",
        ];
        let signals = ports.map(|port| sim.signal(port));
        sim.modify(|io| {
            io.set(reset, 0u8);
            for signal in signals {
                io.set(signal, 0u8);
            }
        })?;
        sim.tick(sim.event("clk"))?;
        sim.modify(|io| io.set(reset, 1u8))?;
    } else {
        let cpu = sim.event("clk_cpu");
        let mem = sim.event("clk_mem");
        let run = sim.signal("run");
        let write = sim.signal("debug_write");
        let address = sim.signal("debug_address");
        let data = sim.signal("debug_data");
        sim.modify(|io| {
            io.set(reset, 0u8);
            io.set(run, 0u8);
            io.set(write, 0u8);
            io.set(address, 0u32);
            io.set(data, 0u8);
        })?;
        sim.tick(cpu)?;
        sim.tick(mem)?;
        sim.modify(|io| io.set(reset, 1u8))?;
        // Repeating sd/ld/add loop. Data at 0x40 is separate from the reset code.
        for (offset, byte) in [
            0x04000093u32,
            0x00100113,
            0x0020b023,
            0x0000b183,
            0x00118113,
            0x00110113,
            0xff1ff06f,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .enumerate()
        {
            sim.modify(|io| {
                io.set(write, 1u8);
                io.set(address, offset as u32);
                io.set(data, byte);
            })?;
            sim.tick(mem)?;
        }
        sim.modify(|io| io.set(write, 0u8))?;
        sim.tick(mem)?;
        sim.modify(|io| io.set(run, 1u8))?;
    }
    Ok(())
}

fn run(
    sim: &mut Simulator<NativeBackend>,
    name: &str,
    iterations: u64,
) -> Result<(f64, Vec<String>), Box<dyn Error>> {
    let start;
    if name == "feedback" {
        let clock = sim.event("clk");
        start = Instant::now();
        for _ in 0..iterations {
            sim.tick(black_box(clock))?;
        }
    } else if name == "memory" {
        let clock = sim.event("clk");
        let write = sim.signal("debug_write");
        let address = sim.signal("debug_address");
        let data = sim.signal("debug_data");
        start = Instant::now();
        for index in 0..iterations {
            sim.modify(|io| {
                io.set(write, 1u8);
                io.set(address, (index & 0x1ffff) as u32 | 0x80000000);
                io.set(data, index as u8);
            })?;
            sim.tick(black_box(clock))?;
        }
    } else {
        let cpu = sim.event("clk_cpu");
        let mem = sim.event("clk_mem");
        start = Instant::now();
        for index in 0..iterations {
            if index & 1 != 0 {
                sim.tick(mem)?;
            }
            sim.tick(black_box(cpu))?;
        }
    }
    let seconds = start.elapsed().as_secs_f64();
    let outputs: &[&str] = match name {
        "feedback" => &["a", "b"],
        "memory" => &[
            "debug_data_q",
            "debug_address_q",
            "cpu_data_read",
            "cpu_instr_line",
        ],
        _ => &[
            "pc",
            "commit_pc",
            "commit_instr",
            "commit_data",
            "data_wdata",
        ],
    };
    let values = outputs
        .iter()
        .map(|name| format!("{:x}", sim.get(sim.signal(name))))
        .collect();
    Ok((seconds, values))
}

fn compare(before: &Path, after: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    assert!(!output.exists(), "use a fresh result path");
    let mut results = Vec::new();
    for (name, iterations) in [
        ("feedback", 3_000_000),
        ("memory", 150_000),
        ("cpu", 150_000),
    ] {
        // Compile and attach outside the measured region. Alternate AB/BA order.
        // Keep only one executable model alive at a time, so the comparison
        // does not require its two JIT images to occupy different address ranges.
        let paths = [before, after];
        for sample in 0..9 {
            let mut observed = [None, None];
            for variant in if sample % 2 == 0 { [0, 1] } else { [1, 0] } {
                let mut sim = load(&paths[variant].join(format!("{name}.bin")))?;
                let event_pointer = format!(
                    "{:?}",
                    sim.event(if name == "cpu" { "clk_cpu" } else { "clk" })
                );
                initialize(&mut sim, name)?;
                run(&mut sim, name, 1024)?;
                let (seconds, values) = run(&mut sim, name, iterations)?;
                observed[variant] = Some(values.clone());
                results.push(json!({"case": name, "variant": variant, "sample": sample, "iterations": iterations,
                    "seconds": seconds, "ns_per_iteration": seconds * 1e9 / iterations as f64, "outputs": values,
                    "event_pointer": event_pointer}));
            }
            assert_eq!(observed[0], observed[1], "{name} sample {sample}");
        }
        println!("compared {name}: matching outputs in all samples");
        fs::write(output, serde_json::to_vec_pretty(&results)?)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [mode, directory] if mode == "build" => build(Path::new(directory)),
        [mode, before, after, output] if mode == "compare" => {
            compare(Path::new(before), Path::new(after), Path::new(output))
        }
        _ => Err("usage: celox-ff-performance build DIR | compare BEFORE AFTER OUTPUT.json".into()),
    }
}
