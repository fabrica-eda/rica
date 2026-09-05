#!/usr/bin/env python3
"""Compare native JTCK STA with the independently constructed supplemental graph.

Both passes use the saved routed delays and the existing Texo STA engine.
This checks model integration and the board's JTAG structure, not independent
delay characterization or reset/CDC timing sign-off.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("--architecture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--supplemental-binary", type=Path,
                        default=ROOT / "target/jtag-route-probe/release/jtag-sta")
    args = parser.parse_args()
    supplemental = args.output.with_suffix(".supplemental.json")
    require(not args.output.exists() and not supplemental.exists(), "refusing to overwrite a saved audit")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    checkpoint_sha = digest(args.checkpoint)
    cp = json.loads(args.checkpoint.read_text())
    timing = cp["timing"]
    reserve = timing.get("setup_uncertainty_ps", 0)
    require(all(e["uncertainty_ps"] == reserve for e in timing["setup_checks"]),
            "native setup checks did not preserve the declared margin")
    require(timing["meets_timing_closure"] and timing["met_timing"], "native timing closure failed")
    require(timing["worst_slack_ps"] >= 0 and timing["worst_hold_slack_ps"] >= 0, "native setup/hold failed")
    require(timing["clock_constraints"] == [{"cell": "jtagg", "pin": "JTCK", "period_ps": 166666}],
            "expected explicit 6 MHz JTCK source constraint")
    require(sorted(c["period_ps"] for c in cp["packing"]["generated_clock_periods_ps"]) == [8013, 16026, 166666],
            "expected fixed 124.8/62.4 MHz CPU/memory and 6 MHz JTCK")
    expected = {
        ("ff_board.jtag_debug_transport.shift_high[63]", "DI", "no_synchronous_launch"),
        ("ff_board.jtag_debug_transport.shift_enable_q", "DI", "no_synchronous_launch"),
        ("ff_board.jtag_debug_transport.update_q", "DI", "no_synchronous_launch"),
        ("ff_board.jtag_debug_transport.request_toggle_meta_q", "DI", "no_synchronous_launch"),
    }
    unchecked = timing["unchecked_endpoints"]
    require(len(unchecked) == 4 and {(e["cell"], e["data_pin"], e["reason"]) for e in unchecked} == expected,
            "native STA has an unexpected unchecked endpoint")
    exceptions = timing["coverage_exceptions"]
    require(len(exceptions) == 4 and {(e["cell"], e["data_pin"], e["reason"]) for e in exceptions} == expected,
            "native STA exceptions do not exactly match the four reviewed boundaries")
    require(all(e["justification"].strip() for e in exceptions), "empty boundary justification")
    boundaries = timing["unmodeled_boundaries"]
    require(len(boundaries) == 1 and boundaries[0]["cell"] == "jtagg"
            and boundaries[0]["reason"] == "launch_capture_not_characterized",
            "JTAGG characterization limitation must remain explicit")

    delays = timing["net_delays"]
    sources = {d["net_id"] for d in delays if d["driver_cell"] == "jtagg" and d["driver_pin"] == "JTCK"}
    require(len(sources) == 1, "expected exactly one JTCK source")
    globals_ = [c["global_net"] for c in cp["packing"]["global_clocks"] if c["source_net"] in sources]
    require(len(globals_) == 1, "expected one promoted JTCK domain")
    pin_names = {d["sink_pin_id"]: d["sink_pin"] for d in delays}
    command = [str(args.supplemental_binary.resolve()), str(args.architecture.resolve()),
               str(args.checkpoint.resolve()), str(supplemental.resolve())]
    result = subprocess.run(command, text=True, capture_output=True)
    audit = json.loads(supplemental.read_text())
    require(result.returncode == 0 and audit["structural_and_internal_timing_gate"],
            "supplemental board structure/STA failed: " + result.stdout + result.stderr)

    counts = {}
    for kind in ("setup_checks", "hold_checks"):
        native = [e for e in timing[kind] if e["clock_net_id"] == globals_[0]]
        describe = lambda e: (e["slack_ps"], e["launch_edge"], e["capture_edge"])
        # The historical supplemental engine has zero setup uncertainty.
        # Compare nominal slacks while separately requiring the guarded native
        # result to pass. Hold has no adjustment.
        native_map = {(e["cell"], pin_names[e["data_pin_id"]]):
                      (e["slack_ps"] + (reserve if kind == "setup_checks" else 0),
                       e["launch_edge"], e["capture_edge"]) for e in native}
        supplement_map = {(e["endpoint"]["cell"], e["endpoint"]["pin"]): describe(e) for e in audit[kind]}
        require(len(native) == len(native_map) == len(supplement_map) == 515,
                f"unexpected JTCK endpoint count in {kind}")
        require(native_map == supplement_map, f"native/supplemental {kind} differ")
        counts[kind] = len(native)
    require(digest(args.checkpoint) == checkpoint_sha, "checkpoint changed during audit")
    output = {
        "checkpoint": str(args.checkpoint.resolve()), "checkpoint_sha256": checkpoint_sha,
        "native_setup_ps": timing["worst_slack_ps"], "native_hold_ps": timing["worst_hold_slack_ps"],
        "setup_uncertainty_ps": reserve,
        "native_nominal_setup_ps": timing["worst_slack_ps"] + reserve,
        "jtck_setup_ps": audit["worst_setup_slack_ps"], "jtck_hold_ps": audit["worst_hold_slack_ps"],
        "native_checks": {kind: len(timing[kind]) for kind in counts}, "jtck_checks": counts,
        "native_unchecked": unchecked, "supplemental_command": command,
        "supplemental_binary_sha256": digest(args.supplemental_binary),
        "supplemental_report_sha256": digest(supplemental),
        "jtck_nominal_slacks_and_edges_match": True, "structural_and_internal_timing_gate": True,
        "external_boundaries_not_characterized": audit["external_boundaries_not_characterized"],
    }
    args.output.write_text(json.dumps(output, indent=2) + "\n")
    print(f"native/supplemental JTCK nominal match: 515 setup, 515 hold; native WNS={timing['worst_slack_ps']} ps, reserve={reserve} ps")


if __name__ == "__main__":
    main()
