"""Recreate the facade from a patched checkout at the pinned revision.

Usage: refresh.py /path/to/patched/celox [--check]
--check compares source bytes and manifest semantics without writing files.
"""
from pathlib import Path
import argparse, hashlib, json, re, shutil, subprocess, tomllib

REV = "7ff0dd2b8c4a92efd9a1aabe376240c6dff3f9c9"
URL = "https://github.com/celox-sim/celox.git"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = args.source.resolve()
destination = Path(__file__).resolve().parent
assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip() == REV
workspace = tomllib.loads((source / "Cargo.toml").read_text())["workspace"]

def value(item):
    if isinstance(item, dict):
        return "{ " + ", ".join(json.dumps(k) + " = " + value(v) for k, v in item.items()) + " }"
    if isinstance(item, list):
        return "[" + ", ".join(map(value, item)) + "]"
    return json.dumps(item)

lines = []
for line in (source / "crates/celox/Cargo.toml").read_text().splitlines():
    inherited = re.match(r"([\w-]+)\.workspace\s*=\s*true$", line)
    dependency = re.match(r"([\w-]+)\s*=\s*(\{.*workspace\s*=\s*true.*\})$", line)
    if inherited:
        key = inherited[1]
        item = workspace["package"].get(key, workspace["dependencies"].get(key))
        line = key + " = " + value(item)
    elif dependency:
        key = dependency[1]
        item = workspace["dependencies"][key]
        item = {"version": item} if isinstance(item, str) else dict(item)
        if key.startswith("celox"):
            item.pop("path", None)
            item.update(git=URL, rev=REV)
        extra = tomllib.loads("x = " + dependency[2])["x"]
        extra.pop("workspace")
        item.update(extra)
        line = key + " = " + value(item)
    lines.append(line)
manifest = "\n".join(lines) + "\n"
manifest = re.sub(r"(?ms)^\[\[(?:bench|test|example)\]\].*?(?=^\[|\Z)", "", manifest)
lints = "\n\n".join("[lints." + kind + "]\n" + "\n".join(k + " = " + value(v) for k, v in rules.items()) for kind, rules in workspace["lints"].items())
manifest = manifest.replace("[lints]\nworkspace = true", lints)
assert "workspace = true" not in manifest and ".workspace" not in manifest
files = {str(p.relative_to(source / "crates")): p.read_bytes() for p in (source / "crates/celox/src").rglob("*") if p.is_file()}
files["celox/README.md"] = (source / "crates/celox/README.md").read_bytes()
files["celox/Cargo.toml"] = manifest.encode()
for name in ["LICENSE-MIT", "LICENSE-APACHE"]:
    files[name] = (source / name).read_bytes()
for name, data in files.items():
    path = destination / name
    if args.check:
        if name.endswith("Cargo.toml"):
            assert tomllib.loads(path.read_text()) == tomllib.loads(data.decode()), name
        else:
            assert path.read_bytes() == data, name
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
if not args.check:
    hashes = {name: hashlib.sha256((destination / name).read_bytes()).hexdigest() for name in sorted(files) if name.startswith("celox/")}
    (destination / "sources.json").write_text(json.dumps({"revision": REV, "source_changes": ["celox/src/simulator/builder.rs"], "sha256": hashes}, indent=2) + "\n")
print("Checked" if args.check else "Refreshed", len(files), "files")
