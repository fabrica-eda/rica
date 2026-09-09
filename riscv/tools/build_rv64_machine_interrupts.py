#!/usr/bin/env python3
"""Build self-checking machine interrupt and integer-context preemption ELFs."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    out = ROOT / 'build/rv64-machine-interrupts'
    out.mkdir(exist_ok=True)
    clang = os.environ.get('RISCV_CLANG', 'clang-20')
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
    linker = sysroot / 'lib/rustlib' / host / 'bin/rust-lld'
    commands, inputs, elfs = [], {}, {}
    for name in ['machine_interrupts', 'timer_context_switch']:
        source = ROOT / 'tests/rv64-os' / (name + '.S')
        obj, elf = out / (name + '.o'), out / (name + '.elf')
        for cmd in [
            [clang, '--target=riscv64-unknown-elf', '-march=rv64imc_zicsr_zifencei', '-mabi=lp64', '-mno-relax', '-mcmodel=medany', '-I', ROOT / 'tests/rv64i-subset', '-c', source, '-o', obj],
            [linker, '-flavor', 'gnu', '-m', 'elf64lriscv', '-T', ROOT / 'tests/rv64-m-benchmarks/link.ld', '-o', elf, obj],
        ]:
            cmd = list(map(str, cmd))
            commands.append(cmd)
            subprocess.run(cmd, check=True)
        dump = subprocess.check_output(['/usr/lib/llvm-20/bin/llvm-objdump', '-d', '-M', 'no-aliases', elf], text=True)
        assert any('mret' in line.split() for line in dump.splitlines())
        (out / (name + '.disasm')).write_text(dump)
        inputs[str(source.relative_to(ROOT.parent))] = digest(source)
        elfs[str(elf.relative_to(ROOT.parent))] = digest(elf)
    for source in [Path(__file__).resolve(), ROOT / 'tests/rv64i-subset/test_macros.inc', ROOT / 'tests/rv64-m-benchmarks/link.ld']:
        inputs[str(source.relative_to(ROOT.parent))] = digest(source)
    record = dict(commands=commands, inputs=inputs, elfs=elfs,
                  compiler=subprocess.check_output([clang, '--version'], text=True),
                  compiler_sha256=digest(clang), linker_sha256=digest(linker))
    (out / 'build.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))


if __name__ == '__main__':
    main()
