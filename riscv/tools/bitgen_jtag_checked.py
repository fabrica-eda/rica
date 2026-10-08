#!/usr/bin/env python3
"""RV64 board bitgen with CPU/memory and explicit JTCK-domain preflight.

This adds a project gate around the workspace-pinned Texo; it does not claim characterized
JTAGG hard-macro timing, reset recovery/removal, or physical qualification.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from checkpoint_io import load_checkpoint
from user_platform import check_checkpoint_clocks
from check_integrated_jtck_sta import default_measured_library

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "riscv/tools/jtag-sta/Cargo.toml"


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def prepare_bitgen_runtime(runtime, architecture, destination, device):
    """Keep the installed codec/IO layout while selecting the audited architecture."""
    manifest = json.loads((runtime / 'manifest.json').read_text())
    require(manifest['device'] == device, 'bitgen runtime device differs from checkpoint')
    require(not destination.exists(), 'refusing to overwrite a saved bitgen runtime')
    destination.mkdir(parents=True)
    for name in ['base.config', 'iodb.json', 'database', 'bin', 'lib']:
        source = runtime / name
        require(source.exists(), f'missing bitgen runtime component: {source}')
        (destination / name).symlink_to(source, target_is_directory=source.is_dir())
    (destination / 'architecture.txdb').symlink_to(architecture)
    with architecture.open('rb') as stream:
        architecture_hash = hashlib.file_digest(stream, 'sha256').hexdigest()
    manifest['architecture'] = {'bytes': architecture.stat().st_size, 'sha256': architecture_hash}
    manifest['artifact_stem'] = 'rica-audited-' + architecture_hash[:16]
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return architecture_hash


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("checkpoint", type=Path)
    p.add_argument("--bit", type=Path, required=True)
    p.add_argument("--audit-report", type=Path, required=True)
    p.add_argument("--architecture", type=Path, required=True)
    p.add_argument("--bitgen-runtime", type=Path, required=True,
                   help="Installed target pack providing database/, base.config and bin/ecppack")
    p.add_argument("--platform-profile", type=Path)
    p.add_argument("--measured-timing-library", type=Path,
                   help="Measured STA library of the checkpoint; defaults to the qualified one in riscv/assets")
    args = p.parse_args()
    require(not args.bit.exists(), "refusing to overwrite an existing bitstream")
    require(not args.audit_report.exists(), "refusing to overwrite an existing audit")
    checkpoint_hash = hashlib.sha256(args.checkpoint.read_bytes()).hexdigest()
    checkpoint = load_checkpoint(args.checkpoint)
    runtime = args.bitgen_runtime.resolve()
    # The audited architecture must also be used for bitgen. Auto-resolving a
    # different target architecture can change the timing-table surface. Keep
    # the pack's separate IO database and shared-library layout intact.
    selected_runtime = args.audit_report.resolve().with_suffix('.runtime')
    architecture_hash = prepare_bitgen_runtime(
        runtime, args.architecture.resolve(), selected_runtime, checkpoint['target']['device'])
    bitgen_command = [str(ROOT / 'riscv/tools/texo'), 'bitgen', str(args.checkpoint.resolve()),
                      '--target-pack', str(selected_runtime),
                      '--bit', str(args.bit.resolve())]
    library = args.measured_timing_library or default_measured_library()
    if checkpoint['target'].get('measured_timing'):
        bitgen_command += ['--measured-timing-library', str(library.resolve())]
    timing = checkpoint["timing"]
    require(timing["met_timing"] and timing["worst_slack_ps"] >= 0 and timing["worst_hold_slack_ps"] >= 0, "CPU/memory STA failed")
    if args.platform_profile:
        check_checkpoint_clocks(checkpoint, args.platform_profile)
    else:
        require(sorted(c["period_ps"] for c in checkpoint["packing"]["generated_clock_periods_ps"]) == [8013, 16026], "expected fixed 124.8/62.4 MHz clocks")
    args.audit_report.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["nix", "develop", "--command", "cargo", "build", "--locked", "--offline", "--release", "--manifest-path", str(MANIFEST),
                    "--target-dir", str(ROOT / "target"), "--bin", "jtag-sta"], check=True, cwd=ROOT)
    if args.platform_profile:
        profile_hash = hashlib.sha256(args.platform_profile.read_bytes()).hexdigest()
        # Current platforms constrain JTCK in the main STA. Use the full
        # integrated boundary audit instead of the historical unconstrained path.
        subprocess.run([
            "nix", "develop", "--command", "python3",
            str(ROOT / 'riscv/tools/check_integrated_jtck_sta.py'),
            str(args.checkpoint.resolve()), '--architecture', str(args.architecture.resolve()),
            '--output', str(args.audit_report.resolve()),
            '--platform-profile', str(args.platform_profile.resolve()),
            '--measured-timing-library', str(library.resolve()),
        ], check=True, cwd=ROOT)
        require(hashlib.sha256(args.checkpoint.read_bytes()).hexdigest() == checkpoint_hash,
                'checkpoint changed during integrated audit')
        require(hashlib.sha256(args.platform_profile.read_bytes()).hexdigest() == profile_hash,
                'platform profile changed during integrated audit')
        subprocess.run(bitgen_command, check=True, cwd=ROOT)
        audit = json.loads(args.audit_report.read_text())
        audit.update(bitstream_sha256=hashlib.sha256(args.bit.read_bytes()).hexdigest(),
                     platform_profile_sha256=profile_hash,
                     architecture_sha256=architecture_hash,
                     bitgen_command=bitgen_command,
                     bitgen_runtime_manifest_sha256=hashlib.sha256((selected_runtime / 'manifest.json').read_bytes()).hexdigest(),
                     physical_qualification='required separately for this bitstream hash')
        args.audit_report.write_text(json.dumps(audit, indent=2) + '\n')
        return
    audit_tool = ROOT / "target/release/jtag-sta"
    command = [str(audit_tool), str(args.architecture.resolve()), str(args.checkpoint.resolve()),
               str(library.resolve()), str(args.audit_report.resolve())]
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
    subprocess.run(bitgen_command, check=True, cwd=ROOT)
    audit["bitgen_command"] = bitgen_command
    audit["bitstream_sha256"] = hashlib.sha256(args.bit.read_bytes()).hexdigest()
    audit["physical_qualification"] = "required separately for this bitstream hash"
    args.audit_report.write_text(json.dumps(audit, indent=2) + "\n")
    print("CPU/memory and JTCK internal timing passed; hard-macro/reset boundaries remain outside this model.")


if __name__ == "__main__":
    main()
