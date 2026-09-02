"""Pure helpers for the nextpnr soft-region smoke configuration.

The embedded nextpnr hook imports this module, while the standalone test uses
the same validation and hierarchy-glob expansion code.  Keeping this part
free of nextpnr objects makes malformed configurations testable without an
FPGA database.
"""

import fnmatch
import json


class RegionSpecError(ValueError):
    """A region specification is empty, ambiguous, or physically invalid."""


def _rectangle(value, label):
    if not isinstance(value, (list, tuple)) or len(value) != 4:
        raise RegionSpecError("%s must be [x0, y0, x1, y1]" % label)
    if not all(isinstance(coordinate, int) and not isinstance(coordinate, bool)
               for coordinate in value):
        raise RegionSpecError("%s coordinates must be integers" % label)
    rectangle = tuple(value)
    if rectangle[0] > rectangle[2] or rectangle[1] > rectangle[3]:
        raise RegionSpecError("%s has inverted bounds: %s" % (label, rectangle))
    return rectangle


def load(path):
    with open(path, "r", encoding="utf-8") as stream:
        config = json.load(stream)
    return validate(config)


def validate(config, device_bounds=None):
    if not isinstance(config, dict) or config.get("schema") != 1:
        raise RegionSpecError("region config schema must be 1")

    raw_regions = config.get("regions")
    if not isinstance(raw_regions, dict) or not raw_regions:
        raise RegionSpecError("region config has no regions")

    regions = {}
    for name, entry in raw_regions.items():
        if not isinstance(name, str) or not name:
            raise RegionSpecError("region names must be non-empty strings")
        if isinstance(entry, dict):
            entry = entry.get("rect")
        regions[name] = _rectangle(entry, "region %s" % name)

    if device_bounds is not None:
        device_bounds = _rectangle(device_bounds, "device bounds")
        for name, rectangle in regions.items():
            if not (device_bounds[0] <= rectangle[0] <= rectangle[2] <= device_bounds[2]
                    and device_bounds[1] <= rectangle[1] <= rectangle[3] <= device_bounds[3]):
                raise RegionSpecError(
                    "region %s=%s is outside device bounds=%s" %
                    (name, rectangle, device_bounds))

    raw_rules = config.get("rules")
    if not isinstance(raw_rules, list) or not raw_rules:
        raise RegionSpecError("region config has no hierarchy rules")
    rules = []
    for index, rule in enumerate(raw_rules):
        if not isinstance(rule, dict):
            raise RegionSpecError("rule %d is not an object" % index)
        region = rule.get("region")
        pattern = rule.get("glob")
        if region not in regions:
            raise RegionSpecError("rule %d references unknown region %r" % (index, region))
        if not isinstance(pattern, str) or not pattern:
            raise RegionSpecError("rule %d has an empty glob" % index)
        cell_type = rule.get("cell_type")
        if cell_type is not None and (not isinstance(cell_type, str) or not cell_type):
            raise RegionSpecError("rule %d has an invalid cell_type" % index)
        normalized = {"region": region, "glob": pattern}
        if cell_type is not None:
            normalized["cell_type"] = cell_type
        rules.append(normalized)

    raw_overlap = config.get("require_overlap", [])
    if not isinstance(raw_overlap, list):
        raise RegionSpecError("require_overlap must be a list")
    for pair in raw_overlap:
        if not isinstance(pair, (list, tuple)) or len(pair) != 2:
            raise RegionSpecError("require_overlap entries must contain two region names")
        if pair[0] not in regions or pair[1] not in regions:
            raise RegionSpecError("require_overlap references an unknown region")
        if rectangle_intersection(regions[pair[0]], regions[pair[1]]) is None:
            raise RegionSpecError(
                "regions %s and %s do not overlap" % (pair[0], pair[1]))

    propagate_ff_di_driver = config.get("propagate_ff_di_driver", True)
    if not isinstance(propagate_ff_di_driver, bool):
        raise RegionSpecError("propagate_ff_di_driver must be boolean")

    return {
        "schema": 1,
        "regions": regions,
        "rules": rules,
        "require_overlap": [tuple(pair) for pair in raw_overlap],
        "propagate_ff_di_driver": propagate_ff_di_driver,
    }


def rectangle_intersection(left, right):
    result = (
        max(left[0], right[0]),
        max(left[1], right[1]),
        min(left[2], right[2]),
        min(left[3], right[3]),
    )
    if result[0] > result[2] or result[1] > result[3]:
        return None
    return result


def expand_exact_members(cell_names, rules, cell_types=None):
    """Expand hierarchy globs and reject unmatched or split assignments."""

    names = [str(name) for name in cell_names]
    assignments = {}
    matches = {}
    for rule in rules:
        region = rule["region"]
        pattern = rule["glob"]
        selected = sorted(name for name in names if fnmatch.fnmatchcase(name, pattern))
        if not selected:
            raise RegionSpecError(
                "unmatched hierarchy glob %r for region %r" % (pattern, region))
        expected_type = rule.get("cell_type")
        if expected_type is not None and cell_types is not None:
            missing_type = [name for name in selected if name not in cell_types]
            if missing_type:
                raise RegionSpecError(
                    "cell_types has no entry for glob %r (first: %s)" %
                    (pattern, missing_type[:5]))
            wrong_type = [
                name for name in selected if str(cell_types[name]) != expected_type
            ]
            if wrong_type:
                raise RegionSpecError(
                    "glob %r for region %r matched wrong cell_type=%s (first: %s)" %
                    (pattern, region, expected_type, wrong_type[:5]))
        matches[pattern] = selected
        for name in selected:
            previous = assignments.get(name)
            if previous is not None and previous != region:
                raise RegionSpecError(
                    "cluster-split cell %s assigned to %s and %s" %
                    (name, previous, region))
            assignments[name] = region
    return assignments, matches


def merge_assignment(assignments, name, region, source):
    previous = assignments.get(name)
    if previous is not None and previous != region:
        raise RegionSpecError(
            "cluster-split cell %s from %s assigned to %s and %s" %
            (name, source, previous, region))
    assignments[name] = region


def required_capacity(assignments, cell_types, legal_sites):
    """Return (needed, capacity) counters and reject over-constrained regions."""

    needed = {}
    for name, region in assignments.items():
        key = (region, str(cell_types[name]))
        needed[key] = needed.get(key, 0) + 1

    capacity = {}
    for key, count in needed.items():
        available = int(legal_sites.get(key, 0))
        capacity[key] = available
        if available < count:
            raise RegionSpecError(
                "illegal resource capacity region=%s type=%s cells=%d legal=%d" %
                (key[0], key[1], count, available))
    return needed, capacity
