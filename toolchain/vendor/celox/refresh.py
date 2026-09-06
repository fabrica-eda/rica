"""Refresh the patched crates from an isolated, patched v0.4.5 checkout.

Usage: python3 toolchain/vendor/celox/refresh.py /path/to/patched/celox
The upstream patch is kept separately in fixes.patch for review/reapplication.
"""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

REV = "9aa2457811b3ef2b002e90d2ae4f5aafb0195099"
URL = "https://github.com/celox-sim/celox.git"
CRATES = ("celox-frontend-core", "celox-backend-wasm", "celox-backend-x86", "celox-sir-opt")
source = Path(sys.argv[1]).resolve()
destination = Path(__file__).resolve().parent
assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip() == REV
workspace = tomllib.loads((source / "Cargo.toml").read_text())

def toml_value(value):
    if isinstance(value, dict):
        return "{ " + ", ".join(key + " = " + toml_value(item) for key, item in value.items()) + " }"
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    return json.dumps(value)

def standalone_manifest(path):
    text = path.read_text()
    for key, value in workspace["workspace"]["package"].items():
        text = re.sub(r"(?m)^" + re.escape(key) + r"\.workspace\s*=\s*true$", key + " = " + toml_value(value), text)
    for key, value in workspace["workspace"]["dependencies"].items():
        pattern = r"(?m)^" + re.escape(key) + r"\s*=\s*\{ workspace = true \}$"
        if key.startswith("celox"):
            value = dict(value)
            value.pop("path", None)
            value.update(git=URL, rev=REV)
        text = re.sub(pattern, key + " = " + toml_value(value), text)
    lints = "\n\n".join("[lints." + kind + "]\n" + "\n".join(key + " = " + toml_value(value) for key, value in rules.items())
                           for kind, rules in workspace["workspace"]["lints"].items())
    text = text.replace("[lints]\nworkspace = true", lints)
    assert "workspace = true" not in text and ".workspace" not in text
    return text

hashes = {}
for crate in CRATES:
    original = source / "crates" / crate
    vendored = destination / crate
    if vendored.exists():
        shutil.rmtree(vendored)
    shutil.copytree(original, vendored, ignore=shutil.ignore_patterns("target", "Cargo.lock"))
    (vendored / "Cargo.toml").write_text(standalone_manifest(original / "Cargo.toml"))
    for path in sorted(vendored.rglob("*")):
        if path.is_file():
            hashes[str(path.relative_to(destination))] = hashlib.sha256(path.read_bytes()).hexdigest()
for name in ("LICENSE-MIT", "LICENSE-APACHE"):
    shutil.copy2(source / name, destination / name)
(destination / "sources.json").write_text(json.dumps({"upstream": URL, "revision": REV, "sha256": hashes}, indent=2) + "\n")
