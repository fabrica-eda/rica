#!/usr/bin/env python3
"""Link the MRE to an already built Celox artifact without changing Cargo pins."""
import argparse, hashlib, json, subprocess
from pathlib import Path
root = Path(__file__).resolve().parents[3]
parser = argparse.ArgumentParser()
parser.add_argument("--rlib", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--expect-unfixed", action="store_true")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
source = Path(__file__).with_name("reproduce.rs")
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
binary = args.output / "parameter-repeat-mre"
build = ["rustc", "--edition=2024", "--crate-name", "parameter_repeat_mre", str(source),
         "-L", "dependency=" + str(root/"target/release/deps"), "--extern", "celox=" + str(args.rlib.resolve()), "-o", str(binary)]
record = {"build": build, "source_sha256": sha(source), "rlib_sha256": sha(args.rlib)}
log = args.output / "build.log"
assert not log.exists()
with log.open("w") as f: result = subprocess.run(build, cwd=root, stdout=f, stderr=subprocess.STDOUT)
record.update(build_exit_code=result.returncode, build_log_sha256=sha(log))
if result.returncode == 0:
    command = [str(binary)] + (["--expect-unfixed"] if args.expect_unfixed else [])
    log = args.output / "run.log"
    with log.open("w") as f: result = subprocess.run(command, cwd=root, stdout=f, stderr=subprocess.STDOUT)
    record.update(command=command, binary_sha256=sha(binary), exit_code=result.returncode, log_sha256=sha(log))
(args.output / "result.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps(record, indent=2))
raise SystemExit(result.returncode)
