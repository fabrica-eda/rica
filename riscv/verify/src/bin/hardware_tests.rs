use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use verify_riscv::ElfImage;

const WINDOW_START: u8 = 0x20;
const WINDOW_LENGTH: usize = 0xe0;
const DEFAULT_RUN_TCK: usize = 1_000_000;
type MemoryRange = (usize, usize);
type HardwareMemoryImage = ([u8; 4096], Vec<MemoryRange>);

fn checked_command(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let printable = format!("{command:?}");
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "command failed: {printable}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .into())
}

fn llvm_mc() -> PathBuf {
    env::var_os("LLVM_MC").map_or_else(
        || {
            let versioned = PathBuf::from("/usr/lib/llvm-20/bin/llvm-mc");
            if versioned.is_file() {
                versioned
            } else {
                PathBuf::from("llvm-mc")
            }
        },
        PathBuf::from,
    )
}

fn rust_lld() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = env::var_os("RUST_LLD") {
        return Ok(path.into());
    }
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim());
    let verbose = Command::new("rustc").arg("-vV").output()?;
    let verbose = String::from_utf8(verbose.stdout)?;
    let host = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("rustc -vV did not report a host triple")?;
    Ok(sysroot.join("lib/rustlib").join(host).join("bin/rust-lld"))
}

fn bundled_elfs(project: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let tests = project.join("tests/rv64i-subset");
    let build = project.join("build/rv64i-subset");
    fs::create_dir_all(&build)?;
    let mut sources = fs::read_dir(&tests)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("S")))
        .collect::<Vec<_>>();
    sources.sort();
    let assembler = llvm_mc();
    let linker = rust_lld()?;
    let mut elfs = Vec::new();
    for source in sources {
        let stem = source.file_stem().ok_or("test source has no stem")?;
        let object = build.join(stem).with_extension("o");
        let elf = build.join(stem).with_extension("elf");
        checked_command(
            Command::new(&assembler)
                .arg("-triple=riscv64-unknown-none-elf")
                .arg("-mattr=+i,+zicsr,+zifencei")
                .arg("-filetype=obj")
                .arg("-I")
                .arg(&tests)
                .arg("-o")
                .arg(&object)
                .arg(&source),
        )?;
        checked_command(
            Command::new(&linker)
                .args(["-flavor", "gnu", "-m", "elf64lriscv"])
                .arg("-T")
                .arg(tests.join("link.ld"))
                .arg("-o")
                .arg(&elf)
                .arg(&object),
        )?;
        elfs.push(elf);
    }
    Ok(elfs)
}

struct SvfI2c {
    text: String,
}

impl SvfI2c {
    fn new() -> Self {
        Self {
            text: String::from(
                "! Rica RV64 JTAGG ER1 I2C hardware test\n\
                 TRST OFF;\n\
                 ENDIR IDLE;\n\
                 ENDDR IDLE;\n\
                 STATE RESET;\n\
                 STATE IDLE;\n\
                 SIR 8 TDI (32);\n",
            ),
        }
    }

    fn command(&mut self, scl: bool, sda_low: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        writeln!(self.text, "SDR 3 TDI ({command:X});").unwrap();
        self.text.push_str("RUNTEST 16 TCK;\n");
    }

    fn sample(&mut self, scl: bool, sda_low: bool, expected_high: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        let expected = if expected_high { 7 } else { 0 };
        writeln!(
            self.text,
            "SDR 3 TDI ({command:X}) TDO ({expected:X}) MASK (7);"
        )
        .unwrap();
        self.text.push_str("RUNTEST 16 TCK;\n");
    }

    fn start(&mut self) {
        self.command(true, false);
        self.command(true, true);
        self.command(false, true);
    }

    fn repeated_start(&mut self) {
        self.command(false, false);
        self.command(true, false);
        self.command(true, true);
        self.command(false, true);
    }

    fn stop(&mut self) {
        self.command(false, true);
        self.command(true, true);
        self.command(true, false);
    }

    fn write_bit(&mut self, bit: bool) {
        let drive_low = !bit;
        self.command(false, drive_low);
        self.command(true, drive_low);
        self.command(false, drive_low);
    }

    fn write_byte(&mut self, byte: u8) {
        for bit in (0..8).rev() {
            self.write_bit(byte & (1 << bit) != 0);
        }
        self.command(false, false);
        self.command(true, false);
        self.sample(true, false, false);
        self.command(false, false);
    }

    fn read_bit(&mut self, expected: bool, check: bool) {
        self.command(false, false);
        self.command(true, false);
        if check {
            self.sample(true, false, expected);
        } else {
            self.command(true, false);
        }
        self.command(false, false);
    }

    fn read_byte_masked(&mut self, expected: u8, mask: u8, acknowledge: bool) {
        for bit in (0..8).rev() {
            self.read_bit(expected & (1 << bit) != 0, mask & (1 << bit) != 0);
        }
        self.command(false, acknowledge);
        self.command(true, acknowledge);
        self.command(false, acknowledge);
        self.command(false, false);
    }

    fn write_registers(&mut self, address: u8, data: &[u8]) {
        writeln!(
            self.text,
            "! WRITE register 0x{address:02x}, {} byte(s)",
            data.len()
        )
        .unwrap();
        self.start();
        self.write_byte(0x42 << 1);
        self.write_byte(address);
        for byte in data {
            self.write_byte(*byte);
        }
        self.stop();
    }

    fn expect_registers(&mut self, address: u8, expected: &[(u8, u8)]) {
        writeln!(
            self.text,
            "! READ register 0x{address:02x}, {} byte(s)",
            expected.len()
        )
        .unwrap();
        self.start();
        self.write_byte(0x42 << 1);
        self.write_byte(address);
        self.repeated_start();
        self.write_byte((0x42 << 1) | 1);
        for (index, (value, mask)) in expected.iter().copied().enumerate() {
            self.read_byte_masked(value, mask, index + 1 != expected.len());
        }
        self.stop();
    }
}

fn physical_index(address: u64) -> Result<usize, Box<dyn Error>> {
    let page = address & !0x7ff;
    let offset = usize::try_from(address & 0x7ff)?;
    match page {
        0 => Ok(offset),
        0x8000_0000 => Ok(0x800 | offset),
        _ => Err(format!(
            "address 0x{address:016x} is outside the hardware test RAM windows 0x0..0x7ff and 0x80000000..0x800007ff"
        )
        .into()),
    }
}

fn memory_image(image: &ElfImage) -> Result<HardwareMemoryImage, Box<dyn Error>> {
    let mut bytes = [0_u8; 4096];
    let mut owners = BTreeMap::<usize, u64>::new();
    let mut maxima = [None::<usize>; 2];
    for (address, byte) in image.load_bytes() {
        let index = physical_index(address)?;
        if let Some(previous) = owners.insert(index, address)
            && previous != address
        {
            return Err(format!(
                "hardware RAM alias: 0x{previous:016x} and 0x{address:016x} both map to 0x{index:03x}"
            )
            .into());
        }
        bytes[index] = byte;
        let half = index >> 11;
        maxima[half] = Some(maxima[half].map_or(index, |maximum| maximum.max(index)));
    }
    let ranges = maxima
        .into_iter()
        .enumerate()
        .filter_map(|(half, maximum)| maximum.map(|maximum| (half << 11, maximum + 1)))
        .collect();
    Ok((bytes, ranges))
}

fn test_svf(
    image: &ElfImage,
    run_tck: usize,
    expected_tohost: u64,
) -> Result<String, Box<dyn Error>> {
    let (memory, ranges) = memory_image(image)?;
    let mut bus = SvfI2c::new();

    bus.expect_registers(0x00, &[(0x52, 0xff), (0x03, 0xff), (0x01, 0xff)]);
    bus.write_registers(0x10, &[0x02]);
    bus.write_registers(0x08, &(image.tohost as u32).to_le_bytes());

    for (start, end) in ranges {
        for offset in (start..end).step_by(WINDOW_LENGTH) {
            let chunk_end = end.min(offset + WINDOW_LENGTH);
            bus.write_registers(0x04, &(offset as u32).to_le_bytes());
            bus.write_registers(WINDOW_START, &memory[offset..chunk_end]);
        }
    }

    bus.write_registers(0x10, &[0x01]);
    writeln!(bus.text, "RUNTEST {run_tck} TCK;").unwrap();
    // A self-checking ELF reports through tohost before entering its terminal
    // loop. Architectural ECALL/EBREAK now trap instead of asserting the legacy
    // debug halt bit, so verify lock/run/result-valid/result without requiring
    // a non-RISC-V halt side effect.
    let expected_status = 0x0b | (u8::from(expected_tohost & 1 != 0) << 4);
    bus.expect_registers(0x03, &[(expected_status, 0x1b)]);
    let expected_result = expected_tohost.to_le_bytes().map(|byte| (byte, 0xff));
    bus.expect_registers(0x18, &expected_result);
    bus.text.push_str("STATE IDLE;\n");
    Ok(bus.text)
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut arguments = env::args_os().skip(1);
    let mut run_tck = DEFAULT_RUN_TCK;
    let mut expected_tohost = 1_u64;
    let mut program = true;
    let mut bitstream = project.join("build/riscv-hardware-test-125.bit");
    let mut requested_elfs = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--run-tck" {
            run_tck = arguments
                .next()
                .ok_or("--run-tck requires a value")?
                .to_str()
                .ok_or("--run-tck is not valid UTF-8")?
                .parse()?;
        } else if argument == "--expect-tohost" {
            let value = arguments.next().ok_or("--expect-tohost requires a value")?;
            let value = value.to_str().ok_or("--expect-tohost is not valid UTF-8")?;
            expected_tohost = if let Some(hex) = value.strip_prefix("0x") {
                u64::from_str_radix(hex, 16)?
            } else {
                value.parse()?
            };
        } else if argument == "--no-program" {
            program = false;
        } else if argument == "--bitstream" {
            bitstream = arguments
                .next()
                .ok_or("--bitstream requires a path")?
                .into();
        } else {
            requested_elfs.push(PathBuf::from(argument));
        }
    }

    let loader = env::var_os("OPENFPGALOADER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("openFPGALoader"));
    if program {
        checked_command(
            Command::new(&loader)
                .args(["-b", "ecp5_evn", "--write-sram"])
                .arg(&bitstream),
        )?;
        println!("PROGRAM {}", bitstream.display());
    }

    let elfs = if requested_elfs.is_empty() {
        bundled_elfs(project)?
    } else {
        requested_elfs
    };
    let output_dir = project.join("build/hardware-rv64");
    fs::create_dir_all(&output_dir)?;
    for elf in &elfs {
        let image = ElfImage::load(elf)?;
        let name = elf
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or("ELF has no valid filename")?;
        let svf = output_dir.join(format!("{name}.svf"));
        fs::write(&svf, test_svf(&image, run_tck, expected_tohost)?)?;
        checked_command(Command::new(&loader).args(["-b", "ecp5_evn"]).arg(&svf))?;
        println!("PASS {name:<24} hardware tohost=0x{expected_tohost:016x}");
    }
    println!("RV64 hardware tests: {}/{} passed", elfs.len(), elfs.len());
    Ok(())
}
