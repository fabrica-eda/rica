# Toolchain issue catalog

This file tracks defects and sign-off limitations found while bringing up the
Rica designs. It is intentionally separate from
[`i2c/KNOWN_ISSUES.md`](i2c/KNOWN_ISSUES.md) and
[`riscv/KNOWN_ISSUES.md`](riscv/KNOWN_ISSUES.md): those files also contain RTL
and board-design findings, while this catalog records only tool behavior.

An entry is considered a tool defect only when an equivalent RTL/netlist
comparison, an independent flow, or a source-level diagnosis isolates the
tool. Unconfirmed observations remain excluded until a reproducer is retained.

| ID | Tool | Status | Affected revision | Impact |
| --- | --- | --- | --- | --- |
| STR-001 | Struo ECP5 mapper | [PR #79](https://github.com/fabrica-eda/struo/pull/79) open (`333eb38`) | `69e7ec13` through `5f30edc` | Wide-cover choice can multiply LUT/mux area discontinuously |
| STR-002 | Struo synthesis | Open; RTL workaround active | `69e7ec13` | Partial packed-register writes can expand recursively and fail to finish |
| STR-003 | Struo Veryl lowering | Fixed upstream/current pin | before `dc80755` | Module constants and dynamic packed selects failed to lower |
| STR-004 | Struo Veryl lowering | [PR #78](https://github.com/fabrica-eda/struo/pull/78) open (`7924cfa`) | `7917f11` through `5f30edc` | Top-level `always_comb` statements can violate blocking-assignment order |
| CEL-001 | Celox native x86 backend | Fixed upstream/current pin | before PR #712 | SSA reload materialization could exhaust the register allocator |
| TEX-001 | Texo ECP5 STA | Fixed upstream/current pin | before `bcc53c6` | Corner, clock-edge, and margin policy did not correlate with nextpnr |
| TEX-002 | Texo hold ECO | Fixed upstream/current pin | before `299836dd` | A pin-only FF rebinding could leave an invalid frozen route |
| TEX-003 | Texo implementation | Fixed upstream/current pin | before `522d7caf` | Successful P&R could emit a functionally wrong configuration |
| TEX-004 | Texo placer | Fixed upstream/current pin | before `ed36807` | Critical sinks could disappear from high-fanout placement edges |
| TEX-005 | Texo bitgen | Fixed upstream/current pin | before `a59f787` | Bitgen emitted a nonexistent `JTAG.MODE` enum |
| NPNR-001 | nextpnr timing reports | Local sign-off workaround | nextpnr 0.6 | Related generated-clock crossings need an explicit phase-aware check |

## STR-001 — Wide-cover selection can cause discontinuous area growth

**Observed.** With the same `BranchPredictor` RTL and fourteen inferred BRAMs,
changing only the mapper timing goal produced the following results:

| Goal | Mapped primitives |
| ---: | ---: |
| 125 MHz | 13,569 |
| 126 MHz | 29,053 |
| 133 MHz | 62,490 |

A second source variant produced 13,496 primitives at 121 MHz, 30,549 at
120 MHz, and 62,226 at 133 MHz. FF count remained approximately 3,800; LUT4,
PFUMX, and L6MUX21 accounted for the discontinuity. The registered TAGE
decision experiment exposed the same failure more directly: adding 661
lowered Boolean nodes selected a 67,973-primitive wide cover. Removing an
unnecessary same-edge RAS preview reduced it to 19,436 without changing the
fourteen BRAMs or register count.

`mapped.cells()` includes PFUMX and L6MUX cluster components, so 67,973 is not
the number of independent FPGA sites. It is nevertheless real expansion, not
only a reporting artifact: even the best possible wide-LUT packing requires
more than four times the baseline LUT4 resources.

**Root cause.** Struo `69e7ec13` ranks the self-contained wide cover and the
conservative cover lexicographically by estimated overall period, data period,
and only then LUT count. A one-picosecond estimate therefore wins regardless
of area. The area metric omits PFUMX/L6MUX21, and conservative retiming is
evaluated only when the unretimed conservative cover already won.

**Proposed resolution/regression.** PR
[#79](https://github.com/fabrica-eda/struo/pull/79), commit `333eb38`, always
evaluates wide, conservative, and retimed-conservative candidates. Preserve a
wide candidate when it alone
closes the requested period only when it remains inside the same explicit
resource-growth envelope; when both close, prefer the smaller physical-resource
score; when neither closes, reject unbounded growth for a sub-resolution timing
win. The score counts FFs, total emitted cells, PFUMX/L6MUX21, and two logic
slices per CCU2C. The final post-wide-mux retiming result is checked against the
same conservative reference.

With STR-004's lowering fix applied as well, the current `BranchPredictor`
maps to 13,490, 13,496, 14,197, and 13,502 cells at 120, 125, 126, and 133 MHz
respectively. The current `Core` maps to 27,047 cells at 125 MHz, with retiming
improving the mapper estimate from 13.720 ns to 12.760 ns. This removes the
area discontinuity; it does not claim 125 MHz timing closure. Unit regressions
cover unbounded one-picosecond wins, target-closing bounded candidates,
FF/CCU2C resource accounting, and a selected automatic-retiming path; Rica
integration validation covers the frequency sweep. Changing the requested
synthesis frequency until a smaller result appears is not an accepted
workaround.

## STR-002 — Partial packed-vector register writes expand recursively

**Observed.** A packed 16-bit speculative-RAS valid register was cleared and
then updated by two loops of per-bit assignments. Synthesis did not finish.
Changing only that logic to three full-vector masks and one whole-vector
assignment produced a 13,638-primitive map.

**Root cause.** Range lowering recursively slices the nested register
expression without memoizing `(expression, least-significant bit, width)`.
Multiple partial writes therefore re-expand the same expression ranges. Lazy
slicing remains necessary for dead self-references, so eagerly lowering the
entire expression is not a safe fix.

**Workaround/regression.** Rica builds full masks before the single assignment
in [`BranchPredictor.veryl`](riscv/src/BranchPredictor.veryl). The upstream MRE
should retain the two priority-ordered loops and add range memoization; a
regression must bound both completion time and lowered-expression count.

## STR-003 — Veryl constants and dynamic packed selects were not lowered

**Observed.** Module constants used by FSM expressions were omitted, while a
dynamic packed select was rejected with the unrelated diagnostic
`non-constant replication count`.

**Resolution.** Struo `dc80755` (PR #50) fixes both paths. The real I2C read
datapath remains the QoR regression: the dynamic select maps to 188 primitives
and routes at 289.35 MHz, versus 181 primitives and 252.02 MHz for the former
hand expansion. See [`i2c/KNOWN_ISSUES.md`](i2c/KNOWN_ISSUES.md).

## STR-004 — `always_comb` blocking order was lost

**Observed.** Struo `5f30edc` rejected the current TAGE predictor with
`CombinationalLoop { signal: "tage.train_entry0", bit: 15 }`. The mapper QoR
patch was not involved: clean `github/main` failed identically, while its
parent before the regression synthesized the same design.

The retained minimal pattern contains no memory:

```veryl
always_comb {
    temporary = source;
    if !temporary[15] {
        temporary[15] = 1'b1;
    }
    sink = temporary;
}
```

`git bisect` identifies `7917f11` (`Add selectable distributed memory
mapping`) as the first bad commit. That change filters memory statements by
iterating the top-level combinational statements itself, but passed the block's
initial environment to every statement. A later statement therefore could not
observe an earlier blocking assignment; the partial write above read the final
`temporary` signal and manufactured a feedback loop.

**Proposed resolution/regression.** PR
[#78](https://github.com/fabrica-eda/struo/pull/78), commit `7924cfa`, snapshots
the current environment immediately before each non-memory statement,
preserving the existing blocking semantics while retaining the memory filter.
The MRE fails before the patch and passes after it. The complete Struo
workspace passes, and the current `BranchPredictor` again synthesizes with
fourteen BRAMs, 3,862 FFs, and the same 11.560 ns unpatched-mapper timing
estimate as the last known-good lowerer.

## CEL-001 — Native x86 SSA reconstruction exceeded planned pressure

**Observed.** Native Celox code generation exhausted the SSA register
allocator while materializing reload recipes. The original Rica failure log
and standalone MRE were not retained; that evidence gap is recorded rather
than reconstructed from memory.

**Resolution.** Celox commits `f2b02950`, `736d05f6`, and `6a6f65ea`, merged by
PR #712 as `651be6b1`, preserve the planned pressure bound during reload
materialization. Rica pins `7bb38a0`. Native compilation of the predictor and
Core recovery suites is the local integration gate.

## TEX-001 — ECP5 STA did not correlate with nextpnr

**Observed.** Texo and nextpnr could report materially different setup margins
for the same mapped design because Texo applied a separate uncertainty policy
and incomplete clock-edge/corner handling. The old Texo WNS could therefore
not be used as a nextpnr-compatible QoR result.

**Resolution.** The fix chain is `0117735` (timing corners), `4023fa2`/PR #44
(setup uncertainty), `2136749`/PR #45 (clock edges), and `bcc53c6`/PR #46
(nextpnr margin). Rica commit `52bf8f2` pinned the correlated model and recorded
+9 ps setup and +290 ps hold slack on the then-current wrapper. Texo
`f8c57e09` contains the complete chain.

## TEX-002 — Hold ECO retained a route after pin-only rebinding

**Observed.** Releasing a packed LUT/FF pair could rebind the FF data input
from the dedicated `DI` path to general-routing `M` without moving the BEL.
The route freezer detected BEL movement but not the pin change and failed with
`invalid routing constraint: route arc ends away from sink pin`.

**Resolution.** Texo `299836dd` (PR #25) invalidates routes when either endpoint
pin binding changes. The I2C regression improved hold slack from -116 ps to
+9 ps while preserving +84 ps setup slack. Details are retained in
[`i2c/KNOWN_ISSUES.md`](i2c/KNOWN_ISSUES.md).

## TEX-003 — Routed configuration was functionally wrong

**Observed/isolation.** Texo P&R plus native bitgen failed the JTAG-I2C pointer
test. Repacking Texo's textual configuration with `ecppack` also failed, while
the byte-identical Struo netlist implemented by nextpnr and packed by
`ecppack` passed. This ruled out RTL, Struo mapping, and the bitstream encoder;
the remaining fault boundary was Texo implementation/configuration generation.

**Resolution.** Texo `522d7caf` (PR #23) preserves the legal JTAG and carry
configuration. Its native result passes the pointer test and full I2C hardware
regression at +106 ps setup and +378 ps hold slack. See
[`i2c/KNOWN_ISSUES.md`](i2c/KNOWN_ISSUES.md).

## TEX-004 — Critical high-fanout placement edges were discarded

**Observed.** The placer divided every timing edge by the full net fanout and
dropped nets above 256 sinks even when STA identified one critical sink. A
placement restart could consequently turn a near-closing result into a
multi-nanosecond regression on the same mapped netlist.

**Resolution.** Texo `ed36807`, merged as `ac29f25` by PR #49, uses square-root
fanout normalization and preserves the strongest sink-local timing edge. Rica
commit `a0368ef` recorded +20 ps setup/+290 ps hold and a hardware pass.
Upstream regressions cover aggregate fanout weight and survival of a critical
sink above the cutoff.

## TEX-005 — Bitgen emitted an invalid JTAG mode

**Observed.** The configuration contained a nonexistent `JTAG.MODE` enum even
though ER1/ER2 were the intended user-chain controls.

**Resolution.** Texo `a59f787` (PR #20) removes the invalid enum and retains
`JTAG.ER1 ENABLED` and `JTAG.ER2 ENABLED`. The public native bitgen API and the
full hardware SVF regression are the integration gates. See
[`i2c/KNOWN_ISSUES.md`](i2c/KNOWN_ISSUES.md).

## NPNR-001 — Related generated clocks need an explicit boundary check

**Classification.** This is a sign-off limitation, not a confirmed nextpnr
upstream defect. The phase-aligned 124.8 MHz CPU and 62.4 MHz memory clocks
allow only one CPU period for a request launched on the intervening CPU edge;
treating the domains as unrelated can hide that crossing.

**Local gate.** [`nextpnr.rs`](riscv/verify/src/bin/nextpnr.rs) extracts both
CPU-to-memory and memory-to-CPU critical paths from the nextpnr report and
rejects either path above one CPU period. The checker remains mandatory until
the flow directly expresses and signs off the generated-clock phase relation.

## Maintenance rule

For every newly isolated tool defect, add the affected revision, a retained
reproducer or numeric comparison, the current workaround, the upstream
commit/PR when available, and a regression gate. Move an item to “fixed” only
after the pinned dependency contains the fix and the relevant Rica regression
passes. Do not add transient failures such as an exporter stopping unless the
log and deterministic reproducer have been retained.
