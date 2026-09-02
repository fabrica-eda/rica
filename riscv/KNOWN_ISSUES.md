# RISC-V implementation findings

## EX forwarding must not close an ALU-to-ALU combinational loop

Directly wiring the current EX ALU result through the forwarding priority mux
into the next EX operand produced an 8.72 ns mapped path. Removing forwarding
met timing but inserted a bubble into every RAW-dependent ALU pair, which is
not an acceptable CPU architecture.

The RV64 implementation now computes a low 32-bit sum and both possible high
32-bit sums, then selects the high half with the low carry. Arithmetic and
logical results are captured by the common completion stage. Consumer
metadata selects that already-registered copy before the next ALU; a duplicate
128-bit bypass result bank was removed.
Consecutive RAW ALU dependencies therefore retain CPI 1 without an
ALU-to-ALU combinational recurrence. Loads, shifts, and comparisons whose
final values are not present in EX1 stall a dependent consumer once while
independent operations retain one-per-clock throughput.

## Data-dependent flushes must be registered

Even after fixing the bypass data path, using the EX branch comparison to
flush every younger pipeline register in the same cycle routed at only
133.60 MHz. The branch decision now terminates at `redirect_pending_q`; the
following cycle squashes wrong-path EX/MEM valid bits and redirects fetch.
This keeps branch-data fanout out of the register-enable network.

## A pending older redirect must suppress younger redirects

Registering redirect application exposed a separate correctness hazard. A
taken BNE correctly selected its target, but a younger JAL could reach EX while
that target was pending and enqueue a second, wrong-path redirect. The board's
delay loop therefore executed only once before jumping back to its outer x31
increment.

Redirect arbitration now refuses new redirects while an older redirect is
pending. The board-firmware regression checks all 4096 dependent ADDI/BNE
iterations before accepting the next x31 update, rather than merely checking
that x31 eventually changes.

## A held taken decision must cancel an already-pending shadow

The registered predictor can contain two sequential fall-through shadows while
an older taken prediction is held at the external response port: one compact
decision packet and one raw BTB/TAGE response. Canceling only a replacement
launched on the decision-accept edge allowed a pre-existing shadow to become a
visible decision after the taken head was accepted.

`BranchPredictor` now derives cancellation from the held classified taken
output. Both hidden shadows are consumed without allocating a lasting token or
appending speculative history. Requests remain blocked on the taken
accept/cancellation edge (`request_accept=0`); the target-path request can
launch on the following edge. The directed RTL tests
`taken_held_decision_cancels_an_already_pending_shadow` and
`taken_prediction_cancels_conditional_shadow_and_reuses_its_token` cover both
the pre-existing and same-edge shadow cases.

## An EX-owned prediction repair must preserve its own side effects

A stale taken BTB alias can classify an ordinary load, store, or CSR
instruction as a control transfer. EX correctly detects that next-PC mismatch,
but its registered redirect is pending on the following cycle, when that same
instruction owns CM. Treating every pending redirect as an older flush could
discard the owner's memory transaction or CSR write.

The redirect packet now records whether it came from the EX instruction or
from an older CM branch/system event. An EX-owned repair preserves its CM
owner, and a synchronous load/store keeps the redirect pending until its
response arrives. `core_predictor_recovery` verifies the load result, store
payload, and CSR write with
`stale_taken_prediction_on_a_load_does_not_bypass_memory_wait`,
`stale_taken_prediction_on_a_store_preserves_the_memory_side_effect`, and
`stale_taken_prediction_on_a_csr_preserves_the_csr_write`.

## Accepted cold BTB misses must reserve their victim before retirement

BTB updates are intentionally deferred until the owning token retires. When
the replacement bit advanced only on hits, two accepted cold misses to the
same set could snapshot the same victim way before either retired; their later
updates then overwrote one another.

Every accepted, non-canceled BTB response now owns the replacement-memory
write and advances its set's bit. Retire-time entry update/invalidation no
longer competes for that port; a miss uses the sole invalid physical way before
consulting PLRU, so invalidation needs no replacement write. The existing
same-address forwarding makes a following lookup observe a reservation made
on the same edge. A canceled sequential shadow does not consume replacement
state. The RTL regressions
`overlapping_same_set_cold_misses_reserve_distinct_btb_ways`,
`retiring_an_unrelated_control_cannot_steal_a_cold_miss_reservation`,
`accepted_hit_makes_the_other_valid_way_next_victim`,
`a_miss_uses_the_sole_invalid_way_even_when_plru_points_at_the_valid_way`, and
`taken_prediction_cancels_conditional_shadow_and_reuses_its_token` cover the
reservation, port-arbitration, hit-LRU, invalidation, and cancellation cases.

## A deferred BTB invalidation must prove that its physical entry is unchanged

A prediction token retains the BTB way selected when its lookup completed, but
the corresponding invalidation is deferred until retirement.  An older control
transfer can overwrite that same `(set, way)` after the younger lookup and
before the younger instruction retires.  If the younger instruction then turns
out to be a non-control, blindly invalidating its saved way erases the older
instruction's newer entry.  Architectural execution remains correct, but BTB
quality silently degrades and a software-only predictor model can diverge from
RTL.

Each live hit token now retains its exact BTB set and an identity-stale bit.
Every physical update or effective invalidation marks matching live snapshots
stale; retirement preserves the original hit/miss event accounting but gates
the destructive invalidation when identity is stale.  The outstanding raw
response carries the same guard, including the edge on which an older retire
overwrites the word while the younger old response is captured.  Directed RTL
regressions cover both the ordinary A-then-B ordering and that same-edge case.

## Predictor BRAM state must be scrubbed before fetch starts

BTB and TAGE validity bits live inside inferred block RAM and therefore cannot
be treated as resettable registers.  The physical tops instantiate
`BranchPredictor` with `SCRUB_ON_RESET=1`: the BTB clears 128 sets while TAGE
clears its 2048-entry base table and all tagged rows, and fetch remains unready
until the longer sweep finishes.

The focused scrub regression first poisons both endpoint rows and both BTB
ways through the real write paths, then resets the predictor.  It proves BTB
ready at exactly 128 clocks, frontend ready at exactly 2048 clocks, no request
acceptance during the sweep, and clean endpoint rows afterward.  This avoids a
false pass from a simulator that happens to initialize all memories to zero.

## The board ROM must not share the next-PC path

Driving the board instruction ROM directly from `fetch_base + fetch_offset`
combined a 32-bit PC addition, ROM decode, and fetch-register input into one
cycle. This became the full-wrapper critical path even after the ALU path was
shortened. `fetch_pc_q` now terminates the next-PC addition before the ROM.
The front end still presents and accepts one instruction address per clock;
this register changes latency, not throughput.

## A heartbeat must distinguish reset from count zero

The original board display drove all eight LEDs high both while reset was
asserted and while x31 was zero. Its separate conditional-branch delay loop
also made register writeback depend on millions of successful loop iterations.
D12 now reports PLL lock, D11 divides the commit stream into a heartbeat, and
the ROM updates x31 before entering its delay loop. D5 must therefore light
immediately, while later changes on D5-D10 prove completion of the dependent
ADDI/BNE loop and JAL redirect. PLL lock alone releases CPU reset, so the unused
generic SW4 input can no longer accidentally hold the CPU in reset. The exact
control-flow shape also has a reduced-delay RTL regression; a generic x31 test
is not accepted as evidence for this board firmware.

## Synchronous memory latency must remain local

The reusable `RegisterFile` infers two 32x64 1R1W memories so that two
independent read ports can map to eight ECP5 `DP16KD` blocks. Its synchronous
read latency is expected.

That latency must be represented by pipeline state and forwarding/stall
control. It must not become a fixed multi-cycle tax paid by every instruction,
as it did in the retired Core250 stress design.

## A memory stall must hold the synchronous register-file read result

The directed hardware sequence below originally returned `tohost=7` (fail 3):

```asm
sw x6, 0(x1)
ld x8, 0(x1)
ld x9, 24(x2)
bne x8, x9, fail_3
```

The store and first load were correct: x8 contained
`0x1234567880000001`. The independent `ld x9, 24(x2)` instead returned the
instruction bytes at address `0x18`, proving that its x2 base had become zero.
While the completion stage waited for synchronous data BRAM, RX and FD were
held but the synchronous register file continued consuming FD's younger read
addresses. When the wait ended, RX therefore captured operands belonging to
the following branch.

Core now deasserts the register file's native read enable during a memory wait.
This holds the already-aligned BRAM output without a 128-bit operand feedback
mux or a new address-mux timing cone. The mapped wrapper grew by three cells
(7757 to 7760), routes at 133.17 MHz for the 126 MHz constraint, and the exact
unreordered hardware suite passes `alu`, `compare`, `control`, `memory`, and
`shift` 5/5 on the LFE5UM5G-85 EVN. This historical 126 MHz PLL configuration
was later found to use a 6 MHz phase-detector clock outside the data-sheet
range and is no longer the board configuration.

## Timing and throughput are separate gates

The nominal timing target is 125 MHz, while the data-sheet-valid board PLL
runs at 124.8 MHz. An implementation is not accepted solely because it meets
an 8.000 ns period. Regressions must report at least:

- clocks per committed instruction for the architectural trace
- steady-state ALU instruction throughput
- taken-branch penalty
- load-use penalty
- mapped resource counts and post-route timing

The current predictor-enabled core commits the 58-instruction mixed trace in
118 clocks (2.034 CPI) and keeps consecutive dependent one-cycle integer ALU
operations at one per clock. The current 125 MHz Struo logical map contains
25,998 cells, has an 8.120 ns estimate at LUT depth 15, and produces mapped
nextpnr JSON SHA-256
`059ccc43681aa41f40e788bcfc6cc75f9d881f43104e9123dfbfe9041187d64d`.
This is a logical-map result, not post-route timing or hardware evidence. The
earlier 24,751-cell/7.920 ns, interim 25,287-cell, and pre-PLRU 25,988-cell
measurements are retained only as historical integration snapshots; they do
not describe the current RTL.

Official nextpnr 0.11.1 out-of-context static seed 5 placement of that exact
mapped JSON reaches 107.204 MHz (9.328 ns). Its routed critical path runs from
`predictor_retire_q[102]` to
`branch_predictor.slot_btb_identity_stale_q[2]`, with 2.000 ns of logic and
7.328 ns of routing. Pre-PLRU seeds selected materially different critical
cones and ranged from 97.163 to 104.624 MHz; the current improvement and path
change further demonstrate placement-sensitive QoR, but do not by themselves
establish a confirmed nextpnr defect.

The standalone current `BranchPredictor` is closer to the target but no longer
closes it: static seeds 1, 2, and 5 reach 117.938, 117.275, and 117.178 MHz
respectively. The nine-cell PLRU cleanup moved all three placements away from
the pre-cleanup 120.817--125.047 MHz range, again without a common worst path.
Integrating it into Core remains the larger timing-closure problem.

The older pre-predictor CSR/trap wrapper reached 125.88 MHz in nextpnr 0.6 seed
4 and its bitstream passed the then-current six-test hardware suite. Texo's
fixed placer later closed that historical wrapper with 20 ps setup and 290 ps
hold slack, and its native bitstream passed the same suite. Those results
predate the current predictor and eleven-counter ABI. The retired 250 MHz
design met timing while taking fifteen clocks per instruction; it remains in
Texo as a physical-design regression precisely so this CPU is not optimized
around that result.
