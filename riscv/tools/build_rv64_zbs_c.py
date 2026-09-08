#!/usr/bin/env python3
"""Build identical C workloads with and without Zbs; retain commands and hashes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    out = ROOT / "build/rv64-zbs-c"
    out.mkdir(exist_ok=True)
    source = ROOT / "tests/rv64-zbs-c"
    clang = os.environ.get("RISCV_CLANG", "clang-20")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    linker = sysroot / "lib/rustlib" / host / "bin/rust-lld"
    mask = (1 << 64) - 1
    words = [((0x9e3779b97f4a7c15 * (i + 1)) ^ (0x8000000000000001 >> (i % 64))) & mask for i in range(128)]
    words[:8] = [0, 1, mask, 1 << 63, 0x80000000, 0xffffffff, 0xffffffff00000000, 0x0123456789abcdef]
    expected = {}
    expected['mutate'] = sum(((x | (1 << ((i*3+5) % 64))) & ~(1 << ((i*7+11) % 64))) ^ (1 << ((i*13+3) % 64)) for _ in range(8) for i,x in enumerate(words)) & mask
    expected['extract'] = sum(((x >> (i%64)) & 1) + ((x >> ((i*7+13)%64)) & 1) + ((x >> ((i*11+31)%64)) & 1) for _ in range(8) for i,x in enumerate(words))
    acc = 0
    for _ in range(8):
        for x in words:
            acc = (acc + ((x >> 37) & 1) + ((x >> 19) & 1) + ((x >> 5) & 1)) & mask
            acc ^= (x | (1 << 47)) & ~(1 << 29)
    expected['immediate'] = acc
    header = ["static const uint64_t words[] = {" + ",".join(f"0x{x:x}ULL" for x in words) + "};"]
    header += [f"#define EXPECTED_{name.upper()} UINT64_C({value})" for name, value in expected.items()]
    inputs = out / "inputs.h"
    inputs.write_text("\n".join(header) + "\n")
    commands = []
    elfs = {}
    for zbs in [False, True]:
        march = "rv64imc_zicsr_zifencei_zba_zbb" + ("_zbs" if zbs else "")
        common = [clang, "--target=riscv64-unknown-elf", f"-march={march}", "-mabi=lp64", "-mno-relax", "-mcmodel=medany", "-resource-dir=/usr/lib/llvm-20/lib/clang/20"]
        start = out / ("start-zbs.o" if zbs else "start-base.o")
        command = common + ["-c", str(source / "start.S"), "-o", str(start)]
        commands.append(command); subprocess.run(command, check=True)
        for workload in ["mutate", "extract", "immediate"]:
            stem = workload + ("-zbs" if zbs else "-base")
            obj, elf = out / (stem + ".o"), out / (stem + ".elf")
            command = common + ["-O2", "-ffreestanding", "-fno-builtin", "-fno-unroll-loops", "-fno-vectorize", "-fno-slp-vectorize", "-I", str(out)]
            command.append("-D" + workload.upper())
            command += ["-c", str(source / "kernels.c"), "-o", str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            command = [str(linker), "-flavor", "gnu", "-m", "elf64lriscv", "-T", str(ROOT / "tests/rv64-m-benchmarks/link.ld"), "-o", str(elf), str(start), str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            dump = subprocess.check_output(["/usr/lib/llvm-20/bin/llvm-objdump", "-d", "-M", "no-aliases", str(elf)], text=True)
            (out / (stem + ".disasm")).write_text(dump)
            zbs_lines = [line for line in dump.splitlines() if any(op in line for op in ["bclr", "bext", "binv", "bset"])]
            assert bool(zbs_lines) == zbs, (stem, zbs_lines)
            elfs[elf.name] = {"sha256": sha(elf), "zbs_instructions": zbs_lines}
    report = {"compiler": subprocess.check_output([clang, "--version"], text=True),
              "commands": commands, "source_sha256": {p.name: sha(p) for p in [source / "kernels.c", source / "start.S", inputs]},
              "expected": expected, "elfs": elfs}
    (out / "build.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(elfs, indent=2))


if __name__ == "__main__":
    main()
