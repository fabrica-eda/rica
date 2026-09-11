#!/usr/bin/env python3
"""RV64 board bitgen with CPU/memory and explicit JTCK-domain preflight.

This adds a project gate around Texo 7b372da; it does not claim characterized
JTAGG hard-macro timing, reset recovery/removal, or physical qualification.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "riscv/tools/jtag-sta/Cargo.toml"


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("checkpoint", type=Path)
    p.add_argument("--bit", type=Path, required=True)
    p.add_argument("--audit-report", type=Path, required=True)
    p.add_argument("--architecture", type=Path, required=True)
    args = p.parse_args()
    require(not args.bit.exists(), "refusing to overwrite an existing bitstream")
    require(not args.audit_report.exists(), "refusing to overwrite an existing audit")
    checkpoint_hash = hashlib.sha256(args.checkpoint.read_bytes()).hexdigest()
    checkpoint = json.loads(args.checkpoint.read_text())
    timing = checkpoint["timing"]
    require(timing["met_timing"] and timing["worst_slack_ps"] >= 0 and timing["worst_hold_slack_ps"] >= 0, "CPU/memory STA failed")
    require(sorted(c["period_ps"] for c in checkpoint["packing"]["generated_clock_periods_ps"]) == [8013, 16026], "expected fixed 124.8/62.4 MHz clocks")
    args.audit_report.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cargo", "build", "--locked", "--offline", "--release", "--manifest-path", str(MANIFEST),
                    "--target-dir", str(ROOT / "target/jtag-route-probe"), "--bin", "jtag-sta"], check=True, cwd=ROOT)
    audit_tool = ROOT / "target/jtag-route-probe/release/jtag-sta"
    command = [str(audit_tool), str(args.architecture.resolve()), str(args.checkpoint.resolve()), str(args.audit_report.resolve())]
    result = subprocess.run(command)
    audit = json.loads(args.audit_report.read_text())
    audit.update(checkpoint_sha256=checkpoint_hash, audit_binary_sha256=hashlib.sha256(audit_tool.read_bytes()).hexdigest(),
                 audit_command=command, audit_exit_code=result.returncode,
                 cpu_memory_setup_slack_ps=timing["worst_slack_ps"], cpu_memory_hold_slack_ps=timing["worst_hold_slack_ps"])
    args.audit_report.write_text(json.dumps(audit, indent=2) + "\n")
    if result.returncode or not audit["structural_and_internal_timing_gate"]:
        raise SystemExit("JTCK preflight failed; bitgen was not run")
    # Every unconstrained endpoint in the main report must be covered by the
    # supplemental audit. A newly unconstrained CPU path must never slip through.
    pin_names = {d["sink_pin_id"]: d["sink_pin"] for d in timing["net_delays"]}
    main_unchecked = {(e["cell"], pin_names[e["data_pin_id"]], e["reason"])
                      for e in timing["unchecked_endpoints"]}
    covered = {(e["endpoint"]["cell"], e["endpoint"]["pin"], "unconstrained_clock")
               for e in audit["setup_checks"] + audit["unchecked_endpoints"]}
    covered.add(("ff_board.jtag_debug_transport.request_toggle_meta_q", "DI", "no_synchronous_launch"))
    require(main_unchecked == covered and len(main_unchecked) == len(timing["unchecked_endpoints"]),
            "unreviewed endpoint in main STA; bitgen was not run")
    audit["main_sta_unchecked_scope_ok"] = True
    require(hashlib.sha256(args.checkpoint.read_bytes()).hexdigest() == checkpoint_hash, "checkpoint changed during audit")
    subprocess.run([str(ROOT / "target/texo-git/bin/texo"), "bitgen", str(args.checkpoint.resolve()),
                    "--bit", str(args.bit.resolve())], check=True, cwd=ROOT)
    audit["bitstream_sha256"] = hashlib.sha256(args.bit.read_bytes()).hexdigest()
    audit["physical_qualification"] = "required separately for this bitstream hash"
    args.audit_report.write_text(json.dumps(audit, indent=2) + "\n")
    print("CPU/memory and JTCK internal timing passed; hard-macro/reset boundaries remain outside this model.")


if __name__ == "__main__":
    main()
