# Archived Celox memory and FF patch

Rica now uses upstream commit
`4c2c227ebfb908e294726e1af80421d976bc69fd`, the merged
[Celox PR #755](https://github.com/celox-sim/celox/pull/755).
The four local Cargo overrides and copied crate directories have been removed.
The upstream tree exactly matches the tested PR head; all 123 previously
vendored source files match that tree. Struo's separate Celox 0.3.1 dependency
and optimizer backport remain unchanged.

The [upstream adoption record](../../measurements/2026-09-06-celox-upstream/README.md)
contains the final CI status, dependency/source comparison and downstream
validation with no local Celox 0.4.5 patches.

## Retained history

The files here reproduce the earlier local implementation on official Celox
0.4.5 `9aa2457811b3ef2b002e90d2ae4f5aafb0195099`:

- [fixes.patch](fixes.patch): complete source/test diff submitted upstream.
- [sources.json](sources.json): SHA-256 of the former 127 copied files, including
  four manifests that expanded upstream workspace inheritance.
- [refresh.py](refresh.py): helper to reconstruct those copies from the patched
  original revision. It does not activate Cargo overrides in the current tree.
- The upstream MIT and Apache-2.0 licenses.

The complete local dependency setup is retained in Rica commit `6eee995`.
Earlier sealed measurement records describe that historical setup and have
not been rewritten to describe the subsequent upstream adoption.

| Component | Correction |
| --- | --- |
| FF event assembly | Preserve pre-edge reads across reset groups sharing a clock; schedule safe direct writes and retain a merged evaluation/publication fallback. |
| WASM shifted stores | Account for destination coverage, including spill beyond 64 bits. |
| Native padded arrays | Split aggregate accesses at logical element boundaries before instruction selection. |
| Native StateSSA | Promote fully defined Working-state joins after seed DSE and normalize mixed Bit/Logic values, while rejecting undefined paths. |

The [correctness record](../../measurements/2026-09-06-celox-memory-fix/README.md)
retains independent negative controls for the three original defects. The
[FF performance record](../../measurements/2026-09-06-celox-ff-performance/README.md)
compares the optimization against the correct two-phase implementation.
Memory debug writes improved by about 3–5% in those measurements; CPU-wide
simulation speedup was not established. These are not FPGA clock or IPC gains.

## Reproduce the historical patch

In an isolated checkout of `9aa2457811b3ef2b002e90d2ae4f5aafb0195099`:

```sh
git apply /path/to/rica/toolchain/vendor/celox/fixes.patch
cargo +1.98.0 test -p celox --test ff_event_snapshot --test ff_narrow_arrays
cargo +1.98.0 test -p celox-backend-x86 -p celox-backend-wasm -p celox-frontend-core -p celox-sir-opt
```

To reconstruct the old vendored setup, use an isolated Rica checkout of
`6eee995`, then run its refresh helper against that patched Celox checkout.
Running the helper in the current Rica tree only recreates unused directories.

The two SV four-state FF instances use the frontend's existing exclusion for
unsupported four-state event signals. All four Celox execution backends remain
covered. The mixed-X 64-bit case uses explicit expected values on those four
backends because the separate Veryl reference simulator disagrees on known
payload bits. See the [PR submission record](../../measurements/2026-09-06-celox-pr/README.md)
for the original test scope and logs.
