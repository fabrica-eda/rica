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

try:
    __import__("region_spec").validate({"schema": 1, "regions": {"r": [0, 0, 1, 1]},
                                         "rules": [{"region": "r", "glob": "*"}],
                                         "require_overlap": None})
except RegionSpecError:
    pass
else:
    raise AssertionError("non-list require_overlap was accepted")

try:
    __import__("region_spec").validate({"schema": 1, "regions": {"r": [0, 0, 1, 1]},
                                         "rules": [{"region": "r", "glob": "*"}],
                                         "propagate_ff_di_driver": "false"})
except RegionSpecError:
    pass
else:
    raise AssertionError("non-boolean propagate_ff_di_driver was accepted")

print("region config tests: PASS")


# The physical A/B candidates intentionally constrain only the memory macros.
# Check their exact hierarchy globs and the legal ECP5 bounds without requiring
# a nextpnr Python context.
candidate_names = [
    "region_memory_overlap.json",
    "region_memory_left.json",
    "region_memory_right.json",
]
candidate_cells = [
    "bram_hardware_memory.g_bank[0].mem_low_0",
    "bram_core.branch_predictor.tage.base_memory",
    "bram_core.registers.bank_a_0",
]
for candidate_name in candidate_names:
    candidate = load(ROOT / candidate_name)
    candidate = __import__("region_spec").validate(candidate, (0, 0, 126, 95))
    assignments, matches = expand_exact_members(candidate_cells, candidate["rules"])
    assert len(matches) == 3
    assert len(assignments) == 3
    assert all(not name.startswith("ff_") for name in assignments)
    assert candidate["propagate_ff_di_driver"] is False
    assert all(rule["cell_type"] == "DP16KD" for rule in candidate["rules"])
    typed_cells = {name: "DP16KD" for name in candidate_cells}
    expand_exact_members(candidate_cells, candidate["rules"], typed_cells)
    try:
        expand_exact_members(
            candidate_cells,
            candidate["rules"],
            {name: "TRELLIS_FF" for name in candidate_cells},
        )
    except RegionSpecError:
        pass
    else:
        raise AssertionError("typed region glob accepted a wrong cell type")

print("region candidate tests: PASS")
