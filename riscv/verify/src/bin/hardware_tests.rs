use std::collections::BTreeSet;
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

#[derive(Clone, Copy)]
enum Act4Suite {
    I,
    M,
    Zca,
}

impl Act4Suite {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value.to_ascii_lowercase().as_str() {
            "i" => Ok(Self::I),
            "m" => Ok(Self::M),
            "zca" => Ok(Self::Zca),
            _ => Err(format!("unsupported ACT4 suite {value:?}; expected I, M or Zca").into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::I => "I",
            Self::M => "M",
            Self::Zca => "Zca",
        }
    }

    fn enabled_file(self) -> &'static str {
        match self {
            Self::I => "act4-rv64-enabled.txt",
            Self::M => "act4-rv64-m-enabled.txt",
            Self::Zca => "act4-rv64-zca-enabled.txt",
        }
    }
}

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
                .arg("-mattr=+i,+c,+zicsr,+zifencei")
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

fn act4_elf_path(directory: &Path, name: &str) -> PathBuf {
    // ACT4 test stems contain an instruction-name dot (for example,
    // `Zca-c.add-00`). `Path::with_extension` would replace `.add-00`
    // and silently look for `Zca-c.elf`; append the ELF suffix instead.
    directory.join(format!("{name}.elf"))
}

fn act4_elfs(project: &Path, suite: Act4Suite) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let enabled_path = project.join("tests").join(suite.enabled_file());
    let repository = project.parent().ok_or("riscv directory has no parent")?;
    let source_dir = repository
        .join("third_party/riscv-arch-test/tests/rv64i")
        .join(suite.name());
    let elf_dir = project
        .parent()
        .ok_or("riscv directory has no parent")?
        .join(if matches!(suite, Act4Suite::M) {
            "third_party/riscv-arch-test/work/rica-rv64im/elfs/rv64i"
        } else {
            "third_party/riscv-arch-test/work/rica-rv64i/elfs/rv64i"
        })
        .join(suite.name());
    if !source_dir.is_dir() {
        return Err(format!(
            "ACT4 source catalog {} is missing; run `git submodule update --init`",
            source_dir.display()
        )
        .into());
    }

    let prefix = format!("{}-", suite.name());
    let catalog = fs::read_dir(&source_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("S")))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?;
            (stem.starts_with(&prefix) && stem.ends_with("-00")).then(|| stem.to_owned())
        })
        .collect::<BTreeSet<_>>();
    if catalog.is_empty() {
        return Err(format!(
            "no ACT4 RV64{} tests found in {}",
            suite.name(),
            source_dir.display()
        )
        .into());
    }

    let mut enabled = BTreeSet::new();
    for (index, line) in fs::read_to_string(&enabled_path)?.lines().enumerate() {
        let name = line.split('#').next().unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        if !name.starts_with(&prefix) || !name.ends_with("-00") {
            return Err(format!(
                "{}:{}: expected an ACT4 RV64{} test stem beginning with {prefix}",
                enabled_path.display(),
                index + 1,
                suite.name(),
            )
            .into());
        }
        if !enabled.insert(name.to_owned()) {
            return Err(format!(
                "{}:{}: duplicate test {name}",
                enabled_path.display(),
                index + 1
            )
            .into());
        }
    }
    let unknown = enabled.difference(&catalog).cloned().collect::<Vec<_>>();
    let missing = catalog.difference(&enabled).cloned().collect::<Vec<_>>();
    if !unknown.is_empty() || !missing.is_empty() {
        return Err(format!(
            "{} does not exactly match the pinned ACT4 RV64{} catalog: unknown=[{}], missing=[{}]",
            enabled_path.display(),
            suite.name(),
            unknown.join(", "),
            missing.join(", "),
        )
        .into());
    }

    let mut elfs = Vec::new();
    for name in enabled {
        let elf = act4_elf_path(&elf_dir, &name);
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
    /// Scan a frame and retain the stable response bit without comparing it.
    /// Backends such as SVF that cannot return TDO may simply emit the scan.
    fn observe_frame(&mut self, frame: &[u8; FRAME_BYTES]);
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

    fn observe_frame(&mut self, frame: &[u8; FRAME_BYTES]) {
        self.scan_frame(frame, None);
    }

    fn run_test(&mut self, clocks: usize) {
        writeln!(self.text, "RUNTEST {clocks} TCK;").unwrap();
    }
}

struct XvcDirect {
    tms: Vec<bool>,
    tdi: Vec<bool>,
    checks: Vec<(usize, bool)>,
    observations: Vec<(usize, usize)>,
}

impl XvcDirect {
    fn new() -> Self {
        let mut bus = Self {
            tms: Vec::new(),
            tdi: Vec::new(),
            checks: Vec::new(),
            observations: Vec::new(),
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

    fn observe_frame(&mut self, frame: &[u8; FRAME_BYTES]) {
        let start = self.begin_scan_dr();
        for bit in 0..PHYSICAL_SCAN_BITS {
            let value = bit < FRAME_BITS && frame[bit / 8] & (1 << (bit % 8)) != 0;
            self.push(bit + 1 == PHYSICAL_SCAN_BITS, value);
        }
        self.observations.push((start, start + PHYSICAL_SCAN_BITS));
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

#[derive(Clone, Copy)]
struct XvcResponseObservation {
    value: bool,
    one_count: usize,
}

impl XvcResponseObservation {
    fn from_bits(bits: &[bool]) -> Self {
        let one_count = bits.iter().filter(|&&bit| bit).count();
        // A selector response is held for the whole 128-bit scan.  Majority
        // voting keeps the decoded fields useful if a transport/timing glitch
        // affects only a few samples; the non-uniform response is still
        // reported separately and still fails the test.
        let value = if one_count * 2 == bits.len() {
            bits.first().copied().unwrap_or(false)
        } else {
            one_count * 2 > bits.len()
        };
        Self { value, one_count }
    }

    fn is_uniform(self) -> bool {
        self.one_count == 0 || self.one_count == PHYSICAL_SCAN_BITS
    }
}

fn response_value(responses: &[XvcResponseObservation], first: usize, width: usize) -> u64 {
    responses[first..first + width]
        .iter()
        .enumerate()
        .fold(0_u64, |value, (bit, response)| {
            value | (u64::from(response.value) << bit)
        })
}

const METRIC_NAMES: [&str; 11] = [
    "mcycle",
    "minstret",
    "dependency",
    "memory",
    "redirect",
    "frontend",
    "conditional",
    "direction_wrong",
    "btb_miss",
    "ras_fallback",
    "repair",
];

fn metric_values(observations: &[XvcResponseObservation]) -> Result<[u64; 11], Box<dyn Error>> {
    if observations.len() != METRIC_NAMES.len() * 64 {
        return Err(format!(
            "internal XVC metric response count is {}, expected {}",
            observations.len(),
            METRIC_NAMES.len() * 64
        )
        .into());
    }
    Ok(std::array::from_fn(|counter| {
        response_value(observations, counter * 64, 64)
    }))
}

fn format_metrics(values: [u64; 11]) -> String {
    let mut output = METRIC_NAMES
        .into_iter()
        .zip(values)
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    if values[1] != 0 {
        write!(output, " CPI={:.3}", values[0] as f64 / values[1] as f64).unwrap();
    }
    output
}

fn validate_metrics(name: &str, values: [u64; 11]) -> Result<(), Box<dyn Error>> {
    if values[0] == 0 || values[1] == 0 || values[0] <= values[1] {
        return Err(format!(
            "{name}: invalid architectural counters: mcycle={} minstret={}",
            values[0], values[1]
        )
        .into());
    }
    if values[2..].iter().any(|&event| event > values[0])
        || values[2..6]
            .iter()
            .map(|&event| u128::from(event))
            .sum::<u128>()
            > u128::from(values[0])
    {
        return Err(format!(
            "{name}: implementation event counters are inconsistent with mcycle: {}",
            format_metrics(values)
        )
        .into());
    }
    // Predictor events can overlap each other and the four cycle classes, so
    // do not add them into the cycle-class sum. The dedicated performance ELF
    // deliberately overwrites every counter with a unique readback signature;
    // semantic inequalities only apply to naturally accumulated runs.
    if name != "performance"
        && (values[6] > values[1]
            || values[7] > values[6]
            || values[8] > values[1]
            || values[9] > values[1]
            || values[10] > values[4])
    {
        return Err(format!(
            "{name}: predictor event counters are inconsistent: {}",
            format_metrics(values)
        )
        .into());
    }
    if name == "performance" {
        let high_halves = values.map(|value| value >> 32);
        let expected = [16, 8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        if high_halves != expected {
            return Err(format!(
                "{name}: selector/high-bank readback mismatch: high halves {high_halves:?}, expected {expected:?}"
            )
            .into());
        }
    }
    Ok(())
}

fn response_name(group: usize, load_response_count: usize) -> String {
    if group < load_response_count {
        format!("load[{group}]")
    } else {
        format!("result[{}]", group - load_response_count)
    }
}

fn xvc_mismatch_report(checks: &[(usize, bool)], actual: &[bool]) -> Option<String> {
    if checks.len() != actual.len() {
        return Some(format!(
            "internal XVC result length mismatch: collected {} of {} checked bits",
            actual.len(),
            checks.len()
        ));
    }

    let mismatch_indices = checks
        .iter()
        .zip(actual)
        .enumerate()
        .filter_map(|(index, ((_, expected), actual))| (expected != actual).then_some(index))
        .collect::<Vec<_>>();
    let &first_mismatch = mismatch_indices.first()?;
    let response_count = checks.len().div_ceil(PHYSICAL_SCAN_BITS);
    let responses = actual
        .chunks(PHYSICAL_SCAN_BITS)
        .map(XvcResponseObservation::from_bits)
        .collect::<Vec<_>>();
    let mut mismatched_responses = vec![false; response_count];
    for &index in &mismatch_indices {
        mismatched_responses[index / PHYSICAL_SCAN_BITS] = true;
    }
    let mismatched_response_count = mismatched_responses
        .iter()
        .filter(|&&mismatch| mismatch)
        .count();
    let (scan_bit, expected) = checks[first_mismatch];
    let mut report = format!(
        "XVC TDO mismatch: {}/{} checked bits across {}/{} selector-response groups; first at scan bit {scan_bit} (check {first_mismatch}/{}): expected {}, got {}",
        mismatch_indices.len(),
        checks.len(),
        mismatched_response_count,
        response_count,
        checks.len(),
        u8::from(expected),
        u8::from(actual[first_mismatch]),
    );

    const LOAD_RESPONSE_COUNT: usize = 10;
    const RESULT_RESPONSE_COUNT: usize = 94;
    if checks.len() == (LOAD_RESPONSE_COUNT + RESULT_RESPONSE_COUNT) * PHYSICAL_SCAN_BITS {
        let load = &responses[..LOAD_RESPONSE_COUNT];
        let result = &responses[LOAD_RESPONSE_COUNT..];
        let load_sequence = response_value(load, 2, 8);
        let magic = response_value(result, 0, 8);
        let version = response_value(result, 8, 8);
        let flags = response_value(result, 16, 6);
        let run_sequence = response_value(result, 22, 8);
        // emit_test appends the 64 tohost response bits after 30 preceding
        // result selectors, so result[30..=93] is the final actual tohost.
        let actual_tohost = response_value(result, 30, 64);
        write!(
            report,
            "\ndecoded actual responses: load(write_busy={}, rejected={}, sequence=0x{load_sequence:02x}) result(magic=0x{magic:02x}, version=0x{version:02x}, flags=0x{flags:02x}, sequence=0x{run_sequence:02x}, tohost=0x{actual_tohost:016x})",
            u8::from(load[0].value),
            u8::from(load[1].value),
        )
        .unwrap();

        let mismatched = mismatched_responses
            .iter()
            .enumerate()
            .filter_map(|(group, &mismatch)| mismatch.then_some(group))
            .collect::<Vec<_>>();
        report.push_str("\nmismatched responses: ");
        for (position, &group) in mismatched.iter().take(8).enumerate() {
            if position != 0 {
                report.push_str(", ");
            }
            let expected = checks[group * PHYSICAL_SCAN_BITS].1;
            let observation = responses[group];
            write!(
                report,
                "{}={} (expected {})",
                response_name(group, LOAD_RESPONSE_COUNT),
                u8::from(observation.value),
                u8::from(expected),
            )
            .unwrap();
        }
        if mismatched.len() > 8 {
            write!(report, ", ... (+{} more)", mismatched.len() - 8).unwrap();
        }

        let nonuniform = responses
            .iter()
            .enumerate()
            .filter(|(_, observation)| !observation.is_uniform())
            .collect::<Vec<_>>();
        if !nonuniform.is_empty() {
            report.push_str("\nnon-uniform responses (decoded by majority): ");
            for (position, &(group, observation)) in nonuniform.iter().take(8).enumerate() {
                if position != 0 {
                    report.push_str(", ");
                }
                write!(
                    report,
                    "{}={}/{} ones",
                    response_name(group, LOAD_RESPONSE_COUNT),
                    observation.one_count,
                    PHYSICAL_SCAN_BITS,
                )
                .unwrap();
            }
            if nonuniform.len() > 8 {
                write!(report, ", ... (+{} more)", nonuniform.len() - 8).unwrap();
            }
        }
    }

    Some(report)
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

        // Request the integer period immediately below 6 MHz. The XVC server
        // chooses a clock no faster than requested and reports the FT2232H's
        // actual 167 ns period, without cascading down to 5 MHz.
        let requested_period_ns = 166_u32;
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

    fn shift(&mut self, bus: &XvcDirect) -> Result<Vec<XvcResponseObservation>, Box<dyn Error>> {
        if bus.tms.len() != bus.tdi.len() {
            return Err("internal XVC TMS/TDI length mismatch".into());
        }
        let mut check = 0_usize;
        let mut actual_checks = Vec::with_capacity(bus.checks.len());
        let mut actual_observations = vec![Vec::new(); bus.observations.len()];
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

            while let Some(&(bit, _expected)) = bus.checks.get(check) {
                if bit >= end {
                    break;
                }
                let local = bit - start;
                let actual = tdo[local / 8] & (1 << (local % 8)) != 0;
                actual_checks.push(actual);
                check += 1;
            }
            for (observation, &(observation_start, observation_end)) in
                bus.observations.iter().enumerate()
            {
                let overlap_start = start.max(observation_start);
                let overlap_end = end.min(observation_end);
                for bit in overlap_start..overlap_end {
                    let local = bit - start;
                    actual_observations[observation].push(tdo[local / 8] & (1 << (local % 8)) != 0);
                }
            }
        }
        if check != bus.checks.len() {
            return Err("internal XVC check position exceeds generated shifts".into());
        }
        if let Some(report) = xvc_mismatch_report(&bus.checks, &actual_checks) {
            return Err(report.into());
        }
        actual_observations
            .iter()
            .enumerate()
            .map(|(index, bits)| {
                if bits.len() != PHYSICAL_SCAN_BITS {
                    return Err(format!(
                        "internal XVC observation {index} collected {} of {PHYSICAL_SCAN_BITS} bits",
                        bits.len()
                    )
                    .into());
                }
                let observation = XvcResponseObservation::from_bits(bits);
                if !observation.is_uniform() {
                    return Err(format!(
                        "non-uniform XVC metric response {index}: {}/{} samples were one",
                        observation.one_count, PHYSICAL_SCAN_BITS
                    )
                    .into());
                }
                Ok(observation)
            })
            .collect()
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

    fn observe_selected_bits(&mut self, selectors: &[u32]) {
        let mut response_pending = false;
        for &selector in selectors {
            let sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.wrapping_add(1);
            let select = frame(Opcode::Select, sequence, selector, 0, 0);
            if response_pending {
                self.bus.observe_frame(&select);
            } else {
                self.bus.scan_frame(&select, None);
                response_pending = true;
            }
        }

        if response_pending {
            let sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.wrapping_add(1);
            let nop = frame(Opcode::Nop, sequence, 0, 0, 0);
            self.bus.observe_frame(&nop);
        }
    }
}

fn push_value_bits(expected: &mut Vec<(u32, bool)>, first: u32, value: u64, width: u32) {
    expected.extend((0..width).map(|bit| (first + bit, value & (1_u64 << bit) != 0)));
}

fn emit_test(
    bus: &mut impl DirectJtag,
    image: &ElfImage,
    run_tck: usize,
    write_bytes: usize,
    expected_tohost: u64,
    observe_metrics: bool,
) -> Result<(), Box<dyn Error>> {
    let (memory, ranges) = memory_image(image)?;
    let mut session = DirectSession::new(bus);

    // Quiesce a preceding test before replacing its instruction image.
    session.control(2);

    for (address_base, start, end) in ranges {
        for offset in (start..end).step_by(write_bytes) {
            let chunk_end = end.min(offset + write_bytes);
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
    push_value_bits(&mut expected, 8, 0x05, 8);
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
    if observe_metrics {
        // SELECT address[11:8] chooses one frozen Core counter while the low
        // byte retains the existing response-bank/bit selector. Banks five
        // and six expose the low and high halves, respectively.
        let mut selectors = Vec::with_capacity(11 * 64);
        for counter in 0_u32..11 {
            selectors.extend((0_u32..32).map(|bit| (counter << 8) | (5 << 5) | bit));
            selectors.extend((0_u32..32).map(|bit| (counter << 8) | (6 << 5) | bit));
        }
        session.observe_selected_bits(&selectors);
    }
    Ok(())
}

fn test_svf(
    image: &ElfImage,
    run_tck: usize,
    write_bytes: usize,
    expected_tohost: u64,
) -> Result<String, Box<dyn Error>> {
    let mut bus = SvfDirect::new();
    emit_test(
        &mut bus,
        image,
        run_tck,
        write_bytes,
        expected_tohost,
        false,
    )?;
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
    // The falling-edge JTAG image passed all 100 physical ELFs and all 48
    // former WRITE64 corruption probes with eight-byte masked writes.
    let mut write_bytes = 8_usize;
    let mut expected_tohost = 1_u64;
    let mut program = true;
    let mut act4 = false;
    let mut act4_suite = Act4Suite::I;
    let mut xvc_address = None::<String>;
    // CPU/memory stay at 124.8/62.4 MHz. This DSP multiplier image passed
    // STA with 200 ps setup/hold reserves and RV64_HOLD_MARGIN_PLAN.md qualification.
    let mut bitstream =
        project.join("build/riscv-hardware-compact-dsp-m-4k-124p8-20260908-hold.bit");
    let mut requested_elfs = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--run-tck" {
            run_tck = arguments
                .next()
                .ok_or("--run-tck requires a value")?
                .to_str()
                .ok_or("--run-tck is not valid UTF-8")?
                .parse()?;
        } else if argument == "--write-bytes" {
            write_bytes = arguments
                .next()
                .ok_or("--write-bytes requires 1, 2, 4, or 8")?
                .to_str()
                .ok_or("--write-bytes is not valid UTF-8")?
                .parse()?;
            if ![1, 2, 4, 8].contains(&write_bytes) {
                return Err("--write-bytes requires 1, 2, 4, or 8".into());
            }
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
        } else if argument == "--act4-suite" {
            let value = arguments
                .next()
                .ok_or("--act4-suite requires I, M or Zca")?;
            act4_suite =
                Act4Suite::parse(value.to_str().ok_or("--act4-suite is not valid UTF-8")?)?;
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
        act4_elfs(project, act4_suite)?
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
            emit_test(
                &mut bus,
                &image,
                run_tck,
                write_bytes,
                expected_tohost,
                true,
            )?;
            let metrics = metric_values(&xvc.shift(&bus)?)?;
            validate_metrics(name, metrics)?;
            println!("METRIC {name:<22} {}", format_metrics(metrics));
        } else {
            let svf = output_dir.join(format!("{name}.svf"));
            fs::write(
                &svf,
                test_svf(&image, run_tck, write_bytes, expected_tohost)?,
            )?;
            checked_command(Command::new(&loader).args(["-b", "ecp5_evn"]).arg(&svf))?;
        }
        println!("PASS {name:<24} hardware tohost=0x{expected_tohost:016x}");
    }
    println!("RV64 hardware tests: {}/{} passed", elfs.len(), elfs.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOAD_RESPONSES: usize = 10;
    const RESULT_RESPONSES: usize = 94;

    fn set_value(responses: &mut [bool], first: usize, width: usize, value: u64) {
        for bit in 0..width {
            responses[first + bit] = value & (1_u64 << bit) != 0;
        }
    }

    fn standard_responses(tohost: u64) -> Vec<bool> {
        let mut responses = vec![false; LOAD_RESPONSES + RESULT_RESPONSES];
        set_value(&mut responses, 2, 8, 0xa5);
        let result = &mut responses[LOAD_RESPONSES..];
        set_value(result, 0, 8, 0x52);
        set_value(result, 8, 8, 0x05);
        set_value(result, 16, 6, 0x07);
        set_value(result, 22, 8, 0x5a);
        set_value(result, 30, 64, tohost);
        responses
    }

    fn expand_responses(responses: &[bool]) -> Vec<bool> {
        responses
            .iter()
            .flat_map(|&value| std::iter::repeat_n(value, PHYSICAL_SCAN_BITS))
            .collect()
    }

    #[test]
    fn act4_dotted_stem_keeps_the_instruction_name() {
        assert_eq!(
            act4_elf_path(Path::new("elfs/Zca"), "Zca-c.add-00"),
            PathBuf::from("elfs/Zca/Zca-c.add-00.elf")
        );
    }

    #[test]
    fn matching_xvc_responses_produce_no_report() {
        let actual = expand_responses(&standard_responses(1));
        let checks = actual.iter().copied().enumerate().collect::<Vec<_>>();
        assert!(xvc_mismatch_report(&checks, &actual).is_none());
    }

    #[test]
    fn standard_xvc_report_decodes_actual_tohost() {
        let expected = expand_responses(&standard_responses(1));
        let actual = expand_responses(&standard_responses(3));
        let checks = expected.iter().copied().enumerate().collect::<Vec<_>>();
        let report = xvc_mismatch_report(&checks, &actual).unwrap();

        assert!(report.contains("128/13312 checked bits across 1/104"));
        assert!(report.contains("load(write_busy=0, rejected=0, sequence=0xa5)"));
        assert!(report.contains("magic=0x52, version=0x05, flags=0x07, sequence=0x5a"));
        assert!(report.contains("tohost=0x0000000000000003"));
        assert!(report.contains("result[31]=1 (expected 0)"));
    }

    #[test]
    fn nonuniform_xvc_response_is_reported_and_still_fails() {
        let expected = expand_responses(&standard_responses(1));
        let mut actual = expected.clone();
        let result_bit_one_group = LOAD_RESPONSES + 31;
        actual[result_bit_one_group * PHYSICAL_SCAN_BITS] = true;
        let checks = expected.iter().copied().enumerate().collect::<Vec<_>>();
        let report = xvc_mismatch_report(&checks, &actual).unwrap();

        assert!(report.contains("non-uniform responses (decoded by majority)"));
        assert!(report.contains("result[31]=1/128 ones"));
        assert!(report.contains("tohost=0x0000000000000001"));
    }

    #[test]
    fn metric_responses_decode_eleven_little_endian_counters() {
        let expected = [
            0x0123_4567_89ab_cdef,
            0xfedc_ba98_7654_3210,
            3,
            5,
            8,
            13,
            21,
            34,
            55,
            89,
            144,
        ];
        let mut responses = Vec::new();
        for value in expected {
            responses.extend((0..64).map(|bit| {
                let value = value & (1_u64 << bit) != 0;
                XvcResponseObservation {
                    value,
                    one_count: if value { PHYSICAL_SCAN_BITS } else { 0 },
                }
            }));
        }
        assert_eq!(metric_values(&responses).unwrap(), expected);
        assert!(format_metrics(expected).contains("CPI="));
    }

    #[test]
    fn select_frame_preserves_four_bit_counter_selector() {
        let selector = (10_u32 << 8) | (6 << 5) | 31;
        let encoded = frame(Opcode::Select, 0x5a, selector, 0, 0);
        assert_eq!(
            u32::from_le_bytes(encoded[3..7].try_into().unwrap()),
            selector
        );
        assert_eq!(encoded[4] & 0x0f, 10);
    }

    #[test]
    fn physical_metric_validation_checks_selectors_and_high_bank() {
        let seeded = [
            0x0000_0010_0000_0080,
            0x0000_0008_0000_0068,
            0x0000_0001_0000_0011,
            0x0000_0002_0000_0022,
            0x0000_0003_0000_0033,
            0x0000_0004_0000_0044,
            0x0000_0005_0000_0050,
            0x0000_0006_0000_0060,
            0x0000_0007_0000_0070,
            0x0000_0008_0000_0080,
            0x0000_0009_0000_0090,
        ];
        assert!(validate_metrics("performance", seeded).is_ok());

        let mut broken_high_bank = seeded;
        broken_high_bank[10] &= 0xffff_ffff;
        assert!(validate_metrics("performance", broken_high_bank).is_err());

        let aliased = [100, 50, 10, 10, 10, 100, 0, 0, 0, 0, 0];
        assert!(validate_metrics("other", aliased).is_err());
    }
}
