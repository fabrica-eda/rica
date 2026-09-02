#!/usr/bin/env python3
"""Dependency-free tests for the soft-region config safety checks."""

from pathlib import Path

from region_spec import RegionSpecError, expand_exact_members, load, merge_assignment, required_capacity


ROOT = Path(__file__).resolve().parent
config = load(ROOT / "region_corridor.json")
device_bounds = (0, 0, 126, 95)
config = __import__("region_spec").validate(
    {
        "schema": 1,
        "regions": {name: {"rect": rectangle} for name, rectangle in config["regions"].items()},
        "rules": config["rules"],
        "require_overlap": config["require_overlap"],
        "propagate_ff_di_driver": config["propagate_ff_di_driver"],
    },
    device_bounds,
)

names = [
    "bram_hardware_memory.mem[0]",
    "ff_hardware_memory.valid[0]",
    "bram_core.branch_predictor.tage[0]",
    "bram_core.registers.ram[0]",
    "ff_core.branch_predictor.tage[0]",
    "ff_core.registers.read[0]",
]
assignments, matches = expand_exact_members(names, config["rules"])
assert len(matches) == 6
assert len(assignments) == len(names)
assert assignments["ff_core.branch_predictor.tage[0]"] == "cpu_predictor_corridor"

try:
    expand_exact_members(names, [{"region": "cpu_predictor_corridor", "glob": "missing.*"}])
except RegionSpecError:
    pass
else:
    raise AssertionError("unmatched glob was accepted")

try:
    expand_exact_members(["shared"], [
        {"region": "hardware_memory_corridor", "glob": "shared"},
        {"region": "cpu_predictor_corridor", "glob": "shared"},
    ])
except RegionSpecError:
    pass
else:
    raise AssertionError("split assignment was accepted")

try:
    merge_assignment({"shared": "hardware_memory_corridor"}, "shared", "cpu_predictor_corridor", "test")
except RegionSpecError:
    pass
else:
    raise AssertionError("cluster merge conflict was accepted")

cell_types = {name: "DP16KD" for name in assignments}
try:
    required_capacity(assignments, cell_types, {("hardware_memory_corridor", "DP16KD"): 1})
except RegionSpecError:
    pass
else:
    raise AssertionError("insufficient legal resource capacity was accepted")

print("region config tests: PASS")
