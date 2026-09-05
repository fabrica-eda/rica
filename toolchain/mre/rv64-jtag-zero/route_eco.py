#!/usr/bin/env python3
"""Pack an isolated JTDI detour for causality testing, without claiming STA pass."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = ROOT / "toolchain/measurements/2026-09-06-rv64-jtag-zero"
BASE = ROOT / "riscv/build/riscv-hardware-compact-4k-124p8-20260906.bit"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def relative(tile, wire):
    owner = re.search(r"R(\d+)C(\d+):", tile)
    target = re.fullmatch(r"R(\d+)C(\d+)/(.+)", wire)
    oy, ox = map(int, owner.groups())
    y, x = map(int, target.groups()[:2])
    name = target[3]
    if name.startswith(("G_", "L_", "R_")):
        return name
    prefix = (f"N{oy-y}" if y < oy else f"S{y-oy}" if y > oy else "")
    prefix += f"W{ox-x}" if x < ox else f"E{x-ox}" if x > ox else ""
    return prefix + "_" + name if prefix else name


def arc(pip):
    return "arc: " + relative(pip["config_tile"], pip["to"]) + " " + relative(pip["config_tile"], pip["from"])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--label", required=True)
    p.add_argument("--extra-min-ps", required=True, type=int)
    args = p.parse_args()
    assert sha(BASE) == "f76f09071540e8ffcf637e53f48b061324fcdcb9b87f1fae2ea09afb3af3473e"
    candidates_path = EVIDENCE / "route-candidates.json"
    routes = json.loads(candidates_path.read_text())
    chosen = min(routes["candidates"], key=lambda c: abs(c["tail_min_ps"] - routes["old_tail_min_ps"] - args.extra_min_ps))
    original = BASE.with_suffix(".bit.config").read_text()
    parts = original.split("\n.tile ")
    tiles = {part.split("\n", 1)[0]: i for i, part in enumerate(parts[1:], 1)}
    changes = []
    for remove, pips in [(True, routes["removed_pips"]), (False, chosen["pips"])]:
        for pip in pips:
            if pip["fixed"]:
                continue
            tile = pip["config_tile"]
            if tile not in tiles:
                assert not remove
                tiles[tile] = len(parts)
                parts.append(tile + "\n")
            i = tiles[tile]
            line = arc(pip)
            lines = parts[i].splitlines()
            if remove:
                assert lines.count(line) == 1, (pip, line)
                lines.remove(line)
            else:
                sink = line.split()[1]
                assert not any(x.startswith("arc: " + sink + " ") for x in lines), (pip, line)
                lines.insert(1, line)
            parts[i] = "\n".join(lines) + "\n"
            changes.append(dict(remove=remove, tile=pip["config_tile"], arc=line))
    out = ROOT / "riscv/build/jtag-zero" / args.label
    out.parent.mkdir(parents=True, exist_ok=True)
    config, bit = out.with_suffix(".config"), out.with_suffix(".bit")
    record_path = EVIDENCE / (args.label + ".eco.json")
    assert not any(path.exists() for path in [config, bit, record_path])
    config.write_text("\n.tile ".join(parts))
    # Independently compare every nonempty configuration line by tile. Only the
    # explicitly requested routing mux arcs may differ; no enums/words change.
    def entries(text):
        tile = "header"
        result = set()
        for line in text.splitlines():
            if line.startswith(".tile "):
                tile = line[6:]
            elif line:
                result.add((tile, line))
        return result
    before, after = entries(original), entries(config.read_text())
    expected = set()
    for change in changes:
        key = (change["tile"], change["arc"])
        expected.symmetric_difference_update({key})
    assert before ^ after == expected
    cmd = ["ecppack", str(config), str(bit), "--db", "/usr/share/trellis/database"]
    subprocess.run(cmd, check=True)
    record = dict(purpose="diagnostic only; JTDI detour, not full STA qualification",
                  original_bitstream_sha256=sha(BASE), config_sha256=sha(config),
                  bitstream=str(bit.relative_to(ROOT)), bitstream_sha256=sha(bit),
                  candidates_sha256=sha(candidates_path), script_sha256=sha(Path(__file__)),
                  waypoint=chosen["waypoint"],
                  added_min_ps=chosen["tail_min_ps"]-routes["old_tail_min_ps"],
                  added_max_ps=chosen["tail_max_ps"]-routes["old_tail_max_ps"],
                  unchanged_configuration_except_listed_arcs=True,
                  changes=changes, pack_command=cmd)
    record_path.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
