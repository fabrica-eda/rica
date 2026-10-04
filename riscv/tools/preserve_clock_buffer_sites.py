#!/usr/bin/env python3
"""Append qualified board clock-buffer sites to a Texo placement seed.

The mapped graph precedes global-clock promotion and omits the three buffers.
Resolve their generated names from the actual clock-source pins, rather than
assuming that synthesis keeps the same temporary net names. These are placement
hints only: routing, clock periods, uncertainty and coverage still need sign-off.
"""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--graph", type=Path, required=True)
    parser.add_argument("--initial-placement", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--sites",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "qualified-clock-buffer-sites.json",
    )
    args = parser.parse_args()
    if args.output.resolve() in {
        args.graph.resolve(), args.initial_placement.resolve(), args.sites.resolve()
    }:
        parser.error("output must be distinct from all inputs")
    cells = json.loads(args.graph.read_text())
    by_id = {cell["id"]: cell for cell in cells}
    placement = json.loads(args.initial_placement.read_text())
    sites = json.loads(args.sites.read_text())
    if not isinstance(placement, dict) or not all(
        isinstance(k, str) and isinstance(v, str) for k, v in placement.items()
    ):
        parser.error("initial placement must map cell names to BEL names")
    clocks = {}
    for cell in cells:
        if cell["kind"] != "Register":
            continue
        for pin in cell["inputs"]:
            if pin["pin"] != "CLK":
                continue
            source = f"{by_id[pin['driver']]['name']}.{pin['driver_pin']}"
            if source in clocks and clocks[source] != pin["net"]:
                parser.error(f"clock source {source} has multiple logical nets")
            clocks[source] = pin["net"]
    if set(clocks) != set(sites):
        parser.error(f"clock sources {sorted(clocks)} differ from site profile {sorted(sites)}")
    buffers = {f"$gbuf${net}": sites[source] for source, net in clocks.items()}
    unexpected = {name for name in placement if name.startswith("$gbuf$")} - buffers.keys()
    if unexpected:
        parser.error(f"placement contains stale clock buffers: {sorted(unexpected)}")
    placement.update(buffers)
    if len(set(placement.values())) != len(placement):
        parser.error("clock-buffer sites conflict with other placement bindings")
    args.output.write_text(json.dumps(placement, indent=2) + "\n")
    print(json.dumps(buffers, indent=2))


if __name__ == "__main__":
    main()
