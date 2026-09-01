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

const DEFAULT_RUN_TCK: usize = 1_000_000;
const HARDWARE_MEMORY_BYTES: usize = 256 * 1024;
const TRAMPOLINE_WINDOW_BYTES: usize = 4 * 1024;
const FRAME_BYTES: usize = 16;
const FRAME_BITS: usize = FRAME_BYTES * 8;
const PHYSICAL_SCAN_BITS: usize = FRAME_BITS;
const FRAME_MAGIC: u8 = 0xd5;
const FRAME_VERSION: u8 = 1;
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

#[derive(Clone, Copy)]
#[repr(u8)]
enum Opcode {
    Nop = 0,
    Write64 = 1,
    SetTohost = 2,
    Control = 3,
    Select = 4,
}

fn frame(opcode: Opcode, sequence: u8, address: u32, data: u64, mask: u8) -> [u8; 16] {
    let mut frame = [0_u8; FRAME_BYTES];
    frame[0] = FRAME_MAGIC;
    frame[1] = ((opcode as u8) << 4) | FRAME_VERSION;
    frame[2] = sequence;
    frame[3..7].copy_from_slice(&address.to_le_bytes());
    frame[7..15].copy_from_slice(&data.to_le_bytes());
    frame[15] = mask;
    frame
}

trait DirectJtag {
    /// Scan one physical 128-bit protocol frame, least-significant bit first.
    /// `expected_tdo` checks the response selected by the preceding frame.
    fn scan_frame(&mut self, frame: &[u8; FRAME_BYTES], expected_tdo: Option<bool>);
    fn run_test(&mut self, clocks: usize);
}

struct SvfDirect {
    text: String,
}

impl SvfDirect {
    fn new() -> Self {
        Self {
            text: String::from(
                "! Rica RV64 JTAGG ER1 direct hardware test\n\
                 TRST OFF;\n\
                 ENDIR IDLE;\n\
                 ENDDR IDLE;\n\
                 STATE RESET;\n\
                 STATE IDLE;\n\
                 SIR 8 TDI (32);\n",
            ),
        }
    }

    fn scan_hex(frame: &[u8; FRAME_BYTES]) -> String {
        // SVF shifts bit zero first; hexadecimal text is written MSB first.
        let mut encoded = String::with_capacity(PHYSICAL_SCAN_BITS.div_ceil(4));
        for byte in frame.iter().rev() {
            write!(encoded, "{byte:02X}").unwrap();
        }
        encoded
    }
}

impl DirectJtag for SvfDirect {
    fn scan_frame(&mut self, frame: &[u8; FRAME_BYTES], expected_tdo: Option<bool>) {
        let tdi = Self::scan_hex(frame);
        if let Some(expected) = expected_tdo {
            let tdo = if expected {
                "F".repeat(PHYSICAL_SCAN_BITS / 4)
            } else {
                "0".repeat(PHYSICAL_SCAN_BITS.div_ceil(4))
            };
            let mask = "F".repeat(PHYSICAL_SCAN_BITS / 4);
            writeln!(
                self.text,
                "SDR {PHYSICAL_SCAN_BITS} TDI ({tdi}) TDO ({tdo}) MASK ({mask});"
            )
            .unwrap();
        } else {
            writeln!(self.text, "SDR {PHYSICAL_SCAN_BITS} TDI ({tdi});").unwrap();
        }
    }

    fn run_test(&mut self, clocks: usize) {
        writeln!(self.text, "RUNTEST {clocks} TCK;").unwrap();
    }
}

struct XvcDirect {
    tms: Vec<bool>,
    tdi: Vec<bool>,
    checks: Vec<(usize, bool)>,
}

impl XvcDirect {
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

    fn begin_scan_dr(&mut self) -> usize {
        for tms in [true, false, false] {
            self.push(tms, false);
        }
        self.tms.len()
    }

    fn end_scan_dr(&mut self) {
        self.push(true, false);
        self.push(false, false);
    }
}

impl DirectJtag for XvcDirect {
    fn scan_frame(&mut self, frame: &[u8; FRAME_BYTES], expected_tdo: Option<bool>) {
        let start = self.begin_scan_dr();
        for bit in 0..PHYSICAL_SCAN_BITS {
            let value = bit < FRAME_BITS && frame[bit / 8] & (1 << (bit % 8)) != 0;
            self.push(bit + 1 == PHYSICAL_SCAN_BITS, value);
        }
        if let Some(expected) = expected_tdo {
            self.checks
                .extend((start..start + PHYSICAL_SCAN_BITS).map(|bit| (bit, expected)));
        }
        self.end_scan_dr();
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

    fn shift(&mut self, bus: &XvcDirect) -> Result<(), Box<dyn Error>> {
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
                        "XVC TDO mismatch at scan bit {bit} (check {check}/{}, local bit {local}): expected {}, got {}",
                        bus.checks.len(),
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

struct DirectSession<'a, B: DirectJtag> {
    bus: &'a mut B,
    next_sequence: u8,
}

impl<'a, B: DirectJtag> DirectSession<'a, B> {
    fn new(bus: &'a mut B) -> Self {
        Self {
            bus,
            next_sequence: 1,
        }
    }

    fn issue(&mut self, opcode: Opcode, address: u32, data: u64, mask: u8) -> u8 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.bus
            .scan_frame(&frame(opcode, sequence, address, data, mask), None);
        sequence
    }

    fn control(&mut self, value: u64) -> u8 {
        self.issue(Opcode::Control, 0, value, 0)
    }

    fn expect_selected_bits(&mut self, expected: &[(u32, bool)]) {
        let mut pending = None;
        for &(selector, value) in expected {
            let sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.wrapping_add(1);
            let select = frame(Opcode::Select, sequence, selector, 0, 0);
            self.bus.scan_frame(&select, pending);
            pending = Some(value);
        }

        // SELECT takes effect on Update-DR, so the response to the final
        // selector is observed throughout this following scan.
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let nop = frame(Opcode::Nop, sequence, 0, 0, 0);
        self.bus.scan_frame(&nop, pending);
    }
}

fn push_value_bits(expected: &mut Vec<(u32, bool)>, first: u32, value: u64, width: u32) {
    expected.extend((0..width).map(|bit| (first + bit, value & (1_u64 << bit) != 0)));
}

fn emit_test(
    bus: &mut impl DirectJtag,
    image: &ElfImage,
    run_tck: usize,
    expected_tohost: u64,
) -> Result<(), Box<dyn Error>> {
    let (memory, ranges) = memory_image(image)?;
    let mut session = DirectSession::new(bus);

    // Quiesce a preceding test before replacing its instruction image.
    session.control(2);

    for (address_base, start, end) in ranges {
        for offset in (start..end).step_by(8) {
            let chunk_end = end.min(offset + 8);
            let mut bytes = [0_u8; 8];
            bytes[..chunk_end - offset].copy_from_slice(&memory[offset..chunk_end]);
            let valid_bytes = chunk_end - offset;
            let mask = if valid_bytes == 8 {
                u8::MAX
            } else {
                (1_u8 << valid_bytes) - 1
            };
            let address = address_base + u32::try_from(offset - start)?;
            session.issue(Opcode::Write64, address, u64::from_le_bytes(bytes), mask);
        }
    }

    let set_tohost_sequence = session.issue(Opcode::SetTohost, image.tohost as u32, 0, 0);
    let mut load_status = vec![(20, false), (21, false)];
    push_value_bits(&mut load_status, 24, u64::from(set_tohost_sequence), 8);
    session.expect_selected_bits(&load_status);

    // Reassert reset and clear the previous result only after checking that
    // the complete image and tohost address were accepted without error.
    session.control(2);
    let run_sequence = session.control(1);
    session.bus.run_test(run_tck);

    // Each SELECT updates the response mux only after its own scan. Pipeline
    // selectors and check the previous selection on every bit of the next scan.
    let mut expected = Vec::with_capacity(94);
    push_value_bits(&mut expected, 0, 0x52, 8);
    push_value_bits(&mut expected, 8, 0x04, 8);
    expected.extend([
        (16, true),  // PLL locked
        (17, true),  // run remains enabled
        (18, true),  // tohost result captured
        (19, false), // architectural traps do not assert debug halt
        (20, false), // no WRITE64 remains busy
        (21, false), // no malformed/busy/run-time command was rejected
    ]);
    push_value_bits(&mut expected, 24, u64::from(run_sequence), 8);
    push_value_bits(&mut expected, 32, expected_tohost, 64);
    session.expect_selected_bits(&expected);
    Ok(())
}

fn test_svf(
    image: &ElfImage,
    run_tck: usize,
    expected_tohost: u64,
) -> Result<String, Box<dyn Error>> {
    let mut bus = SvfDirect::new();
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
    let mut bitstream = project.join("build/riscv-hardware-test-125-texo.bit");
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
            let mut bus = XvcDirect::new();
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
