use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use verify_riscv::ElfImage;

const WINDOW_START: u8 = 0x20;
const WINDOW_LENGTH: usize = 0xe0;
const COMMAND_SETTLE_TCK: usize = 16;
const DEFAULT_RUN_TCK: usize = 1_000_000;
const HARDWARE_MEMORY_BYTES: usize = 256 * 1024;
const TRAMPOLINE_WINDOW_BYTES: usize = 4 * 1024;
type MemoryRange = (u32, usize, usize);
type HardwareMemoryImage = (Vec<u8>, Vec<MemoryRange>);

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

fn act4_elfs(project: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let enabled_path = project.join("tests/act4-rv64-enabled.txt");
    let elf_dir = project
        .parent()
        .ok_or("riscv directory has no parent")?
        .join("third_party/riscv-arch-test/work/rica-rv64i/elfs/rv64i/I");
    let mut elfs = Vec::new();
    for line in fs::read_to_string(&enabled_path)?.lines() {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let elf = elf_dir.join(name).with_extension("elf");
        if !elf.is_file() {
            return Err(format!(
                "{} enables {name}, but {} does not exist",
                enabled_path.display(),
                elf.display()
            )
            .into());
        }
        elfs.push(elf);
    }
    if elfs.is_empty() {
        return Err(format!("{} enables no ACT4 tests", enabled_path.display()).into());
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
}

trait I2cJtag {
    fn command(&mut self, scl: bool, sda_low: bool);
    fn sample(&mut self, scl: bool, sda_low: bool, expected_high: bool);
    fn run_test(&mut self, clocks: usize);

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
        self.start();
        self.write_byte(0x42 << 1);
        self.write_byte(address);
        for byte in data {
            self.write_byte(*byte);
        }
        self.stop();
    }

    fn expect_registers(&mut self, address: u8, expected: &[(u8, u8)]) {
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

impl I2cJtag for SvfI2c {
    fn command(&mut self, scl: bool, sda_low: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        writeln!(self.text, "SDR 3 TDI ({command:X});").unwrap();
        writeln!(self.text, "RUNTEST {COMMAND_SETTLE_TCK} TCK;").unwrap();
    }

    fn run_test(&mut self, clocks: usize) {
        writeln!(self.text, "RUNTEST {clocks} TCK;").unwrap();
    }

    fn sample(&mut self, scl: bool, sda_low: bool, expected_high: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        let expected = if expected_high { 7 } else { 0 };
        writeln!(
            self.text,
            "SDR 3 TDI ({command:X}) TDO ({expected:X}) MASK (7);"
        )
        .unwrap();
        writeln!(self.text, "RUNTEST {COMMAND_SETTLE_TCK} TCK;").unwrap();
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

struct XvcI2c {
    tms: Vec<bool>,
    tdi: Vec<bool>,
    checks: Vec<(usize, bool)>,
}

impl XvcI2c {
    fn new() -> Self {
        let mut bus = Self {
            tms: Vec::new(),
            tdi: Vec::new(),
            checks: Vec::new(),
        };
        // Enter Test-Logic-Reset, return to Run-Test/Idle, and select ER1.
        for _ in 0..5 {
            bus.push(true, false);
        }
        bus.push(false, false);
        bus.scan_ir(0x32, 8);
        bus
    }

    fn push(&mut self, tms: bool, tdi: bool) {
        self.tms.push(tms);
        self.tdi.push(tdi);
    }

    fn scan_ir(&mut self, value: u8, width: usize) {
        for tms in [true, true, false, false] {
            self.push(tms, false);
        }
        for bit in 0..width {
            self.push(bit + 1 == width, value & (1 << bit) != 0);
        }
        self.push(true, false);
        self.push(false, false);
    }

    fn scan_dr(&mut self, value: u8, width: usize) -> std::ops::Range<usize> {
        for tms in [true, false, false] {
            self.push(tms, false);
        }
        let start = self.tms.len();
        for bit in 0..width {
            self.push(bit + 1 == width, value & (1 << bit) != 0);
        }
        let end = self.tms.len();
        self.push(true, false);
        self.push(false, false);
        start..end
    }
}

impl I2cJtag for XvcI2c {
    fn command(&mut self, scl: bool, sda_low: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        self.scan_dr(command, 3);
        self.run_test(COMMAND_SETTLE_TCK);
    }

    fn sample(&mut self, scl: bool, sda_low: bool, expected_high: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        let bits = self.scan_dr(command, 3);
        self.checks.extend(bits.map(|bit| (bit, expected_high)));
        self.run_test(COMMAND_SETTLE_TCK);
    }

    fn run_test(&mut self, clocks: usize) {
        self.tms.resize(self.tms.len() + clocks, false);
        self.tdi.resize(self.tdi.len() + clocks, false);
    }
}

struct XvcClient {
    stream: TcpStream,
    max_bits: usize,
}

impl XvcClient {
    fn connect(address: &str) -> Result<Self, Box<dyn Error>> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        stream.write_all(b"getinfo:")?;
        let mut response = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte)?;
            response.push(byte[0]);
            if byte[0] == b'\n' || response.len() == 256 {
                break;
            }
        }
        let response = String::from_utf8(response)?;
        let max_bits = response
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| format!("invalid XVC getinfo response: {response:?}"))?
            .1
            .parse::<usize>()?;
        if max_bits == 0 {
            return Err("XVC server reported a zero-bit shift limit".into());
        }

        let requested_period_ns = 167_u32;
        let mut command = Vec::with_capacity(11);
        command.extend_from_slice(b"settck:");
        command.extend_from_slice(&requested_period_ns.to_le_bytes());
        stream.write_all(&command)?;
        let mut actual_period = [0_u8; 4];
        stream.read_exact(&mut actual_period)?;
        let actual_period = u32::from_le_bytes(actual_period);
        println!("XVC {address}: max {max_bits} bits/shift, TCK period {actual_period} ns");
        Ok(Self { stream, max_bits })
    }

    fn pack(bits: &[bool]) -> Vec<u8> {
        let mut packed = vec![0_u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.iter().copied().enumerate() {
            if bit {
                packed[index / 8] |= 1 << (index % 8);
            }
        }
        packed
    }

    fn shift(&mut self, bus: &XvcI2c) -> Result<(), Box<dyn Error>> {
        if bus.tms.len() != bus.tdi.len() {
            return Err("internal XVC TMS/TDI length mismatch".into());
        }
        let mut check = 0_usize;
        for start in (0..bus.tms.len()).step_by(self.max_bits) {
            let end = bus.tms.len().min(start + self.max_bits);
            let bit_count = end - start;
            let tms = Self::pack(&bus.tms[start..end]);
            let tdi = Self::pack(&bus.tdi[start..end]);
            let mut command = Vec::with_capacity(10 + tms.len() + tdi.len());
            command.extend_from_slice(b"shift:");
            command.extend_from_slice(&u32::try_from(bit_count)?.to_le_bytes());
            command.extend_from_slice(&tms);
            command.extend_from_slice(&tdi);
            self.stream.write_all(&command)?;
            let mut tdo = vec![0_u8; bit_count.div_ceil(8)];
            self.stream.read_exact(&mut tdo)?;

            while let Some(&(bit, expected)) = bus.checks.get(check) {
                if bit >= end {
                    break;
                }
                let local = bit - start;
                let actual = tdo[local / 8] & (1 << (local % 8)) != 0;
                if actual != expected {
                    return Err(format!(
                        "XVC TDO mismatch at scan bit {bit}: expected {}, got {}",
                        u8::from(expected),
                        u8::from(actual)
                    )
                    .into());
                }
                check += 1;
            }
        }
        if check != bus.checks.len() {
            return Err("internal XVC check position exceeds generated shifts".into());
        }
        Ok(())
    }
}

fn physical_index(address: u64) -> Result<usize, Box<dyn Error>> {
    if address < TRAMPOLINE_WINDOW_BYTES as u64 {
        return Ok(usize::try_from(address)?);
    }
    let act4_bytes = HARDWARE_MEMORY_BYTES - TRAMPOLINE_WINDOW_BYTES;
    if (0x8000_0000..0x8000_0000 + act4_bytes as u64).contains(&address) {
        return Ok(TRAMPOLINE_WINDOW_BYTES + usize::try_from(address - 0x8000_0000)?);
    }
    Err(format!(
        "address 0x{address:016x} is outside the hardware test RAM windows 0x0..0xfff and 0x80000000..0x8003efff"
    )
    .into())
}

fn memory_image(image: &ElfImage) -> Result<HardwareMemoryImage, Box<dyn Error>> {
    let mut bytes = vec![0_u8; HARDWARE_MEMORY_BYTES];
    let mut maxima = [None::<usize>; 2];
    for (address, byte) in image.load_bytes() {
        let index = physical_index(address)?;
        bytes[index] = byte;
        let window = usize::from(index >= TRAMPOLINE_WINDOW_BYTES);
        maxima[window] = Some(maxima[window].map_or(index, |maximum| maximum.max(index)));
    }
    let mut ranges = Vec::new();
    if let Some(maximum) = maxima[0] {
        ranges.push((0, 0, maximum + 1));
    }
    if let Some(maximum) = maxima[1] {
        ranges.push((0x8000_0000, TRAMPOLINE_WINDOW_BYTES, maximum + 1));
    }
    Ok((bytes, ranges))
}

fn emit_test(
    bus: &mut impl I2cJtag,
    image: &ElfImage,
    run_tck: usize,
    expected_tohost: u64,
) -> Result<(), Box<dyn Error>> {
    let (memory, ranges) = memory_image(image)?;

    bus.expect_registers(0x00, &[(0x52, 0xff), (0x03, 0xff), (0x01, 0xff)]);
    bus.write_registers(0x10, &[0x02]);
    bus.write_registers(0x08, &(image.tohost as u32).to_le_bytes());

    for (address_base, start, end) in ranges {
        for offset in (start..end).step_by(WINDOW_LENGTH) {
            let chunk_end = end.min(offset + WINDOW_LENGTH);
            let aperture = address_base + u32::try_from(offset - start)?;
            bus.write_registers(0x04, &aperture.to_le_bytes());
            bus.write_registers(WINDOW_START, &memory[offset..chunk_end]);
        }
    }

    bus.write_registers(0x10, &[0x01]);
    bus.run_test(run_tck);
    // A self-checking ELF reports through tohost before entering its terminal
    // loop. Architectural ECALL/EBREAK now trap instead of asserting the legacy
    // debug halt bit, so verify lock/run/result-valid/result without requiring
    // a non-RISC-V halt side effect.
    let expected_status = 0x0b | (u8::from(expected_tohost & 1 != 0) << 4);
    bus.expect_registers(0x03, &[(expected_status, 0x1b)]);
    let expected_result = expected_tohost.to_le_bytes().map(|byte| (byte, 0xff));
    bus.expect_registers(0x18, &expected_result);
    Ok(())
}

fn test_svf(
    image: &ElfImage,
    run_tck: usize,
    expected_tohost: u64,
) -> Result<String, Box<dyn Error>> {
    let mut bus = SvfI2c::new();
    emit_test(&mut bus, image, run_tck, expected_tohost)?;
    bus.text.push_str("STATE IDLE;\n");
    Ok(bus.text)
}

fn xvc_host_port(address: &str) -> Result<(&str, u16), Box<dyn Error>> {
    if let Some((host, port)) = address.rsplit_once(':') {
        if host.is_empty() {
            return Err("XVC host is empty".into());
        }
        return Ok((host, port.parse()?));
    }
    Ok((address, 3721))
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut arguments = env::args_os().skip(1);
    let mut run_tck = DEFAULT_RUN_TCK;
    let mut expected_tohost = 1_u64;
    let mut program = true;
    let mut act4 = false;
    let mut xvc_address = None::<String>;
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
        } else if argument == "--act4" {
            act4 = true;
        } else if argument == "--xvc" {
            xvc_address = Some(
                arguments
                    .next()
                    .ok_or("--xvc requires HOST[:PORT]")?
                    .to_str()
                    .ok_or("--xvc address is not valid UTF-8")?
                    .to_owned(),
            );
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
        let mut command = Command::new(&loader);
        if let Some(address) = xvc_address.as_deref() {
            let (host, port) = xvc_host_port(address)?;
            command
                .args(["-c", "xvc-client", "--ip", host, "--port"])
                .arg(port.to_string())
                .arg("--write-sram");
        } else {
            command.args(["-b", "ecp5_evn", "--write-sram"]);
        }
        checked_command(command.arg(&bitstream))?;
        println!("PROGRAM {}", bitstream.display());
    }

    if act4 && !requested_elfs.is_empty() {
        return Err("positional ELF paths cannot be combined with --act4".into());
    }
    let elfs = if act4 {
        act4_elfs(project)?
    } else if requested_elfs.is_empty() {
        bundled_elfs(project)?
    } else {
        requested_elfs
    };
    let output_dir = project.join("build/hardware-rv64");
    fs::create_dir_all(&output_dir)?;
    let mut xvc = xvc_address.as_deref().map(XvcClient::connect).transpose()?;
    for elf in &elfs {
        let image = ElfImage::load(elf)?;
        let name = elf
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or("ELF has no valid filename")?;
        if let Some(xvc) = &mut xvc {
            let mut bus = XvcI2c::new();
            emit_test(&mut bus, &image, run_tck, expected_tohost)?;
            xvc.shift(&bus)?;
        } else {
            let svf = output_dir.join(format!("{name}.svf"));
            fs::write(&svf, test_svf(&image, run_tck, expected_tohost)?)?;
            checked_command(Command::new(&loader).args(["-b", "ecp5_evn"]).arg(&svf))?;
        }
        println!("PASS {name:<24} hardware tohost=0x{expected_tohost:016x}");
    }
    println!("RV64 hardware tests: {}/{} passed", elfs.len(), elfs.len());
    Ok(())
}
