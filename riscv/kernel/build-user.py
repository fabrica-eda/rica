#!/usr/bin/env python3
"""Build and audit the U/PMP kernel, preserving qualified M-mode ELF inputs."""
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

from build import HERE, ROOT, TOOLCHAIN, OBJDUMP, audit


def main():
    out = HERE.parent / "build/user-kernel"
    out.mkdir(parents=True, exist_ok=True)
    sysroot = Path(subprocess.check_output(
        ["rustup", "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True
    ).strip())
    flags = [
        "-C", "target-feature=+c,+zicsr,+zifencei",
        "-C", "relocation-model=static",
        "-C", "code-model=medium",
        "-C", "link-arg=-Tuser.ld",
        "-C", "link-arg=--no-relax",
    ]
    env = {
        **os.environ,
        "RUSTC_BOOTSTRAP": "1",
        "RUSTC": str(sysroot / "bin/rustc"),
        "RUSTDOC": str(sysroot / "bin/rustdoc"),
        "CARGO_ENCODED_RUSTFLAGS": "\x1f".join(flags),
    }
    command = [
        "rustup", "run", TOOLCHAIN, "cargo", "build", "--release", "--locked",
        "--offline", "-Z", "build-std=core", "--target",
        "riscv64imac-unknown-none-elf", "--target-dir", str(out / "target"),
        "--bin", "rica-user",
    ]
    subprocess.run(command, cwd=HERE, env=env, check=True)
    elf = out / "kernel-user.elf"
    shutil.copy2(out / "target/riscv64imac-unknown-none-elf/release/rica-user", elf)
    dump = subprocess.check_output(
        [OBJDUMP, "-d", "-M", "no-aliases", "--mattr=+a", elf], text=True
    )
    counts = audit(dump, allow_a=True)
    (out / "kernel-user.disasm").write_text(dump)
    inputs = [HERE / name for name in (
        "src/user.rs", "src/user_entry.S", "user.ld", "build-user.py", "build.py",
        "Cargo.toml", "Cargo.lock", ".cargo/config.toml",
    )]
    diagnostics = {}
    assembler = OBJDUMP.with_name("llvm-mc")
    linker = sysroot / "lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld"
    tests = HERE.parent / "tests"
    for source, script in (
        (tests / "rv64-u/pmp-machine.S", tests / "rv64i-subset/link.ld"),
        (tests / "rv64-u/pmp-cache.S", tests / "rv64-u/link-ram.ld"),
        (tests / "rv64i-subset/system.S", tests / "rv64i-subset/link.ld"),
        (tests / "rv64-s/supervisor.S", tests / "rv64i-subset/link.ld"),
    ):
        obj = out / (source.stem + ".o")
        diagnostic = out / (source.stem + ".elf")
        commands = [
            [str(assembler), "-triple=riscv64-unknown-none-elf",
             "-mattr=+i,+c,+zicsr,+zifencei", "-filetype=obj", "-I",
             str(tests / "rv64i-subset"), "-o", str(obj), str(source)],
            [str(linker), "-flavor", "gnu", "-m", "elf64lriscv", "-T",
             str(script), "-o", str(diagnostic), str(obj)],
        ]
        for diagnostic_command in commands:
            subprocess.run(diagnostic_command, check=True)
        diagnostics[str(diagnostic.relative_to(ROOT))] = {
            "commands": commands,
            "sha256": hashlib.sha256(diagnostic.read_bytes()).hexdigest(),
        }
        inputs.append(source)
    inputs.extend(tests / "rv64i-subset" / name for name in ("link.ld", "test_macros.inc"))
    inputs.append(tests / "rv64-u/link-ram.ld")
    record = {
        "command": command,
        "rustflags": flags,
        "diagnostics": diagnostics,
        "toolchain": TOOLCHAIN,
        "elf": str(elf.relative_to(ROOT)),
        "sha256": hashlib.sha256(elf.read_bytes()).hexdigest(),
        "instruction_counts": counts,
        "inputs": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in inputs
        },
    }
    (out / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    print(record["elf"], record["sha256"])


if __name__ == "__main__":
    main()
