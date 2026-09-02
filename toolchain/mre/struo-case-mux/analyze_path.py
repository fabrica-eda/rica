#!/usr/bin/env python3
import json
import sys
from functools import lru_cache

path, source_name, target_name = sys.argv[1:4]
with open(path, encoding="utf-8") as source:
    design = json.load(source)
module = next(iter(design["modules"].values()))
cells = module["cells"]


def scalar(cell, port):
    bits = cell["connections"][port]
    if len(bits) != 1:
        raise ValueError(f"{port} is not scalar: {bits}")
    return bits[0]


source_wire = scalar(cells[source_name], "Q")
target_wire = scalar(cells[target_name], "DI")
drivers = {}
for name, cell in cells.items():
    inputs = []
    outputs = []
    for port, direction in cell["port_directions"].items():
        connections = cell["connections"].get(port, [])
        if direction == "input":
            inputs.extend(connections)
        elif direction == "output":
            outputs.extend(connections)
    for output in outputs:
        drivers[output] = (name, cell["type"], tuple(inputs))


@lru_cache(maxsize=None)
def longest_to_source(wire):
    if wire == source_wire:
        return ()
    driver = drivers.get(wire)
    if driver is None or driver[1] == "TRELLIS_FF":
        return None
    name, kind, inputs = driver
    candidates = []
    for input_wire in inputs:
        prefix = longest_to_source(input_wire)
        if prefix is not None:
            candidates.append(prefix + ((name, kind, input_wire, wire),))
    return max(candidates, key=len) if candidates else None


result = longest_to_source(target_wire)
print(f"source={source_name}.Q wire={source_wire}")
print(f"target={target_name}.DI wire={target_wire}")
if result is None:
    print("no combinational path")
    raise SystemExit(1)
for index, (name, kind, input_wire, output_wire) in enumerate(result, 1):
    print(f"{index:02d} {kind:12s} {name} {input_wire}->{output_wire}")
counts = {}
for _, kind, _, _ in result:
    counts[kind] = counts.get(kind, 0) + 1
print("counts=" + ",".join(f"{kind}:{count}" for kind, count in sorted(counts.items())))
