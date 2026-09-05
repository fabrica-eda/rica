#!/usr/bin/env python3
"""Record reproducible compact-predictor model comparisons, without hardware IO."""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import time


ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new evidence directory")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    baseline = json.loads((ROOT / "toolchain/measurements/2026-09-05-rv64-124p8-hardware/elf-sha256.json").read_text())
    for name, expected in baseline.items():
        if sha(ROOT / name) != expected:
            raise RuntimeError(f"baseline ELF changed: {name}")
    extra = sorted((ROOT / "riscv/build/rv64-predictor").glob("*.elf"))
    if len(extra) != 2:
        raise RuntimeError("build --predictor-tests first (two diagnostic ELFs required)")
    elfs = {**baseline, **{str(p.relative_to(ROOT)): sha(p) for p in extra}}
    sources = sorted((ROOT / "riscv/src").glob("*.veryl")) + [
        ROOT / "riscv/Veryl.toml", ROOT / "riscv/pll-12-to-124p8.json",
        ROOT / "riscv/lfe5um5g-85f-evn.lpf",
    ]
    source_hashes = {str(p.relative_to(ROOT)): sha(p) for p in sources}
    (output / "source-sha256.json").write_text(json.dumps(source_hashes, indent=2) + "\n")
    (output / "elf-sha256.json").write_text(json.dumps(elfs, indent=2) + "\n")
    diagnostics = [p for p in elfs if any(f"/{suite}/" in p for suite in [
        "rv64-benchmarks", "rv64-cache", "rv64-predictor",
    ])]
    jobs = [("core64", ["target/release/rv64-riscv-tests", "--compact-predictor", "--max-cycles", "1000000", *elfs])]
    for name, flag in [("compact64", "--compact-predictor"), ("compact32", "--compact-predictor-32"),
                       ("no-ras", "--compact-predictor-no-ras"), ("no-direction", "--compact-predictor-no-direction"),
                       ("static", "--static-predictor")]:
        jobs.append((name, ["target/release/verify-riscv", "--hardware-memory-rtl", flag,
                            "--icache-kib", "4", *(elfs if name == "compact64" else diagnostics)]))

    def run(job):
        name, command = job
        record = {"command": command, "started_epoch": time.time(), "executable_sha256": sha(ROOT / command[0])}
        log_path = output / f"{name}.log"
        with log_path.open("w") as log:
            result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        record.update(exit_code=result.returncode, finished_epoch=time.time(), log_sha256=sha(log_path))
        (output / f"{name}-command.json").write_text(json.dumps(record, indent=2) + "\n")
        print(name, "exit", result.returncode, flush=True)
        return result.returncode

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(run, jobs))
    if any(sha(ROOT / name) != expected for name, expected in source_hashes.items()):
        raise RuntimeError("RTL or constraints changed during measurement")
    if any(results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
