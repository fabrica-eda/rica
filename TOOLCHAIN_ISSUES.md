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
| STR-001 | Struo ECP5 mapper | Fixed upstream/current pin ([PR #79](https://github.com/fabrica-eda/struo/pull/79)) | `69e7ec13` through `5f30edc` | Wide-cover choice can multiply LUT/mux area discontinuously |
| STR-002 | Struo synthesis | Open; RTL workaround active | `69e7ec13` through validated integration revision `eb6dd6a` | Partial packed-register writes can expand recursively and fail to finish |
| STR-003 | Struo Veryl lowering | Fixed upstream/current pin | before `dc80755` | Module constants and dynamic packed selects failed to lower |
| STR-004 | Struo Veryl lowering | Fixed upstream/current pin ([PR #78](https://github.com/fabrica-eda/struo/pull/78)) | `7917f11` through `5f30edc` | Top-level `always_comb` statements can violate blocking-assignment order |
| STR-005 | Struo Veryl lowering | [PR #81](https://github.com/fabrica-eda/struo/pull/81) validated (`917327f`) but not integrated | `66e7b14` through validated integration revision `eb6dd6a` | Many-arm cases become linear priority-mux chains |
| STR-006 | Struo ECP5 mapper | [PR #80](https://github.com/fabrica-eda/struo/pull/80) candidate included in validated integration revision `eb6dd6a` | `4943a43` through `001974a` | Redundant true-dual-port BRAM clock-enable logic consumes timing margin |
| STR-007 | Struo ECP5/Celox adapter | [PR #82](https://github.com/fabrica-eda/struo/pull/82) candidate included in validated integration revision `eb6dd6a`; focused fixes validated, full release gate pending | `6b080ed` and earlier; early PR #82 heads | TDP rejection or a linear 512-way read mux blocked post-map simulation |
| STR-008 | Struo Veryl lowering | [PR #83](https://github.com/fabrica-eda/struo/pull/83) validated (`a4b9dad`) and included in integration revision `eb6dd6a` | `68be24c` verified pre-fix; fixed by the validated PR #83 candidate | Left-associated AND/OR expressions retain avoidable depth through synthesis and mapping |
| STR-009 | Struo ECP5 CE fanout constraints | Open algorithmic limitation; broad predictor constraint rejected | current integration snapshots | Broad LUT-driver replication can increase input fanout, route delay, and negative slack |
| STR-010 | Struo Veryl lowering | [PR #84](https://github.com/fabrica-eda/struo/pull/84) validated (`367101a`); Rica structural workaround active | integration revision `eb6dd6a` and upstream before PR #84 | Disjoint packed-struct fields written by separate FF processes are falsely reported as multiple drivers |
| STR-011 | Struo ECP5 mapper | [PR #86](https://github.com/fabrica-eda/struo/pull/86) validated (`eff404d`) and pinned through Texo PR #60 (`893d699`) | integration revision `eb6dd6a` and upstream before PR #86 | INIT-independent dynamic LUT inputs create false timing arcs and unnecessary routing load |
| CEL-001 | Celox native x86 backend | Historical fix; original Rica MRE/log not retained | before PR #712 | SSA reload materialization could exhaust the register allocator |
| CEL-002 | Celox SIR optimizer | [PR #736](https://github.com/celox-sim/celox/pull/736) open (`c005d316`); current workflow pins validated 0.3.1 [backport `f5c548d5`](https://github.com/celox-sim/celox/commit/f5c548d5d4becb298f924ab06e1febb9eb3da064) | Celox 0.3.1 and upstream before `c005d316` | Rejecting a >128-bit concat fold can discard operands and make native verification fail |
| CEL-003 | Celox source-memory simulation | Fixed upstream; [PR #755](https://github.com/celox-sim/celox/pull/755) merged and pinned at `4c2c227e` | 0.4.5 before `4c2c227e` | Shared-clock FF publication order, WASM store spill, native padded array access |
| TEX-001 | Texo ECP5 STA | Fixed upstream/current pin | before `bcc53c6` | Corner, clock-edge, and margin policy did not correlate with nextpnr |
| TEX-002 | Texo hold ECO | Fixed upstream/current pin | before `299836dd` | A pin-only FF rebinding could leave an invalid frozen route |
| TEX-003 | Texo implementation | Fixed upstream/current pin | before `522d7caf` | Successful P&R could emit a functionally wrong configuration |
| TEX-004 | Texo placer | Fixed upstream/current pin | before `ed36807` | Critical sinks could disappear from high-fanout placement edges |
| TEX-005 | Texo bitgen | Fixed upstream/current pin | before `a59f787` | Bitgen emitted a nonexistent `JTAG.MODE` enum |
| TEX-006 | Texo timing release gate | [PR #61](https://github.com/fabrica-eda/texo/pull/61) and internal-clock [PR #62](https://github.com/fabrica-eda/texo/pull/62) merged; native JTCK physically validated | exercised `7b372da` and upstream main `049b4be` | Positive checked-path slack allowed bitgen with unconstrained JTAG endpoints |
| NPNR-001 | nextpnr timing reports | Local one-CPU-period setup-budget guard; generated-clock STA still missing | historically observed with 0.6; locally reproduced with official 0.11.1 | Related divided-clock crossings are absent from the ordinary per-domain pass/fail result |
| RICA-001 | Rica nextpnr report validator | Fixed locally | before this work | Assuming exactly two global clocks rejected current reports containing two JTAG clocks |

**Current cleanup (2026-09-11).** The table above and detailed entries below
include historical integration states. Current Struo `4e9de73` contains the
case-mux and associative-Boolean fixes; Celox `7ff0dd2` plus the workspace
patches fixes the retained Celox reproductions. Fixed MREs and obsolete probes
have been deleted; historical commands require checking out their original Git
revision. See the [active MRE inventory](toolchain/mre/README.md) and
[current regressions](riscv/tests/README.md).

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

**Upstream resolution/regression.** PR
[#79](https://github.com/fabrica-eda/struo/pull/79), rebased head `0c4ce3c`, merged
as `001974a`, always
evaluates wide, conservative, and retimed-conservative candidates. Preserve a
wide candidate when it alone
closes the requested period only when it remains inside the same explicit
resource-growth envelope; when both close, prefer the smaller physical-resource
score; when neither closes, reject unbounded growth for a sub-resolution timing
win. The score counts FFs, total emitted cells, PFUMX/L6MUX21, and two logic
slices per CCU2C. The final post-wide-mux retiming result is checked against the
same conservative reference.

With STR-004's lowering fix applied as well, the predictor snapshot used for
PR validation
maps to 13,490, 13,496, 14,197, and 13,502 cells at 120, 125, 126, and 133 MHz
respectively. The Core snapshot from that validation maps to 27,047 cells at
125 MHz, with retiming improving the mapper estimate from 13.720 ns to
12.760 ns. These historical measurements demonstrate removal of the area
discontinuity; they are not a current Core timing-closure claim. Unit regressions
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

**Workaround/regression.** Rica avoids the triggering packed expression by
storing RAS validity as an unpacked sixteen-entry array of one-bit registers
and updating each bit directly in
[`BranchPredictor.veryl`](riscv/src/BranchPredictor.veryl). This is a structural
workaround, not evidence that range lowering is fixed. The upstream MRE should
retain the two priority-ordered partial-write loops and add range memoization;
a regression must bound both completion time and lowered-expression count.

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

**Upstream resolution/regression.** PR
[#78](https://github.com/fabrica-eda/struo/pull/78), head `7924cfa`, merged as
`17f0883`, snapshots
the current environment immediately before each non-memory statement,
preserving the existing blocking semantics while retaining the memory filter.
The MRE fails before the patch and passes after it. The complete Struo
workspace passes, and the then-current `BranchPredictor` again synthesizes with
fourteen BRAMs, 3,862 FFs, and the same 11.560 ns unpatched-mapper timing
estimate as the last known-good lowerer.

## STR-005 — Many-arm `case` statements become serial mux chains

**Observed.** On combined Struo commit `31ce27e`, an isolated registered
64-bit mux produced the following results:

| Form | Arms | Lowered/mapped depth | Mapped period | LUT4 | PFUMX |
| --- | ---: | ---: | ---: | ---: | ---: |
| `case` | 9 | 9 | 5.720 ns | 521 | 0 |
| Balanced tree | 9 | 4 | 2.380 ns | 384 | 192 |
| `case` | 16 | 16 | 9.220 ns | 976 | 0 |
| Balanced tree | 16 | 4 | 2.380 ns | 640 | 320 |

Changing only the mapper goal from 125 to 250 MHz did not alter the result.
The former Core CSR path exhibited the same shape: nineteen LUT4s followed by
one PFUMX. The source and analyzer were removed after integration of the fix; see the
[MRE inventory](toolchain/mre/README.md).

**Root cause.** `struo-frontend-veryl/src/lower.rs::lower_case` folds reversed
arms one at a time into nested `rtl.mux` expressions. Serialization therefore
occurs before synthesis and ECP5 mapping; the mapper cannot recover the mutual
exclusion of constant-equality arms.

**Upstream resolution/regression.** PR
[#81](https://github.com/fabrica-eda/struo/pull/81), amended head `917327f`,
replaces a case of four or more arms with a recursively balanced
`(any_match, value)` tree. One- through three-arm cases retain the established
priority chain because balancing them does not reduce final mux depth and can
needlessly perturb packing. The left subtree contains earlier source arms and
wins whenever its `any_match` is set, while the root's combined match selects
the default. This preserves first-match semantics for duplicate and
overlapping patterns rather than assuming mutual exclusion.

On the PR's registered one-bit structural benchmarks, the mapped results are:

| Arms | `001974a` | PR #81 | Critical combinational cells |
| ---: | ---: | ---: | ---: |
| 9 | 5.980 ns | 3.620 ns | 10 -> 6 |
| 16 | 9.480 ns | 4.140 ns | 17 -> 6 |

Regressions exercise duplicate/range overlap and assert logarithmic match and
data-tree depth for sixteen arms. Balancing every case in the earlier
`4d521a0` head perturbed the AXI4 physical regression enough that seed 3 fell
to 294.29 MHz and only 9/10 fixed seeds met 300 MHz. With the four-arm
threshold, amended-head CI passes 10/10 seeds at 302.57--316.96 MHz
(307.96 MHz mean), as well as the complete workspace tests, strict Clippy,
and formatting.

The fix remains intentionally separate from Rica's current integration pin.
Adding PR #81 to the otherwise identical PR #82 + selective PR #83 candidate
reduced mapped cells from 30,922 to 30,807, implemented cells from 29,601 to
29,345, routed nets from 31,126 to 30,870, PIPs from 362,622 to 358,571, and
setup violations from 1,201 to 1,129. It nevertheless worsened setup WNS from
-2.274 ns to -2.439 ns and setup TNS from -1.095210 us to -1.117105 us. The
formerly critical `rx_illegal_q` path improved, but TAGE lookup and fetch-line
paths became the new worst endpoints. This is useful upstream area evidence,
not a global timing win, so Rica continues to express timing-sensitive CSR,
RAS, and TAGE selection as explicit balanced trees rather than importing PR
#81 into the sign-off candidate.

## STR-006 — True-dual-port BRAM clock enable was combined after Boolean mapping

**Observed.** The canonical true-dual-port process in
[`TageTaggedTableMemory.veryl`](riscv/src/TageTaggedTableMemory.veryl) has an
outer port enable and an inner write/read choice. Veryl lowering produces
write enable `E && W` and read enable `E`, so the physical clock enable is
algebraically just `E`. The unpatched `31ce27e` map nevertheless placed both
the write-enable AND and another OR LUT on each `DP16KD.CEA` path.

With the otherwise identical retained Core snapshot at a 125 MHz goal, the worst TAGE
BRAM clock-enable endpoint improved from 8.120 ns to 7.080 ns after the local
mapper fix. The ECP5 mapper library passes all 76 tests, including a new
true-dual-port implication regression.

**Root cause.** In `struo-target-ecp5/src/mapped.rs`, the
`materialize_memory_clock_enable` function appended
`write_active || read_active` as a new physical LUT after generic Boolean
covering had already mapped both inputs. At that point the mapper could no
longer apply `E || (E && W) = E`.

**Upstream candidate/regression.** The validated change proves enable
implication in the source Boolean netlist and directly reuses the covering
enable; unrelated enables retain the physical OR. The regression checks both
ports, the emitted `CEA`/`CEB` connections, and absence of redundant CE LUTs.
The rebased `fix/ecp5-bram-clock-enable` commit
`6b080ed7a087633f3e3c36f324bed53b60ecbe33` is based on merged main
`001974aeadce241a95fd4bfa08025ea195b9d738`; all 76 target-crate tests and
strict Clippy pass. Applying the same fix to combined integration commit
`31ce27e9e63677dd6cc44e05151e72a5e504a8ad` produced the Core result above.
The fix is [PR #80](https://github.com/fabrica-eda/struo/pull/80). The validated
PR #82 snapshot `68be24c0ee6b172a2b22cb88ccb62b182e05151e` contains the #80
candidate. Against that snapshot, the predictor RTL/model and Core recovery
suites pass, and the 125 MHz logical Core map completed at 25,204 cells with a
7.920 ns mapper estimate. These are archived snapshot metrics rather than the
current PLRU-clean Core result. They validate the candidate in the integration
chain; they are not a post-route timing or hardware claim, and #80 remains
open upstream.

## STR-007 — ECP5 post-map TDP support and balanced BRAM reads

**Original rejection.** On former pin `6b080ed`, the complete source RTL and
native Celox regressions passed and the Core mapped, but requesting the
target-level simulation gate with

```sh
cargo run --locked --offline -p verify-riscv -- --post-map
```

reached that same map and then failed deterministically with:

```text
UnsupportedTrueDualPortMemory("bram_branch_predictor.tage.tagged_table0.g_memory[0].memory")
```

This was not a failure to infer or map the canonical
TAGE memory. `struo-celox/src/lib.rs` explicitly scans the mapped ECP5 netlist
for any `BlockRam` with `second_port: Some(_)` and returns
`UnsupportedTrueDualPortMemory` before constructing the simulation artifact.
Its unit test
`rejects_true_dual_port_memory_instead_of_ignoring_its_second_port` codifies
that rejection.

**Resolution and supported boundary.** Struo
[#82](https://github.com/fabrica-eda/struo/pull/82), validated head
`68be24c0ee6b172a2b22cb88ccb62b182e05151e`, implements mapped true-dual-port
memory when both ports use the same clock and the same active edge, including
simultaneous port activity. Independent clocks and opposite-edge port pairs
remain explicit errors rather than being simulated with incorrect ordering.
Focused adapter regressions cover the supported and rejected combinations.

The first TDP-capable implementation exposed a second deterministic failure:
a 512-word mapped memory became a linear 512-way read mux and overflowed a
128 KiB Rust stack while constructing the Celox artifact. The same #82 head
builds that mux as a balanced tree. Its focused 512x16 mapped-memory regression
passes with the 128 KiB stack limit, so neither the old TDP rejection nor the
old stack overflow is the current full-design blocker.

Consequently #82's focused fixes are validated, but the complete full-design
release-mode `--post-map` functional comparison has not yet run to completion.
On integration revision `eb6dd6a`, the complete source/native/map stages and
the Celox post-map frontend pass. One interactive run was stopped after
`post_map_builder.build_native()` remained silent for more than five minutes,
but that run had no retained bounded log or smaller reproducer. It is therefore
only motivation for the still-pending full gate, not a newly catalogued defect
or a reproducible performance measurement.

## STR-008 — Associative Boolean expressions retain source association

**Observed.** On pre-fix Struo `68be24c`, a registered sixteen-input Boolean
MRE compares ordinary left-associated Veryl `&&`/`||` expressions with an
explicitly balanced equivalent:

| Form | Synthesized depth | Mapped depth | Mapped period | Mapped cells |
| --- | ---: | ---: | ---: | ---: |
| Left-associated source | 15 | 5 | 3.320 ns | 22 |
| Explicit balanced tree | 4 | 2 | 1.820 ns | 22 |

Both forms use the same mapped cell count, so the 1.500 ns difference is not
an area-for-speed tradeoff. The fixed reproducer has been removed; see the
[MRE inventory](toolchain/mre/README.md).

**Isolation.** Veryl lowering preserves the parser's left association as a
chain of binary Boolean nodes. General Struo synthesis does not canonicalize
or rebalance associative AND/OR trees, and ECP5 mapping reduces but does not
eliminate the avoidable depth. This is separate from STR-005: `case` lowering
constructs a priority mux chain with first-match semantics, while these AND/OR
operators are associative and can be rebracketed without changing priority.

**Resolution.** [PR #83](https://github.com/fabrica-eda/struo/pull/83), final
head `a4b9dad186d67e22a51bac584a857c9e6a5150f9`,
flattens consecutive source-level `&&` and `||` expressions during Veryl
lowering, booleanizes each operand exactly once, and reduces the result as a
balanced tree only when the synthesized expression-depth improvement is at
least two levels. On the retained sixteen-input MRE both the ordinary and
explicitly balanced forms then synthesize to depth 4 and map to depth 2,
1.820 ns, and 22 cells. Regressions cover AND and OR truth behavior as well as
the logarithmic synthesized depth. Rica pins immutable combined revision
`eb6dd6ac184b2783e4738edd369a991131b3de7b`, which applies this fix after PR
#82. Both the PR branch and combined revision pass the complete Struo
workspace tests, formatting, and strict Clippy checks.

The two-level gate is a measured physical-QoR boundary, not a semantic
restriction. Re-associating every eligible chain caused the full board route
to stop with three overused wires. A control candidate that left all current
CPU chains unchanged but still fixed the sixteen-input MRE reproduced the
old payload-cut physical result: 30,957 mapped cells, 29,692 implemented
cells, 31,217 nets, 366,950 PIPs, -3.459 ns setup WNS, -3.121680 us setup
TNS, and 2,798 setup violations. Selecting expressions by a depth gain of at
least two routed successfully with 30,922 mapped cells, 29,601 implemented
cells, 31,126 nets, 362,622 PIPs, -2.274 ns WNS, -1.095210 us setup TNS, and
1,201 violations. Thus the selective frontend transform improves WNS by
1.185 ns, TNS by 2.026470 us, removes 1,597 violations, and reduces both cells
and route usage on the same RTL. Hold WNS remains +290 ps in both runs.

This remains deliberately a frontend source-expression fix: re-associating
arbitrary shared synthesis DAGs would need separate fanout-, sharing-, and
physical-congestion-aware cost analysis.

## STR-009 — Broad CE fanout constraints can worsen their input cone

**Observed.** Applying `max_fanout = 8` across the TAGE and BTB register-enable
endpoints on the retained predictor P&R baseline snapshot made routed timing
worse:

| Metric | Baseline | Broad TAGE/BTB limit 8 |
| --- | ---: | ---: |
| Representative LUT input fanout A | 254 | 274 |
| Its routed delay | 1.889 ns | 2.732 ns |
| Representative LUT input fanout B | 255 | 293 |
| Its routed delay | 2.493 ns | 2.885 ns |
| Setup WNS | -4.184 ns | -4.951 ns |

This result is scope- and cone-dependent, not evidence that CE branching
should be disabled globally. The retained artifact hashes and extracted
physical metrics are recorded in
[`toolchain/measurements/2026-09-02-rica-qor.md`](toolchain/measurements/2026-09-02-rica-qor.md).

**Root cause/constraint boundary.**
`apply_register_enable_fanout_constraints` duplicates a LUT that drives an
over-limit CE, copies its truth table, and reconnects at most the requested
number of FFs to each clone. Every clone still consumes all four original LUT
inputs. The transformation bounds the output branch fanout but does not model
the multiplied fanout and routing congestion moved onto those shared inputs.
For the broad predictor constraint, that upstream cost exceeds the CE-output
benefit.

**Workaround/required resolution.** An earlier speculative-RAS-only experiment
was not retained with a reproducible log, so its numerical result and routing
outcome are deliberately excluded from this catalog. Rica applies no
predictor-wide or RAS-only CE constraint in its documented flow; TAGE/BTB/RAS
remain unconstrained while the existing CSR/counter constraints are retained.
A general fix must score both sides of each proposed
replication, including input fanout, placement locality, and routed or
estimated congestion, and must be selectable per endpoint group. Regressions
must retain both the RAS improvement and the
broad-predictor counterexample; merely proving logical equivalence or bounded
CE output fanout is insufficient.

## STR-010 — Disjoint packed-struct FF fields falsely collide

**Observed.** The upstream PR regression fixture stores an eight-bit unreset
payload field and a one-bit reset valid field in the same packed struct, with
one `always_ff` process owning each disjoint field. The established lowerer
rejects the design as multiple procedural drivers even though no destination
bit is shared. Rica hit the same failure when it separated reset-bearing
fetch-queue occupancy from masked reset-free payload while retaining one
aggregate `FetchPacket` register. Rica retains the structural workaround; the
minimal source currently lives in the upstream regression rather than this
repository's `toolchain/mre` directory.

**Root cause.** The lowering ownership key names only the aggregate signal and
does not include a packed select; the register target likewise denotes the
whole aggregate. Merely weakening the collision check would therefore let two
whole-signal FF lowerings overwrite one another. Rica's current workaround is
structural: payload and valid are distinct stored variables, with the packed
packet retained only as a combinational view.

**Upstream candidate/regression.** Struo
[#84](https://github.com/fabrica-eda/struo/pull/84), head
`367101a96563d6f317b5b1cc71c185f9e30e9d9d`, first computes destination-bit
ownership and creates slice proxy FFs only for aggregates with multiple
disjoint owners. Ordinary one-owner registers retain their established
lowering and materialization order. The positive disjoint-field MRE passes
synthesis, ECP5 mapping, and native simulation; overlapping slices and
whole-plus-partial ownership remain rejected. The final head passes the full
workspace/Veryl/ECP5 CI and the fixed-seed 300 MHz physical smoke gate. AXI4
and counter32 mapped JSON remain byte-identical to upstream main. Rica does not
repin solely for this fix because its explicit payload/valid split is also the
desired reset and clock-enable structure.

## STR-011 — LUT inputs outside functional support remain routed and timed

**Observed.** The current `HardwareTestTop` map, SHA-256
`8dbbe688e3c736f9f5ea7ea247aef1f73398f1c602e2137b874625a37f0da167`,
contains `lut11591` with `INIT=0xa000` and dynamic inputs A, B, C, and D.
ECP5 LUT input A is the least-significant INIT index bit, so this truth table
implements `A && C && D`: its B=0 and B=1 cofactors are identical. Nevertheless
B remains connected to the high-fanout `lookup_accept` wire, and the LUT output
drives all four tagged-table `WEA` pins.

Official nextpnr 0.11.1 therefore reports the current static-seed-5 CPU
critical path through `lut11591.B` and into tagged-table 3 `WEA`. The retained
baseline report has SHA-256
`ba1ea3023f1c5a10d609bb92caacd4c33ada39fb6688d666886af1098f57ebe0`
and reports 11.161 ns. This is not an isolated LUT: an exhaustive cofactor scan
of the current mapped JSON finds 114 dynamic LUT pin connections outside their
INIT functional support across 89 LUT4 cells. The counts by physical input are
40 A, 27 B, 32 C, and 15 D connections.

**Isolation.** A controlled netlist edit changed only `lut11591.B` from its
dynamic wire to constant zero, which is functionally equivalent for
`INIT=0xa000`. Re-running the same official nextpnr 0.11.1 static-seed-5 flow
removed the impossible `lookup_accept -> lut11591.B -> WEA` timing path. The
reported CPU critical path instead ended at the genuine
`tage.lookup_base_q[5].CE` endpoint. The edited map and report have SHA-256
`3c37b8f91841f99d7193d65cee82328035ac40bcfd7f6819d2c2fc8cbe5e4cdf`
and `4576db1312168a4ed263a499e91d758e81f11bf598b97a828ec0b7019f4dee33`.

| Variant | CPU period | Endpoint |
| --- | ---: | --- |
| Current mapped netlist | 11.161 ns | tagged table 3 `WEA` through `lut11591.B` |
| Only dead B input tied low | 11.301 ns | `tage.lookup_base_q[5].CE` |

The slightly worse replacement path is placement/routing perturbation plus the
next genuine bottleneck; it does not make the removed B dependency real. This
experiment also shows that support pruning corrects timing-path validity but
does not by itself close the current CPU timing.

**Root cause.** LUT covering can retain a cut leaf after the resulting truth
table has become independent of that leaf. `LutEmitter` preserves every
selected leaf as a physical LUT input, and later mapped timing, depth, fanout,
serialization, and nextpnr STA process every connected A/B/C/D pin. Neither
the mapper nor the emitted netlist minimizes the final LUT's functional
support.

**Required resolution/regression.** Before mapped fanout/timing scoring and
nextpnr serialization, Struo must compare each LUT4 input's two INIT cofactors.
When they are identical, a dynamic connection on that pin must be replaced by
a constant; tying it low preserves INIT unchanged. The pass must cover LUTs
created by later mapped transformations as well as the initial `LutEmitter`.

A false-path constraint is not an accepted fix: it would suppress STA while
retaining the useless physical connection and congestion. A net-wide exception
on `lookup_accept` would additionally hide its genuine CE paths, while a
generated-cell/pin-specific exception would be unstable across mapping and
retiming. The focused upstream regression must construct a mapped LUT4 with
four dynamic inputs and `INIT=0xa000`, prune functional support, and assert
that only B becomes constant, its former wire loses that fanout, and exhaustive
input evaluation remains equivalent. A control LUT whose output depends on B
must retain B. A final-map invariant should reject every dynamic LUT input
whose INIT cofactors are identical.

**Resolution.** Struo PR #86 implements the functional-support cleanup at the
mapped-netlist normalization points and includes equivalence plus focused LUT
regressions. Rica pins its validated head
`eff404dff0c4806064e2a4e0f59564b1079c001d` directly and also consumes the
matching Texo integration from PR #60 at
`893d699f3624e8ff23e00e0264cfd87e9ed1ba2c`. An exhaustive scan of the final
`HardwareTestTop` map (SHA-256
`91d811681b519be86218558ca588f6617ee7cfc3c4281b69e087e5b62347ea8a`)
finds zero dynamic LUT4 pins outside functional support. The retained patch in
`toolchain/patches/` is the review artifact; normal builds use the pinned Git
dependencies and do not apply it locally.

## CEL-001 — Native x86 SSA reconstruction exceeded planned pressure

**Observed.** Native Celox code generation exhausted the SSA register
allocator while materializing reload recipes. The original Rica failure log
and standalone MRE were not retained; that evidence gap is recorded rather
than reconstructed from memory.

**Resolution.** Celox commits `f2b02950`, `736d05f6`, and `6a6f65ea`, merged by
PR #712 as `651be6b1`, preserve the planned pressure bound during reload
materialization. Rica pins `7bb38a0`. Native compilation of the predictor and
Core recovery suites is the local integration gate.

## CEL-002 — Rejected wide-concat folding discards slice operands

**Observed.** The official Struo CLI on combined commit
`31ce27e9e63677dd6cc44e05151e72a5e504a8ad`, whose lock file selects Celox
0.3.1, completed synthesis and ECP5 mapping of the retained `CaseMux9` source
but failed in the CLI's unconditional post-map native-simulator build. The
exact command, run from the Rica repository root, was equivalent to:

```sh
/path/to/struo/target/release/struo \
  toolchain/mre/struo-case-mux \
  --top CaseMux9 \
  --output /tmp/rica-CaseMux9-retained.json \
  --timing-goal-mhz 125
```

The output file is written after native compilation, so it was never reached.
Celox reported:

```text
JIT Code generation error: native merged-chain optimization: during native jump inlining: SIR verify [TYPE.CONCAT_WIDTH] at b0/i66: after single-predecessor inlining: concat operands total 69 bits, r644 has width 644
```

**Isolation/ownership.** The independent public-API MRE (since
[removed after the fix](toolchain/mre/README.md)) constructs one 132-bit atomic FF bank directly as 129 input bits, two selector
bits, and one current-state XOR bit. `FrontendArtifact::validate()` and
`build_wasm()` both pass, as does native compilation of the otherwise identical
131-bit control. Native compilation of the 132-bit artifact fails with:

```text
JIT Code generation error: native merged-chain optimization: during native jump inlining: SIR verify [TYPE.CONCAT_WIDTH] at b0/i5: after single-predecessor inlining: concat operands total 4 bits, r134 has width 132
```

No Struo code or serialized artifact participates in that reproducer.

**Root cause.** `ConcatFoldingPass` first consumes an entire contiguous slice
run and then tries to replace it with a load. Loads wider than 128 bits are not
eligible. On that rejection path the old implementation restored only the
first consumed operand instead of the complete run, reducing this 132-bit
concat to four bits. The later single-predecessor jump-inlining verifier
reported `TYPE.CONCAT_WIDTH`; it detected the already-corrupt SIR but did not
cause the corruption.

**Upstream resolution/regression.** Celox
[#736](https://github.com/celox-sim/celox/pull/736), validated head
`c005d316`, restores every consumed slice operand in source order when the
load fold is rejected. Its local validation includes the retained 132-bit MRE
in debug and release configurations, the 131-bit control, and all 402 optimizer
tests. The consumer-safe Celox 0.3.1 backport is immutable revision
[`f5c548d5d4becb298f924ab06e1febb9eb3da064`](https://github.com/celox-sim/celox/commit/f5c548d5d4becb298f924ab06e1febb9eb3da064);
Rica patches only
`celox-sir-opt` to that revision while leaving the sibling Celox crates on the
registry version, avoiding duplicate public SIR types. With that backport the
artifact validation, WASM build, control native build, and 132-bit native build
all pass. Upstream #736 is still open. The complete release-mode full-design
post-map gate remains pending and is not evidence for or against this isolated
width fix.

## CEL-003 — Unused module parameter changes memory behavior

The historical source/harness ([removed after the fix](toolchain/mre/README.md))
adds one unused parameter to the original `HardwareMemory`, with no other RTL
change. Native simulation passes the original consecutive-byte stream but
returns the following write's byte at each address in the parameterized form.
WASM also disagrees with the expected stream, including on the original source.
Updating to official Celox 0.4.5 reproduced the same failures. The byte shift
was isolated to shared-clock FF event assembly: the ordinary tick path
published one reset group's updates before evaluating the other groups.
The parameter changes variable/map ordering, exposing this pre-edge sampling
violation. Merging all evaluations followed by all applications fixes it.
The fused comb/FF schedule was already correct.

Two independent backend defects also appeared in the expanded matrix. WASM
lost spill bits when a shifted scalar store covered more than 64 destination
bits. Native late optimization created aggregate accesses across padded array
elements even at O0. The fixes use destination coverage for WASM and split
native accesses at logical element boundaries. In the correctness-only
baseline, independent negative controls failed when each corresponding fix
was removed.

The initial local fix used 0.4.5 `9aa2457` with four
[vendored crate patches](toolchain/vendor/celox/README.md). Original and
parameterized memories pass native, Cranelift, interpreter and WASM at all
three optimization levels (24 cases). The active regression covers both
source forms at O0/O1/O2 on native and WASM. Struo's separate 0.3.1 dependency
and optimizer backport are retained.

The follow-up [FF performance work](toolchain/measurements/2026-09-06-celox-ff-performance/README.md)
reuses the existing dependency scheduler for ordinary multi-group tick events
and fixes two missed native StateSSA promotion cases. Safe direct updates and
register values remove unnecessary staging while preserving pre-edge reads.
The correct merged two-phase unit remains the fallback for adapters without
source-aware lowering. The fourth crate patch is this optimizer change;
the three original correctness defects are unchanged.

The fixes and optimization were submitted as
[Celox PR #755](https://github.com/celox-sim/celox/pull/755), based on
`master` `01acba6ec`. The [submission record](toolchain/measurements/2026-09-06-celox-pr/README.md)
contains the final commit, validation on that base, and the two test-only SV
exclusions for unsupported four-state FF event signals.

PR #755 was merged on 2026-09-06 after CI and both benchmark workflows
succeeded. Rica now pins the merge commit `4c2c227ebfb908e294726e1af80421d976bc69fd`
directly, with all four local Cargo overrides and crate copies removed.
The merge tree is identical to the tested PR head. The historical patch and
manifests remain for reproducing earlier measurements; Struo's separate 0.3.1
optimizer backport is unchanged. See the
[upstream adoption record](toolchain/measurements/2026-09-06-celox-upstream/README.md)
for CI details and downstream validation without local patches.

The historical failed LSU run supplies no IPC result. The normal 31 RTL files
match the physical native-JTCK baseline, whose exact bitstream passed 100
distinct ELFs. The new [validation record](toolchain/measurements/2026-09-06-celox-memory-fix/README.md)
separates simulator regressions and the isolated LSU-disabled CPU replay from
that earlier physical qualification. Fixing the simulator does not resolve
the independently rejected LSU candidates' routed timing.

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

## TEX-006 — Incomplete STA coverage could be promoted to timing closure

**Observed.** The RV64 image with checkpoint SHA-256
`2968727d267284680a1f22ada914f8b259d7bfca1d61ed04a805b23cba34198e`
had setup +14 ps / hold +290 ps and `met_timing=true`, but 519 JTAG endpoints
were `unconstrained_clock` and one CDC endpoint was `no_synchronous_launch`.
Texo still recorded `timing_closure` and generated a bitstream with dead JTAG
ER1 responses. The source audit isolates a missing coverage gate; it does not
prove that nextpnr/Trellis delay numbers or STA arithmetic were wrong.

**Upstream coverage fix (merged).** [PR #61](https://github.com/fabrica-eda/texo/pull/61),
head `472340e1f9b81b001a82d732c464847933f1be80`, requires reviewed modeled
endpoint coverage in the flow and independently at bitgen. Unconnected or
unconstrained capture clocks cannot be excepted. Deliberate
`no_synchronous_launch` omissions require an exact cell/pin/reason and a
justification, retained in the checkpoint. New omissions, stale/duplicate
exceptions, and missing coverage block generation before runtime lookup or
output. Inherited timing evidence is cleared before evaluating a new run.

**Validation.** Workspace tests: 417 passed, 5 existing ignored; example tests:
3 passed; fmt, Clippy with warnings denied, and Python compilation passed.
The new bitgen regressions fail against clean main `049b4be` and pass at the
PR head. Both retained full Rica checkpoints (original failing and RTL-fixed
with only main STA) are rejected for unconstrained JTAG clocks, with no output.
[PR validation records](toolchain/measurements/2026-09-06-texo-timing-coverage-pr/)
retain the logs and artifact identifiers.

**Native JTCK follow-up.** [PR #62](https://github.com/fabrica-eda/texo/pull/62)
merged as `fd261c7`. The `--clock-constraints` interface selects `jtagg.JTCK`
by mapped cell/output pin and preserves PLL/DCCA relationships. Rica's native
STA checks all 515 JTCK setup/hold endpoints; their nominal slacks and edges
match the supplemental graph on the same routed checkpoint. Four external/CDC
endpoints have exact reviewed exceptions. Normal bitgen produced an image
that passed 100 unique ELFs / 118 physical executions. Records are in
[native JTCK measurements](toolchain/measurements/2026-09-06-rv64-native-jtck/).
The pinned local CLI includes the coverage fix; older Cargo/tool snapshots
and sealed historical artifacts retain their original provenance.

JTAGG launch/capture characterization and reset recovery/removal remain
follow-up work; an exception is not a timing check or proof of CDC correctness.
[PR #63](https://github.com/fabrica-eda/texo/pull/63) adds an explicit setup
reserve without changing the PLL periods; the reserve is not characterized
PVT/jitter data.

## NPNR-001 — Divided PLL outputs are not signed off as related clocks

**Classification.** This is a confirmed limitation of the exercised ECP5
flow, not yet an upstream correctness claim. With `CLKOS_CPHASE=0`,
`CLKOS2_CPHASE=0`, and output dividers 5 and 10, the 124.8 MHz CPU and 62.4 MHz
memory outputs are treated as edge-aligned by the local board contract. A
transfer launched on the intervening CPU edge therefore has only one CPU
period before the next memory edge. This behavior was observed historically
with nextpnr 0.6 and reproduced from source and reports with the locally built
official 0.11.1. Both crossings appear as separate `Max delay` paths but are
not compared with that one-period budget in the ordinary per-domain Fmax
pass/fail summary. The retained 0.11.1 report hash and extracted delays are in
[`toolchain/measurements/2026-09-02-rica-qor.md`](toolchain/measurements/2026-09-02-rica-qor.md).

The ECP5 packer creates a separate frequency constraint for each enabled PLL
output, but the architecture timing model marks every `EHXPLLL` port as
`TMG_IGNORE`; it does not expose a timing arc through the PLL or interpret its
`CPHASE`/`FPHASE` parameters. Generic related-clock discovery therefore cannot
trace `CLKOS` and `CLKOS2` to a common clock source. The generic reporter also
expects an identified related pair to have equal frequencies, which is a
second reason it cannot sign off this 2:1 relationship. The 0.11.1 reports
contain no `clk-to-clk` segment for the pair, consistent with that source-level
analysis. Absence of such a segment is not sufficient evidence by itself:
generic nextpnr reporting can also omit a zero-delay clock segment.

**Local gate and boundary.** [`nextpnr.rs`](riscv/verify/src/bin/nextpnr.rs)
first validates the exact board PLL topology, feedback, 12 -> 624 MHz divider
chain, enabled `CLKOS`/`CLKOS2` outputs, zero phase parameters, and 124.8/62.4
MHz frequency attributes. It checks that nextpnr's quantized report constraints
match those output frequencies and retain the 2:1 relation. Only then does it
require both interior Fmax checks to pass, take the single reported worst setup
path for each direction, sum nextpnr's path segments, and reject a sum above one
CPU period.

This is a deliberately limited setup-budget guard for one exact PLL
configuration. It recognizes zero phase as a required invariant rather than
implementing general `CPHASE` semantics, does not enumerate every
launch/capture edge, and does not sign off hold, clock skew, PLL jitter, or
uncertainty. It remains mandatory, but it must not be described as complete
generated-clock STA.

## RICA-001 — Board-report validation assumed exactly two global clocks

**Observed/root cause.** The local sign-off checker originally selected the
faster of exactly two `glbnet` entries as CPU and the other as memory. Current
nextpnr 0.11.1 reports four global clocks for the same board: CPU, memory, and
the two JTAG transport clocks. The checker therefore stopped with `found 4`
before evaluating either interior timing or the related-clock crossings.

**Resolution/regression.** The checker now identifies CPU and memory clocks
from mapped critical-path endpoint hierarchy (`core.*` versus
`hardware_memory.*`), ignores unrelated JTAG domains, and verifies that the
selected constraints match the validated PLL's absolute frequencies and retain
the expected 2:1 relation. Unit tests construct synthetic two-clock and
four-clock report shapes, including both JTAG domains and stacked
retiming/name prefixes; they also reject a different 100/50 MHz pair and a
nonzero phase parameter. On the retained nextpnr 0.11.1 heap report the checker
now reaches the real failure:
`CPU 80.89/124.81 MHz, memory 96.97/62.40 MHz`.
The current PLRU-clean seed-5 report exercises both diagnostics at once:
`CPU 89.60/124.81 MHz, memory 115.18/62.40 MHz` and
`CPU->memory 2.923/8.012 ns, memory->CPU 9.205/8.012 ns`.

## Maintenance rule

For every newly isolated tool defect, add the affected revision, a retained
reproducer or numeric comparison, the current workaround, the upstream
commit/PR when available, and a regression gate. Move an item to “fixed” only
after the pinned dependency contains the fix and the relevant Rica regression
passes. Do not add transient failures such as an exporter stopping unless the
log and deterministic reproducer have been retained.
