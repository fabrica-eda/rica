# Celox 0.4.5 memory correctness and FF optimization

Rica pins the official Celox 0.4.5 commit
`9aa2457811b3ef2b002e90d2ae4f5aafb0195099`. Updating from 0.4.2 alone
reproduced the same byte shift and WASM byte loss. The root Cargo manifest
patches four crates to the copies here. Every other Celox 0.4.5 crate uses
that same upstream revision; Struo's separate Celox 0.3.1 dependency and
`celox-sir-opt` backport are retained.

These local fixes are proposed upstream in
[Celox PR #755](https://github.com/celox-sim/celox/pull/755); they are not yet an
upstream release. The PR uses the same implementation on `master` `01acba6ec`.
Its two SV four-state test instances are excluded for the frontend's existing
unsupported FF event signals, following the existing four-state FF tests.
The four Celox execution backends remain covered. See the
[submission and validation record](../../measurements/2026-09-06-celox-pr/README.md).
[fixes.patch](fixes.patch) contains the complete upstream code/test diff.
[sources.json](sources.json) records the upstream revision and SHA-256 of each
vendored file. The manifests expand upstream workspace inheritance and point
Celox dependencies back to the pinned Git revision. The original MIT and
Apache-2.0 licenses are included.

| Crate | Defect | Fix |
| --- | --- | --- |
| `celox-frontend-core` | The ordinary tick path evaluates and publishes each reset group before evaluating the next group on the same clock. | Merge all evaluations followed by all applications into one execution unit for the shared event. |
| `celox-backend-wasm` | A scalar store shifted within a byte can span more than 64 destination bits; the small-store path truncates its spill. | Select that path by destination coverage, `bit_shift + op_width <= 64`. |
| `celox-backend-x86` | Late native optimization creates aggregate accesses across padded array elements after memory layout. | Split those static accesses at logical element boundaries before instruction selection. |
| `celox-sir-opt` | Working-state promotion rejects fully defined joins after seed DSE and joins between known reset constants and Logic values. | Let the existing StateSSA analysis prove that no value is live on entry; normalize mixed Bit/Logic stores to Logic with explicit zero masks for Bit inputs. |

The FF fix preserves pre-edge values independently of map/variable ordering.
Merging the phases into one optimizer unit is necessary: separate evaluation
units alone allow working-region staging stores to be removed as dead.
The already scheduled fused comb/FF path did not cause the observed shift.

The ordinary tick now also uses that existing dependency planner when the
frontend provides source-aware lowering and the event has multiple FF groups.
Readers run before writers where a direct write is safe. Cycles, dynamic RAM
addresses and aliased storage retain staging. The merged all-evaluate/all-apply
unit remains the fallback for other adapters; single-group events keep their
existing path. Scheduling happens during compilation, not on every tick.
The native StateSSA changes further replace eligible staging with register
values while keeping publication after every read of the old stable value.
Conditional paths with an undefined value still reject promotion.

[Performance measurements](../../measurements/2026-09-06-celox-ff-performance/README.md)
compare these changes against the correct, three-fix baseline. They measure
simulation time, not FPGA clock or CPU IPC.

The WASM regression checks both sides of the 64-bit boundary, surrounding
bits, static/packed-element offsets, and both value and unknown-mask planes.
Native regressions include one-bit arrays and padded three-bit arrays with
unknown masks. The frontend regression swaps two registers in separate reset
groups and checks both outputs against an explicit expected sequence.
The correctness-only baseline has independent negative controls for all three
fixes in the [original validation](../../measurements/2026-09-06-celox-memory-fix/README.md).
The later FF optimization has additional direct-update, four-state and
StateSSA regressions, including a negative control for the promotion change.

## Rebuild and remove this patch

Apply the patch in an isolated checkout of the exact revision above:

```sh
git apply /path/to/rica/toolchain/vendor/celox/fixes.patch
cargo +1.98.0 test -p celox --test ff_event_snapshot --test ff_narrow_arrays
cargo +1.98.0 test -p celox-backend-x86 -p celox-backend-wasm -p celox-frontend-core -p celox-sir-opt
```

After editing or reapplying the patch, refresh the distributed copies from
Rica's root with:

```sh
python3 toolchain/vendor/celox/refresh.py /path/to/patched/celox
```

Keep the patch and copied sources synchronized. An upstream version can
replace these copies once the memory matrix, Rica regressions and CPU replay
in the [fix report](../../mre/celox-parameterized-memory/README.md) pass with
all four Cargo patches removed, including the FF optimization regressions.
Recheck Struo's separate 0.3.1 pin rather
than removing its unrelated backport at the same time.
