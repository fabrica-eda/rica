#!/usr/bin/env python3
"""Build and audit the M-mode Rust kernel with a core that does not require A."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUT = HERE.parent / "build/rust-kernel"
TARGET = "riscv64im-unknown-none-elf"
TOOLCHAIN = "1.98.1"
OBJDUMP = Path("/usr/lib/llvm-20/bin/llvm-objdump")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def audit(text):
    # Explicit integer-only allow-list, in addition to the encoding checks.
    # In particular, the usual riscv64gc target would introduce A/F/D here.
    base = set("""lui auipc jal jalr beq bne blt bge bltu bgeu
        lb lh lw ld lbu lhu lwu sb sh sw sd
        addi slti sltiu xori ori andi slli srli srai add sub sll slt sltu xor srl sra or and
        addiw slliw srliw sraiw addw subw sllw srlw sraw
        mul mulh mulhsu mulhu div divu rem remu mulw divw divuw remw remuw
        fence fence.i csrrw csrrs csrrc csrrwi csrrsi csrrci ecall ebreak mret wfi""".split())
    compressed = {"c." + name for name in """addi4spn lw ld sw sd nop addi addiw li
        addi16sp lui srli srai andi sub xor or and subw addw j beqz bnez slli
        lwsp ldsp jr mv ebreak jalr add swsp sdsp""".split()}
    counts = {}
    for match in re.finditer(r"^\s*[0-9a-f]+:\s+([0-9a-f]{4}|[0-9a-f]{8})\s+(\S+)", text, re.M):
        encoding, mnemonic = match.groups()
        instruction = int(encoding, 16)
        if instruction == 0 and mnemonic in {"unimp", "c.unimp"}:
            # Linker alignment padding is deliberately an illegal instruction.
            counts["zero_padding"] = counts.get("zero_padding", 0) + 1
            continue
        assert mnemonic in base | compressed, match.group(0)
        if len(encoding) == 8:
            assert instruction & 0x7f in {
                0x03, 0x0f, 0x13, 0x17, 0x1b, 0x23, 0x33, 0x37,
                0x3b, 0x63, 0x67, 0x6f, 0x73,
            }, match.group(0)
        else:
            assert mnemonic in compressed, match.group(0)
        counts[mnemonic] = counts.get(mnemonic, 0) + 1
    assert counts.get("mret", 0) and counts.get("ecall", 0)
    assert sum(v for k, v in counts.items() if k.startswith("c.")) > 0
    return counts


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    compiler = subprocess.check_output(["rustup", "run", TOOLCHAIN, "rustc", "-vV"], text=True)
    assert "release: 1.98.1\n" in compiler
    sysroot = Path(subprocess.check_output(
        ["rustup", "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True).strip())
    source = sysroot / "lib/rustlib/src/rust/library"
    assert (source / "core/Cargo.toml").is_file(), "rustup component add rust-src --toolchain 1.98.1"
    channel = tomllib.loads((sysroot / "lib/rustlib/multirust-channel-manifest.toml").read_text())
    rust_src = channel["pkg"]["rust-src"]
    source_fingerprint = hashlib.sha256()
    for path in sorted(p for p in source.rglob("*") if p.is_file()):
        source_fingerprint.update((str(path.relative_to(source)) + "\0" + digest(path) + "\n").encode())
    records, elfs = {}, {}
    for name, features in [("kernel-tasks", []), ("kernel-access-faults", ["--features", "access-faults"])]:
        cmd = ["cargo", "+" + TOOLCHAIN, "build", "--release", "--locked", "--offline",
               "-Z", "build-std=core", "--target-dir", "../build/kernel-target"] + features
        # Scope the unstable build-std opt-in to this exact pinned command. No
        # global toolchain switch and no forced/emulated atomics are used.
        subprocess.run(cmd, cwd=HERE, env={**os.environ, "RUSTC_BOOTSTRAP": "1"}, check=True)
        elf = OUT / (name + ".elf")
        shutil.copy2(HERE.parent / "build/kernel-target" / TARGET / "release/rica-kernel", elf)
        header = elf.read_bytes()[:64]
        assert header[:6] == b"\x7fELF\x02\x01"  # ELF64, little-endian
        assert struct.unpack_from("<H", header, 18)[0] == 243  # EM_RISCV
        flags = struct.unpack_from("<I", header, 48)[0]
        assert flags & 1 and flags & 6 == 0  # RVC, soft-float LP64 ABI
        dump = subprocess.check_output([OBJDUMP, "-d", "-M", "no-aliases", elf], text=True)
        counts = audit(dump)
        (OUT / (name + ".disasm")).write_text(dump)
        records[name] = {"command": cmd, "elf_flags": flags, "instruction_counts": counts}
        elfs[str(elf.relative_to(ROOT))] = digest(elf)
    workload = []
    for task in range(2):
        value = 0x123456789abcdef0 ^ task
        for step in range(512):
            rotated = ((value << 7) | (value >> 57)) % (1 << 64)
            value = (rotated * 0x1000000001b3) % (1 << 64) ^ step
        workload.append(f"0x{value:016x}")
    assert workload == ["0xd7216b1b6ab9df9b", "0x9c776af03aedcfd4"]
    inputs = [HERE / p for p in ["Cargo.toml", "Cargo.lock", ".cargo/config.toml", "link.ld", "build.py"]]
    inputs += [p for p in (HERE / "src").rglob("*") if p.is_file()]
    record = {
        "compiler": compiler, "rustc_sha256": digest(sysroot / "bin/rustc"),
        "std_lock_sha256": digest(source / "Cargo.lock"),
        "rust_src_version": rust_src["version"],
        "rust_src_package": {k: rust_src["target"]["*"][k] for k in ["xz_url", "xz_hash"]},
        "rust_library_tree_sha256": source_fingerprint.hexdigest(),
        "objdump_sha256": digest(OBJDUMP), "target": TARGET,
        "workload_checksums": workload, "builds": records, "elfs": elfs,
        "inputs": {str(p.relative_to(ROOT)): digest(p) for p in sorted(inputs)},
    }
    (OUT / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({"elfs": elfs, "workload_checksums": workload}, indent=2))


if __name__ == "__main__":
    main()
