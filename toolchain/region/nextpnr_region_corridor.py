"""Apply a broad, overlapping ECP5 corridor experiment in nextpnr.

This hook is deliberately a soft-region prototype.  It does not pin BELs and
it does not create disjoint CPU/Memory halves.  It expands hierarchy globs to
exact packed cell names, checks rectangle bounds and legal resource capacity,
and propagates a named FF's DI-driver LUT into the same region so a packed
LUT/FF unit is not silently split.  Any unmatched glob, split assignment, or
over-constrained resource fails before placement.

The hook is run by nextpnr's embedded Python interpreter.  The pure helper
module next to this file is also exercised by test_region_config.py.
"""

import collections
import os
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
if SCRIPT_DIR not in sys.path:
    sys.path.insert(0, SCRIPT_DIR)

from region_spec import (  # noqa: E402
    RegionSpecError,
    expand_exact_members,
    load,
    merge_assignment,
    required_capacity,
    validate,
)


def fail(message):
    print("REGION_ERROR", message)
    raise SystemExit(2)


config_path = os.environ.get(
    "RICA_REGION_CONFIG", os.path.join(SCRIPT_DIR, "region_corridor.json"))
try:
    with open(config_path, "r", encoding="utf-8") as stream:
        raw_config = __import__("json").load(stream)
except Exception as error:
    fail("cannot read config=%s: %r" % (config_path, error))

cells = {}
for key_value in ctx.cells:
    cells[str(key_value.first)] = key_value.second
if not cells:
    fail("empty packed cell map")

bel_locations = []
for bel in ctx.getBels():
    location = ctx.getBelLocation(bel)
    bel_locations.append((location.x, location.y))
if not bel_locations:
    fail("empty BEL database")
device_bounds = (
    min(x for x, _ in bel_locations),
    min(y for _, y in bel_locations),
    max(x for x, _ in bel_locations),
    max(y for _, y in bel_locations),
)

try:
    config = validate(raw_config, device_bounds)
except RegionSpecError as error:
    fail(str(error))

for name, rectangle in config["regions"].items():
    ctx.createRectangularRegion(name, *rectangle)


def cell_type(cell):
    return str(cell.type)


try:
    assignments, matches = expand_exact_members(
        cells.keys(), config["rules"], {name: cell_type(cell) for name, cell in cells.items()})
except RegionSpecError as error:
    fail(str(error))


def di_driver(cell):
    """Return a packed LUT driving an FF's DI, if one is visible."""

    for key_value in cell.ports:
        if str(key_value.first) != "DI" or key_value.second.net is None:
            continue
        driver = key_value.second.net.driver.cell
        if driver is not None and cell_type(driver) == "TRELLIS_COMB":
            return driver
    return None


direct_counts = collections.Counter()
propagated_counts = collections.Counter()
for name, region in list(assignments.items()):
    direct_counts[(region, cell_type(cells[name]))] += 1
    if not config["propagate_ff_di_driver"]:
        continue
    driver = di_driver(cells[name]) if cell_type(cells[name]) == "TRELLIS_FF" else None
    if driver is None:
        continue
    driver_name = str(driver.name)
    was_new = driver_name not in assignments
    try:
        merge_assignment(assignments, driver_name, region, "DI of %s" % name)
    except RegionSpecError as error:
        fail(str(error))
    if was_new:
        propagated_counts[(region, cell_type(driver))] += 1

cell_types = {name: cell_type(cell) for name, cell in cells.items()}
legal_sites = collections.Counter()
for region, rectangle in config["regions"].items():
    for bel in ctx.getBels():
        location = ctx.getBelLocation(bel)
        if rectangle[0] <= location.x <= rectangle[2] and rectangle[1] <= location.y <= rectangle[3]:
            legal_sites[(region, str(ctx.getBelType(bel)))] += 1
try:
    needed, capacity = required_capacity(assignments, cell_types, legal_sites)
except RegionSpecError as error:
    fail(str(error))

for name, region in sorted(assignments.items()):
    try:
        ctx.constrainCellToRegion(name, region)
    except Exception as error:
        fail("constraint cell=%s region=%s exception=%r" % (name, region, error))

print("REGION_DEVICE_BOUNDS", device_bounds)
print("REGION_RECTANGLES", config["regions"])
print("REGION_MATCHES", {pattern: len(names) for pattern, names in matches.items()})
print("REGION_DIRECT_COUNTS", dict(direct_counts))
print("REGION_PROPAGATED_COUNTS", dict(propagated_counts))
print("REGION_NEEDED", needed)
print("REGION_CAPACITY", capacity)
print("REGION_ASSIGNMENTS", len(assignments))
print("REGION_ERRORS", 0)
