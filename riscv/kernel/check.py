#!/usr/bin/env python3
"""Build the Rust kernels and run them against the current CPU RTL."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from build import TOOLCHAIN

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUT = HERE.parent / 'build/rust-kernel'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    subprocess.run([sys.executable, str(HERE / 'build.py')], cwd=ROOT, check=True)
    sysroot = Path(subprocess.check_output(
        ['rustup', 'run', TOOLCHAIN, 'rustc', '--print', 'sysroot'], text=True).strip())
    subprocess.run(['rustup', 'run', TOOLCHAIN, 'cargo', 'build', '--release',
                    '--offline', '--locked', '--bin', 'verify-riscv',
                    '--target-dir', str(ROOT / 'target')],
                   cwd=ROOT, check=True, env={**os.environ,
                       'RUSTC': str(sysroot / 'bin/rustc'),
                       'RUSTDOC': str(sysroot / 'bin/rustdoc')})
    build = json.loads((OUT / 'build.json').read_text())
    runner = ROOT / 'target/release/verify-riscv'
    sources = {str(p.relative_to(ROOT)): sha(p) for p in sorted((ROOT / 'riscv/src').glob('*.veryl'))}
    inputs = {**sources, **build['inputs'], **build['elfs']}
    assert all(sha(ROOT / p) == h for p, h in inputs.items())
    flags = [
        '--hardware-memory-rtl', '--machine-interrupts', '--strict-memory',
        '--early-bitmanip', '--early-taken-fetch', '--early-divide',
        '--compact-predictor', '--icache-kib', '4', '--fast-lsu', '6',
        '--dsp-multiplier', '--zba-extension', '--zbb-extension', '--zbs-extension',
        '--max-cycles', '1000000',
    ]
    record = {
        'kind': 'CPU/kernel model regression; physical timing and hardware qualification are separate',
        'inputs': inputs, 'runner_sha256': sha(runner),
        'build_record_sha256': sha(OUT / 'build.json'), 'runs': [],
    }
    plain = [p for p in build['elfs'] if Path(p).stem not in {'kernel-console', 'kernel-atomics'}]
    console = [p for p in build['elfs'] if Path(p).stem == 'kernel-console']
    atomic = [p for p in build['elfs'] if Path(p).stem == 'kernel-atomics']
    assert len(atomic) == 1
    assert len(plain) == 3 and len(console) == 1
    for name, extra, elfs, timed in [
        ('atomic-serialized', ['--a-extension'], atomic, False),
        ('atomic-simultaneous', ['--a-extension', '--timed-clocks'], atomic, True),
        ('serialized', [], plain, False),
        ('simultaneous', ['--timed-clocks'], plain, True),
        ('console-serialized', ['--kernel-console'], console, False),
        ('console-simultaneous', ['--kernel-console', '--timed-clocks'], console, True),
    ]:
        command = [str(runner), *flags, *extra, *[str(ROOT / p) for p in elfs]]
        log = OUT / (f'check-{name}.log')
        start = time.monotonic()
        with log.open('w') as stream:
            result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
        lines = log.read_text().splitlines()
        metrics = [json.loads(line.removeprefix('TIMED_METRIC '))
                   for line in lines if line.startswith('TIMED_METRIC ')]
        passes = sum('] PASS;' in line for line in lines) if not timed else len(metrics)
        record['runs'].append({
            'name': name, 'command': command, 'exit_code': result.returncode,
            'seconds': time.monotonic() - start, 'passes': passes, 'metrics': metrics,
            'transcripts': [json.loads(line.removeprefix('CONSOLE_TRANSCRIPT '))
                            for line in lines if line.startswith('CONSOLE_TRANSCRIPT ')],
            'log': str(log.relative_to(ROOT)), 'log_sha256': sha(log),
        })
        record['inputs_unchanged'] = all(sha(ROOT / p) == h for p, h in inputs.items())
        record['runner_unchanged'] = sha(runner) == record['runner_sha256']
        (OUT / 'check.json').write_text(json.dumps(record, indent=2) + '\n')
        assert result.returncode == 0 and passes == len(elfs) * (1 if timed else 2), log
        assert record['inputs_unchanged'] and record['runner_unchanged']
        print(f'{name}: {passes} PASS', flush=True)


if __name__ == '__main__':
    main()
