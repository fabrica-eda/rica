#!/usr/bin/env python3
"""Diagnostic clock-edge ECO: freeze all routing and CPU/memory configuration."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = ROOT / "toolchain/measurements/2026-09-06-rv64-jtag-zero"
BASE = ROOT / "riscv/build/riscv-hardware-compact-4k-124p8-20260906.bit"


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--scope", choices=["all", "shift", "control"], default="all")
    args = parser.parse_args()
    assert sha(BASE) == "f76f09071540e8ffcf637e53f48b061324fcdcb9b87f1fae2ea09afb3af3473e"
    checkpoint = ROOT / "riscv/build/compact-predictor/compact64-token-next.texo.json"
    cp = json.loads(checkpoint.read_text())
    original = BASE.with_suffix(".bit.config").read_text()
    parts = original.split("\n.tile ")
    tiles = {part.split("\n", 1)[0]: i for i, part in enumerate(parts[1:], 1)}
    controls = {}
    selected = []
    for c in cp["placement"]:
        if c["bel_type"] != "TRELLIS_FF":
            continue
        pin = next(p for p in c["bel_pins"] if p["name"] == "CLK")
        local = re.fullmatch(r"R\d+C\d+/CLK(\d+)_SLICE", pin["wire"])[1]
        tile = c["configuration_tiles"][0]["name"]
        source = re.search(r"^arc: MUXCLK" + local + r" (CLK\d+)$", parts[tiles[tile]], re.M)[1]
        key = (tile, source)
        controls.setdefault(key, []).append(c["cell"])
        name = c["cell"].removeprefix("ff_board.jtag_debug_transport.")
        shift = name.startswith(("shift_low[", "shift_high["))
        control = name.startswith("request_toggle_tap_") or name == "last_chain_er1"
        if (shift and args.scope != "control") or (control and args.scope != "shift"):
            selected.append((c["cell"], key))
    assert len(selected) == {"all": 132, "shift": 128, "control": 4}[args.scope]
    names = {name for name, _ in selected}
    keys = {key for _, key in selected}
    changes = []
    for tile, source in sorted(keys):
        assert set(controls[(tile, source)]) <= names, controls[(tile, source)]
        old = "enum: " + source + ".CLKMUX CLK"
        new = "enum: " + source + ".CLKMUX INV"
        i = tiles[tile]
        occurrences = parts[i].splitlines().count(old)
        assert occurrences >= 1, (tile, source)
        parts[i] = parts[i].replace(old + "\n", new + "\n")
        changes.append(dict(tile=tile, before=old, after=new, occurrences=occurrences, cells=controls[(tile, source)]))
    out = ROOT / "riscv/build/jtag-zero" / args.label
    config, bit = out.with_suffix(".config"), out.with_suffix(".bit")
    record_path = EVIDENCE / (args.label + ".eco.json")
    assert not any(p.exists() for p in [config, bit, record_path])
    config.write_text("\n.tile ".join(parts))
    before, after = original.splitlines(), config.read_text().splitlines()
    assert len(before) == len(after)
    assert sum(a != b for a, b in zip(before, after)) == sum(c["occurrences"] for c in changes)
    cmd = ["ecppack", str(config), str(bit), "--db", "/usr/share/trellis/database"]
    subprocess.run(cmd, check=True)
    record = dict(purpose="diagnostic only; invert selected JTCK registers; no full STA qualification",
                  scope=args.scope, changed_registers=len(selected),
                  original_bitstream_sha256=sha(BASE), checkpoint_sha256=sha(checkpoint),
                  config_sha256=sha(config), bitstream=str(bit.relative_to(ROOT)),
                  bitstream_sha256=sha(bit), script_sha256=sha(Path(__file__)),
                  routing_and_cpu_memory_configuration_unchanged=True,
                  changed_clock_muxes=len(changes), changes=changes, pack_command=cmd)
    record_path.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({k: v for k, v in record.items() if k != "changes"}, indent=2))


if __name__ == "__main__":
    main()
