#!/usr/bin/env python3
"""Build identical C workloads with and without Zbb; retain commands and hashes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    out = ROOT / "build/rv64-zbb-c"
    out.mkdir(exist_ok=True)
    source = ROOT / "tests/rv64-zbb-c"
    clang = os.environ.get("RISCV_CLANG", "clang-20")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    linker = sysroot / "lib/rustlib" / host / "bin/rust-lld"
    mask = (1 << 64) - 1
    words = [((0x9e3779b97f4a7c15 * (i + 1)) ^ (0x8000000000000001 >> (i % 64))) & mask for i in range(128)]
    words[:8] = [0, 1, mask, 1 << 63, 0x80000000, 0xffffffff, 0xffffffff00000000, 0x0123456789abcdef]
    def signed(x, width):
        x &= (1 << width) - 1
        return x - (1 << width) if x >> (width - 1) else x
    def rotate_right(x, amount, width):
        amount %= width
        return ((x >> amount) | (x << ((-amount) % width))) & ((1 << width) - 1)
    counts = 0
    hashed = 0x1234567887654321
    byte_sum = 0
    for _ in range(8):
        for i, x in enumerate(words):
            for width in [64, 32]:
                low = x & ((1 << width) - 1)
                trailing = low | (1 << (width - 1))
                counts += low.bit_count() + width - (low | 1).bit_length() + (trailing & -trailing).bit_length() - 1
            hashed = (rotate_right(hashed ^ x, -(i * 7 + 13), 64) + rotate_right(x & 0xffffffff, i * 11 + 3, 32)) & mask
            byte_sum = (byte_sum + int.from_bytes(x.to_bytes(8, 'little'), 'big') + min(signed(x,64), -1234567) + max(signed(x,64),7654321)) & mask
            byte_sum ^= (signed(x,8) + signed(x,16) + (x & 65535)) & mask
    expected = {"counts": counts, "rotate": hashed, "bytes": byte_sum}
    header = ["static const uint64_t words[] = {" + ",".join(f"0x{x:x}ULL" for x in words) + "};"]
    header += [f"#define EXPECTED_{name.upper()} UINT64_C({value})" for name, value in expected.items()]
    inputs = out / "inputs.h"
    inputs.write_text("\n".join(header) + "\n")
    commands = []
    elfs = {}
    for zbb in [False, True]:
        march = "rv64imc_zicsr_zifencei_zba" + ("_zbb" if zbb else "")
        common = [clang, "--target=riscv64-unknown-elf", f"-march={march}", "-mabi=lp64", "-mno-relax", "-mcmodel=medany", "-resource-dir=/usr/lib/llvm-20/lib/clang/20"]
        start = out / ("start-zbb.o" if zbb else "start-base.o")
        command = common + ["-c", str(source / "start.S"), "-o", str(start)]
        commands.append(command); subprocess.run(command, check=True)
        for workload in ["counts", "rotate", "bytes"]:
            stem = workload + ("-zbb" if zbb else "-base")
            obj, elf = out / (stem + ".o"), out / (stem + ".elf")
            command = common + ["-O2", "-ffreestanding", "-fno-builtin", "-fno-unroll-loops", "-fno-vectorize", "-fno-slp-vectorize", "-I", str(out)]
            command.append("-D" + workload.upper())
            command += ["-c", str(source / "kernels.c"), "-o", str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            command = [str(linker), "-flavor", "gnu", "-m", "elf64lriscv", "-T", str(ROOT / "tests/rv64-m-benchmarks/link.ld"), "-o", str(elf), str(start), str(obj)]
            commands.append(command); subprocess.run(command, check=True)
            dump = subprocess.check_output(["/usr/lib/llvm-20/bin/llvm-objdump", "-d", "-M", "no-aliases", str(elf)], text=True)
            (out / (stem + ".disasm")).write_text(dump)
            zbb_lines = [line for line in dump.splitlines() if any(op in line for op in ["rol", "ror", "cpop", "clz", "ctz", "min", "max", "rev8", "sext.", "zext.h", "andn", "orn", "xnor", "orc.b"])]
            assert bool(zbb_lines) == zbb, (stem, zbb_lines)
            elfs[elf.name] = {"sha256": sha(elf), "zbb_instructions": zbb_lines}
    report = {"compiler": subprocess.check_output([clang, "--version"], text=True),
              "commands": commands, "source_sha256": {p.name: sha(p) for p in [source / "kernels.c", source / "start.S", inputs]},
              "expected": expected, "elfs": elfs}
    (out / "build.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(elfs, indent=2))


if __name__ == "__main__":
    main()
