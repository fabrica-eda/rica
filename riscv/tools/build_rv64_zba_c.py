#!/usr/bin/env python3
"""Build identical C workloads with and without Zba; retain commands and hashes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    out = ROOT / "build/rv64-zba-c"
    out.mkdir(exist_ok=True)
    source = ROOT / "tests/rv64-zba-c"
    clang = os.environ.get("RISCV_CLANG", "clang-20")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    linker = sysroot / "lib/rustlib" / host / "bin/rust-lld"
    table = [(i * 251 + 17) & 65535 for i in range(256)]
    indices32 = [(i * 73 + 19) & 255 for i in range(128)]
    mask = (1 << 64) - 1
    indices64 = [((0x9e3779b97f4a7c15 * (i + 1)) ^ 0xffffffff80000001) & mask for i in range(128)]
    expected_gather = sum(table[i] for i in indices32) * 8
    expected_addresses = 0
    for _ in range(8):
        for index in indices64:
            expected_addresses ^= (0x1234567800000100 + ((index & 0xffffffff) << 3)) & mask
            expected_addresses = (expected_addresses + 0x1234567800000100 + (index << 2)) & mask
    header = []
    for typename, name, values in [("uint16_t", "table", table), ("uint32_t", "indices32", indices32), ("uint64_t", "indices64", indices64)]:
        header.append(f"static const {typename} {name}[] = {{" + ",".join(f"0x{v:x}ULL" for v in values) + "};")
    header += [f"#define EXPECTED_GATHER UINT64_C({expected_gather})", f"#define EXPECTED_ADDRESSES UINT64_C({expected_addresses})"]
    inputs = out / "inputs.h"
    inputs.write_text("\n".join(header) + "\n")
    commands = []
    elfs = {}
    for zba in [False, True]:
        march = "rv64imc_zicsr_zifencei" + ("_zba" if zba else "")
        common = [clang, "--target=riscv64-unknown-elf", f"-march={march}", "-mabi=lp64", "-mno-relax", "-mcmodel=medany", "-resource-dir=/usr/lib/llvm-20/lib/clang/20"]
        start = out / ("start-zba.o" if zba else "start-base.o")
        command = common + ["-c", str(source / "start.S"), "-o", str(start)]
        commands.append(command); subprocess.run(command, check=True)
        for workload in ["gather", "addresses"]:
            stem = workload + ("-zba" if zba else "-base")
            obj, elf = out / (stem + ".o"), out / (stem + ".elf")
            command = common + ["-O2", "-ffreestanding", "-fno-builtin", "-fno-unroll-loops", "-fno-vectorize", "-fno-slp-vectorize", "-I", str(out)]
            if workload == "gather": command.append("-DGATHER")
            command += ["-c", str(source / "kernels.c"), "-o", str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            command = [str(linker), "-flavor", "gnu", "-m", "elf64lriscv", "-T", str(ROOT / "tests/rv64-m-benchmarks/link.ld"), "-o", str(elf), str(start), str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            dump = subprocess.check_output(["/usr/lib/llvm-20/bin/llvm-objdump", "-d", "-M", "no-aliases", str(elf)], text=True)
            (out / (stem + ".disasm")).write_text(dump)
            zba_lines = [line for line in dump.splitlines() if any(op in line for op in ["sh1add", "sh2add", "sh3add", "add.uw", "slli.uw"])]
            assert bool(zba_lines) == zba, (stem, zba_lines)
            elfs[elf.name] = {"sha256": sha(elf), "zba_instructions": zba_lines}
    report = {"compiler": subprocess.check_output([clang, "--version"], text=True),
              "commands": commands, "source_sha256": {p.name: sha(p) for p in [source / "kernels.c", source / "start.S", inputs]},
              "expected_gather": expected_gather, "expected_addresses": expected_addresses, "elfs": elfs}
    (out / "build.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(elfs, indent=2))


if __name__ == "__main__":
    main()
