#!/usr/bin/env python3
"""Build division diagnostics and an M-mode OS boot contract, recording every input."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    out = ROOT / "build/rv64-divide-c"
    out.mkdir(exist_ok=True)
    source = ROOT / "tests/rv64-divide-c"
    clang = os.environ.get("RISCV_CLANG", "clang-20")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    linker = sysroot / "lib/rustlib" / host / "bin/rust-lld"
    common = [clang, "--target=riscv64-unknown-elf", "-march=rv64imc_zicsr_zifencei_zba_zbb_zbs",
              "-mabi=lp64", "-mno-relax", "-mcmodel=medany", "-resource-dir=/usr/lib/llvm-20/lib/clang/20"]
    commands, elfs, inputs, expected = [], {}, {}, {}
    mask = (1 << 64) - 1
    state = 0x984ac313839a6425

    def random():
        nonlocal state
        state ^= (state << 13) & mask
        state ^= state >> 7
        state ^= (state << 17) & mask
        return state

    def invoke(command):
        commands.append([str(v) for v in command])
        subprocess.run(command, check=True)

    def finish(name, objects, required):
        elf = out / (name + ".elf")
        invoke([linker, "-flavor", "gnu", "-m", "elf64lriscv", "-T", ROOT / "tests/rv64-m-benchmarks/link.ld", "-o", elf, *objects])
        dump = subprocess.check_output(["/usr/lib/llvm-20/bin/llvm-objdump", "-d", "-M", "no-aliases", str(elf)], text=True)
        (out / (name + ".disasm")).write_text(dump)
        for mnemonic in required:
            assert any(mnemonic in line.split() for line in dump.splitlines()), (name, mnemonic)
        elfs[str(elf.relative_to(ROOT.parent))] = sha(elf)

    start = out / "start.o"
    invoke(common + ["-c", source / "start.S", "-o", start])
    for name in ["small", "wide", "words"]:
        directory = out / name
        directory.mkdir(exist_ok=True)
        pairs = []
        for i in range(128):
            a, b = random(), random()
            if name == "small":
                a &= (1 << (8 * (1 + i % 4))) - 1
                b = (b & 0xffff) + 1
            elif name == "wide":
                # Include tiny quotients and long full-width divisions.
                if i % 4 == 0:
                    b = (b & 0xffff) + 1
                else:
                    b |= 1 << (32 + i % 32)
            else:
                a &= 0xffff_ffff
                a = a if a < 1 << 31 else a - (1 << 32)
                b = (b & 0xffff) + 1
                if i & 1:
                    b = -b
            pairs.append((a, b))
        checksum = 0
        for a, b in pairs:
            # Integer-only oracle: Python // rounds negative numbers down,
            # while RISC-V/C truncates toward zero.
            q = abs(a) // abs(b)
            if (a < 0) != (b < 0):
                q = -q
            r = a - q * b
            checksum += q + r
        expected[name] = (checksum * 2) & mask
        header = directory / "inputs.h"
        header.write_text("static const uint64_t pairs[128][2] = {\n" +
                          "\n".join(f"    {{UINT64_C({a & mask}), UINT64_C({b & mask})}}," for a, b in pairs) +
                          f"\n}};\n#define EXPECTED UINT64_C({expected[name]})\n")
        inputs[str(header.relative_to(ROOT.parent))] = sha(header)
        obj = out / (name + ".o")
        command = common + ["-O2", "-ffreestanding", "-fno-builtin", "-fno-unroll-loops", "-fno-vectorize", "-fno-slp-vectorize", "-I", directory]
        if name == "words":
            command += ["-DWORDS"]
        invoke(command + ["-c", source / "kernels.c", "-o", obj])
        finish(name, [start, obj], ["divw"] if name == "words" else ["divu"])
    boot = ROOT / "tests/rv64-os/boot_contract.S"
    obj = out / "os-boot-contract.o"
    invoke(common + ["-I", ROOT / "tests/rv64i-subset", "-c", boot, "-o", obj])
    finish("os-boot-contract", [obj], ["wfi", "mret"])
    for path in [Path(__file__).resolve(), source / "kernels.c", source / "start.S", boot,
                 ROOT / "tests/rv64-m-benchmarks/link.ld", ROOT / "tests/rv64i-subset/test_macros.inc"]:
        inputs[str(path.relative_to(ROOT.parent))] = sha(path)
    report = {"compiler": subprocess.check_output([clang, "--version"], text=True),
              "commands": commands, "inputs": inputs, "expected": expected, "elfs": elfs}
    (out / "build.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
