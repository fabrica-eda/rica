#!/usr/bin/env python3
"""Route RV64 configurations with identical constraints and retain evidence.

Build maps with nextpnr-riscv Hardware{,Static,Cached2K,Cached4K,Cached8K}TestTop --nextpnr-json first.
This runner never generates or programs a bitstream. A reported frequency
ceiling is an STA estimate, not a qualified operating clock.
"""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess


ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def timing(report):
    clocks = report["fmax"]
    # The checked-in 624 MHz PLL has output divides 5/10. Ignore the two
    # unrelated JTAG clocks, which nextpnr may constrain to exactly 125 MHz.
    def clock_at(frequency):
        matches = [name for name, item in clocks.items()
                   if abs(item["constraint"] - frequency) < 0.02]
        if len(matches) != 1:
            raise ValueError(f"expected one {frequency} MHz PLL clock: {matches}")
        return matches[0]

    cpu = clock_at(124.8128)
    memory = clock_at(62.4025)

    def crossing(source, destination):
        paths = [sum(segment["delay"] for segment in path["path"])
                 for path in report["critical_paths"]
                 if path["from"] == f"posedge {source}"
                 and path["to"] == f"posedge {destination}"]
        if not paths:
            raise ValueError(f"missing related-clock path: {source} -> {destination}")
        return max(paths)

    cpu_to_memory = crossing(cpu, memory)
    memory_to_cpu = crossing(memory, cpu)
    cpu_fmax = clocks[cpu]["achieved"]
    memory_fmax = clocks[memory]["achieved"]
    ceiling = min(cpu_fmax, 2 * memory_fmax,
                  1000 / max(cpu_to_memory, memory_to_cpu))
    return {
        "cpu_fmax_mhz": cpu_fmax,
        "memory_fmax_mhz": memory_fmax,
        "cpu_to_memory_ns": cpu_to_memory,
        "memory_to_cpu_ns": memory_to_cpu,
        "related_clock_ceiling_mhz": ceiling,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compact", type=Path)
    parser.add_argument("--static", dest="static_map", type=Path)
    parser.add_argument("--cache2k", type=Path)
    parser.add_argument("--cache4k", type=Path)
    parser.add_argument("--cache8k", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--nextpnr", default="nextpnr-ecp5")
    parser.add_argument("--placer", default="heap")
    parser.add_argument("--seeds", nargs="+", type=int, default=[1, 2, 5])
    parser.add_argument("--validator", type=Path,
                        default=ROOT / "target/release/nextpnr-riscv")
    args = parser.parse_args()
    variants = [(name, source) for name, source in
                [("compact", args.compact), ("static", args.static_map), ("cache2k", args.cache2k),
                 ("cache4k", args.cache4k), ("cache8k", args.cache8k)] if source is not None]
    if not variants:
        parser.error("provide at least one of --compact, --static, --cache2k, --cache4k or --cache8k")
    if len(set(args.seeds)) != len(args.seeds):
        parser.error("seeds must be distinct")
    args.output.mkdir(parents=True, exist_ok=True)
    version = subprocess.check_output([args.nextpnr, "--version"],
                                      stderr=subprocess.STDOUT, text=True).strip()
    lpf = ROOT / "riscv/lfe5um5g-85f-evn.lpf"
    results = {
        "nextpnr": version, "placer": args.placer, "router": "router1",
        "device": "LFE5UM5G-85F", "package": "CABGA381", "speed": 8,
        "lpf_sha256": digest(lpf),
        "pll_sha256": digest(ROOT / "riscv/pll-12-to-124p8.json"),
        "runs": [], "medians": {},
    }
    summary = args.output / "summary.json"
    for variant, source in variants:
        # Snapshot each input before launching P&R. Another synthesis run must
        # not silently replace the input between seeds.
        mapped = args.output / f"{variant}.mapped.json"
        if mapped.exists():
            raise FileExistsError(f"use a fresh output directory: {mapped}")
        mapped.write_bytes(source.read_bytes())
        map_hash = digest(mapped)
        for seed in args.seeds:
            stem = args.output / f"{variant}-seed{seed}"
            routed = Path(f"{stem}.routed.json")
            report = Path(f"{stem}.report.json")
            command = [args.nextpnr, "--um5g-85k", "--package", "CABGA381",
                       "--speed", "8", "--json", str(mapped), "--freq", "125",
                       "--timing-allow-fail", "--lpf", str(lpf),
                       "--placer", args.placer, "--router", "router1",
                       "--seed", str(seed), "--write", str(routed),
                       "--report", str(report)]
            print(f"Routing {variant}, seed {seed}", flush=True)
            with Path(f"{stem}.log").open("w") as log:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT,
                               check=True)
            validation = subprocess.run(
                [str(args.validator), "--validate-board-report", str(report)],
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
            )
            Path(f"{stem}.validation.log").write_text(validation.stdout)
            result = {
                "variant": variant, "seed": seed, "command": command,
                "map_sha256": map_hash, "report_sha256": digest(report),
                "routed_sha256": digest(routed),
                "target_timing_pass": validation.returncode == 0,
                "target_timing_message": validation.stdout.strip(),
                **timing(json.loads(report.read_text())),
            }
            results["runs"].append(result)
            summary.write_text(json.dumps(results, indent=2) + "\n")
            print(json.dumps(result), flush=True)
        rows = [row for row in results["runs"] if row["variant"] == variant]
        results["medians"][variant] = {
            key: statistics.median(row[key] for row in rows)
            for key in timing(json.loads(report.read_text()))
        }
        summary.write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
