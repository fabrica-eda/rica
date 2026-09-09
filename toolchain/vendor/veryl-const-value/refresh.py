"""Recreate the patched crate from the original veryl-analyzer 0.21.0 source.

Usage: refresh.py /path/to/veryl-analyzer-0.21.0 [--check]
The input directory is read only; the patch is applied in a temporary directory.
"""
from pathlib import Path
import argparse, hashlib, json, shutil, subprocess, tempfile, tomllib
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = args.source.resolve()
destination = Path(__file__).resolve().parent
package = tomllib.loads((source / "Cargo.toml").read_text())["package"]
assert package["name"] == "veryl-analyzer" and package["version"] == "0.21.0"
with tempfile.TemporaryDirectory(prefix="rica-veryl-const-") as temporary:
    staged = Path(temporary)
    shutil.copytree(source / "src", staged / "src")
    for name in ["Cargo.toml", "README.md"]:
        shutil.copy2(source / name, staged / name)
    subprocess.run(["git", "apply", "-p3", str(destination / "fix.patch")], cwd=staged, check=True)
    hashes = {}
    for path in sorted(staged.rglob("*")):
        if not path.is_file():
            continue
        relative = Path("veryl-analyzer") / path.relative_to(staged)
        target = destination / relative
        if args.check:
            assert target.read_bytes() == path.read_bytes(), relative
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
        hashes[str(relative)] = hashlib.sha256(path.read_bytes()).hexdigest()
    if not args.check:
        (destination / "sources.json").write_text(json.dumps({"revision": "2e32d53f056eef323225a91079af92574d18f726", "source_changes": ["veryl-analyzer/src/conv/utils.rs"], "sha256": hashes}, indent=2) + "\n")
    print("Checked" if args.check else "Refreshed", len(hashes), "files")
