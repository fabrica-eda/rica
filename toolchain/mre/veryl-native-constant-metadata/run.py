#!/usr/bin/env python3
"""Run the native testbench in a fresh project and record exact observations."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--veryl", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--backend", choices=["interpret", "cranelift", "cc"], default="interpret")
args = parser.parse_args()
source = Path(__file__).resolve().parent
binary = args.veryl.resolve()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
project = output / "project"
project.mkdir()
shutil.copy2(source / "Veryl.toml", project / "Veryl.toml")
shutil.copytree(source / "src", project / "src")
environment = dict(os.environ)
environment["XDG_CACHE_HOME"] = str(output / "cache")
command = [str(binary), "test", "--backend", args.backend, "--no-capture", "--seed", "1"]
record = {"command": command, "cwd": str(project), "backend": args.backend,
          "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
          "binary_sha256": sha(binary), "start": time.time(),
          "source_sha256": {str(p.relative_to(project)): sha(p) for p in project.rglob("*") if p.is_file()}}
log = output / "run.log"
with log.open("w") as stream:
    result = subprocess.run(command, cwd=project, env=environment, stdout=stream, stderr=subprocess.STDOUT)
record.update(exit_code=result.returncode, seconds=time.time()-record["start"], log_sha256=sha(log),
              source_unchanged=all(sha(project/p)==h for p,h in record["source_sha256"].items()))
(output / "result.json").write_text(json.dumps(record, indent=2) + "\n")
print(log.read_text())
print(json.dumps(record, indent=2))
raise SystemExit(result.returncode)
