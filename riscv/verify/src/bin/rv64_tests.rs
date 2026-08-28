use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use verify_riscv::{ElfImage, core_source, run_elf};

const DEFAULT_MAX_CYCLES: usize = 20_000;

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
                .arg("-mattr=+i")
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

fn read_enabled_tests(path: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let mut enabled = BTreeSet::new();
    for (index, line) in contents.lines().enumerate() {
        let name = line.split('#').next().unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        if !name.starts_with("I-") || !name.ends_with("-00") {
            return Err(format!(
                "{}:{}: expected an ACT4 RV64I test stem such as I-add-00",
                path.display(),
                index + 1
            )
            .into());
        }
        if !enabled.insert(name.to_owned()) {
            return Err(format!("{}:{}: duplicate test {name}", path.display(), index + 1).into());
        }
    }
    Ok(enabled)
}

fn act4_catalog(source_dir: &Path) -> Result<BTreeMap<String, PathBuf>, Box<dyn Error>> {
    if !source_dir.is_dir() {
        return Err(format!(
            "ACT4 source catalog {} is missing; run `git submodule update --init`",
            source_dir.display()
        )
        .into());
    }
    let mut catalog = BTreeMap::new();
    for entry in fs::read_dir(source_dir)? {
        let path = entry?.path();
        if path.extension() != Some(OsStr::new("S")) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("invalid ACT4 test filename: {}", path.display()))?;
        if stem.starts_with("I-") && stem.ends_with("-00") {
            catalog.insert(stem.to_owned(), path);
        }
    }
    if catalog.is_empty() {
        return Err(format!("no ACT4 RV64I tests found in {}", source_dir.display()).into());
    }
    Ok(catalog)
}

fn collect_elfs(
    directory: &Path,
    output: &mut BTreeMap<String, PathBuf>,
) -> Result<(), Box<dyn Error>> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_elfs(&path, output)?;
        } else if path.extension() == Some(OsStr::new("elf")) {
            let stem = path
                .file_stem()
                .and_then(OsStr::to_str)
                .ok_or_else(|| format!("invalid ELF filename: {}", path.display()))?;
            if let Some(previous) = output.insert(stem.to_owned(), path.clone()) {
                return Err(format!(
                    "duplicate ACT4 ELF stem {stem}: {} and {}",
                    previous.display(),
                    path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn run_act4(
    project: &Path,
    elf_dir: &Path,
    enabled_file: &Path,
    max_cycles: usize,
) -> Result<(), Box<dyn Error>> {
    let repository = project.parent().ok_or("riscv directory has no parent")?;
    let source_dir = repository.join("third_party/riscv-arch-test/tests/rv64i/I");
    let catalog = act4_catalog(&source_dir)?;
    let enabled = read_enabled_tests(enabled_file)?;
    let available = catalog.keys().cloned().collect::<BTreeSet<_>>();
    let unknown = enabled.difference(&available).cloned().collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(format!(
            "{} enables tests absent from the pinned ACT4 catalog: {}",
            enabled_file.display(),
            unknown.join(", ")
        )
        .into());
    }

    let mut elfs = BTreeMap::new();
    collect_elfs(elf_dir, &mut elfs)?;
    let source = (!enabled.is_empty())
        .then(|| core_source(project))
        .transpose()?;
    let mut passed = 0;
    let mut ignored = 0;
    let mut failures = Vec::new();

    for name in catalog.keys() {
        if !enabled.contains(name) {
            println!("IGNORE {name:<24} not enabled for Rica yet");
            ignored += 1;
            continue;
        }
        let Some(elf) = elfs.get(name) else {
            failures.push(format!(
                "{name}: enabled but {} contains no {name}.elf",
                elf_dir.display()
            ));
            continue;
        };
        let image = ElfImage::load(elf);
        let result = image.and_then(|image| {
            run_elf(
                source.as_deref().expect("source exists for enabled tests"),
                image,
                max_cycles,
            )
        });
        match result {
            Ok(result) => {
                println!(
                    "PASS   {name:<24} {:>7} cycles, {:>7} commits",
                    result.cycles, result.commits
                );
                passed += 1;
            }
            Err(error) => {
                println!("FAIL   {name:<24} {error}");
                failures.push(format!("{name}: {error}"));
            }
        }
    }

    println!(
        "ACT4 RV64I: {passed} passed, {ignored} ignored, {} failed ({} total)",
        failures.len(),
        catalog.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("ACT4 failures:\n  {}", failures.join("\n  ")).into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut arguments = env::args_os().skip(1);
    let mut max_cycles = DEFAULT_MAX_CYCLES;
    let mut act4 = false;
    let mut act4_elf_dir = project
        .parent()
        .unwrap()
        .join("third_party/riscv-arch-test/work/rica-rv64i/elfs");
    let mut act4_enabled = project.join("tests/act4-rv64-enabled.txt");
    let mut requested_elfs = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--max-cycles" {
            let value = arguments.next().ok_or("--max-cycles requires a value")?;
            max_cycles = value
                .to_str()
                .ok_or("--max-cycles is not valid UTF-8")?
                .parse()?;
        } else if argument == "--act4" {
            act4 = true;
        } else if argument == "--act4-elf-dir" {
            act4_elf_dir = arguments
                .next()
                .ok_or("--act4-elf-dir requires a value")?
                .into();
        } else if argument == "--act4-enabled" {
            act4_enabled = arguments
                .next()
                .ok_or("--act4-enabled requires a value")?
                .into();
        } else {
            requested_elfs.push(PathBuf::from(argument));
        }
    }
    if act4 {
        if !requested_elfs.is_empty() {
            return Err("positional ELF paths cannot be combined with --act4".into());
        }
        return run_act4(project, &act4_elf_dir, &act4_enabled, max_cycles);
    }
    let elfs = if requested_elfs.is_empty() {
        bundled_elfs(project)?
    } else {
        requested_elfs
    };
    if elfs.is_empty() {
        return Err("no RV64 ELF tests were supplied".into());
    }
    let source = core_source(project)?;
    for elf in &elfs {
        let result = run_elf(&source, ElfImage::load(elf)?, max_cycles)?;
        println!(
            "PASS {:<24} {:>5} cycles, {:>5} commits",
            elf.file_name().unwrap_or_default().to_string_lossy(),
            result.cycles,
            result.commits
        );
    }
    println!("RV64 ELF tests: {}/{} passed", elfs.len(), elfs.len());
    Ok(())
}
