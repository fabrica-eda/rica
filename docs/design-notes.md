# Design notes

Hazards and timing problems found while building Rica, and the rules they led
to. Each entry names the regression that guards it where one exists.

## Pipeline

**No ALU-to-ALU combinational loop.** Forwarding the live EX result into the
next EX operand produced an 8.72 ns mapped path. The ALU result is now captured
in CM and forwarded from that register, so dependent ALU instructions still run
one per clock. Values that are not ready in EX (loads, and shifts/comparisons
without `EARLY_ALU`) stall a dependent consumer once.

**Register data-dependent flushes.** Using the EX branch comparison to flush all
younger stages in the same cycle routed at 133.6 MHz. The decision now ends in
`redirect_pending_q`, and squashing happens the next cycle.

**An older pending redirect blocks younger ones.** Without this, a younger JAL
reaching EX while a taken branch's redirect was pending enqueued a second,
wrong-path redirect.

**Hold synchronous register-file reads during memory waits.** While CM waited
for data BRAM, the register file kept consuming FD's younger read addresses, so
RX later captured operands of the wrong instruction (an independent load used a
zero base register). The core now deasserts the register file's read enable
during a wait, which holds the BRAM output without an operand feedback mux.

**Keep synchronous latency local.** The register file and other BRAM-based
structures have one cycle of read latency. That latency is handled by pipeline
state and forwarding/stall logic, never by a fixed multi-cycle cost paid by
every instruction.

**Keep comparators off issue paths.** The first U-mode build compared every PC
against all PMP bounds each cycle; that path was 11.4 ns, 8.4 ns of it routing.
The PMP permission caches and registered range decoding remove the comparators
from per-access paths. The same reasoning led to the registered ITLB lookup, the
dual I-cache tag compare and the registered PMA/PMP checker inputs in Sv39.

## Branch prediction

**A held taken decision cancels every pending fall-through shadow.** The
registered predictor can hold two sequential shadows behind a taken response;
cancelling only the one launched on the accept edge let the other become a
visible decision. (`taken_held_decision_cancels_an_already_pending_shadow`)

**A repair owned by EX keeps its own side effects.** A stale BTB alias can mark
a load, store or CSR instruction as a branch. The resulting redirect is pending
while that same instruction owns CM; the redirect records its origin so the
instruction's memory transaction or CSR write is preserved.
(`core_predictor_recovery`: `stale_taken_prediction_on_a_*`)

**Cold BTB misses reserve their victim at lookup.** Updates are deferred to
retirement, so two misses to the same set could pick the same way. Each accepted
response now advances the set's replacement bit immediately.

**Deferred invalidations check that the entry is unchanged.** A token's saved
BTB way may be overwritten by an older branch before the younger instruction
retires; each token tracks whether its `(set, way)` has changed and skips a stale
invalidation.

**Scrub predictor BRAM before fetching.** BTB and TAGE valid bits live in block
RAM, which has no reset. Board builds sweep every row on reset (128 BTB sets,
2048 TAGE base entries) and hold fetch until the sweep finishes. The regression
poisons the rows first, so a simulator that zero-initializes memory cannot hide
a missing scrub.

## Board

**Do not put the ROM on the next-PC path.** Driving the board ROM from the
next-PC adder made it the critical path; a fetch-address register now ends the
addition.

**Make the heartbeat distinguish reset from zero.** The LEDs show PLL lock, a
commit-driven heartbeat and x31; the ROM writes x31 before its delay loop, so
an immediately lit LED proves writeback and later changes prove the branch loop.

**Sample JTAGG on the falling edge.** An image that passed STA failed to load
programs because the debug shifter captured JTAGG signals on the same edge on
which they change. Sampling on falling JTCK, registering CE/SHIFT and treating
UPDATE as data fixed it. STA had passed because JTCK-domain endpoints were not
checked; Texo now refuses to report closure with unchecked endpoints, and the
JTCK domain is part of sign-off.

## Measuring performance

**Timing and throughput are separate gates.** A design that meets the clock
but costs cycles is not an improvement, and neither is one that saves cycles
but fails routed timing. Comparisons use the time for the same work (cycles ÷
clock) on identical ELFs, together with IPC, and keep diagnostic workloads
separate from application-like ones.

**Frequency is not the target by itself.** An early 250 MHz design met timing
but took fifteen clocks per instruction. It now lives in Texo as a
physical-design regression (`examples/core250-qor`).
