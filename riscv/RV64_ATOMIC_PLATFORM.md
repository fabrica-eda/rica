# RV64 A platform

2026-09-23の追加認定: [実測STA表全体を使うCPU](characterization/STA_REPLACEMENT.md)を
CPU 124.8 MHz／メモリ62.4 MHzで採用済み。温度余裕20%とsetup/hold各194 psを含めて
残余 +2/+1 ps、267実機試験に合格した。
[採用プロファイル](qualified-measured-sta.json)にbitstreamとcheckpointのハッシュを記録した。
実行時は `riscv/build/riscv-hardware-compact-atomic-measured-sta-124p8-20260922.bit` を明示する。
以下の `atomic-120` と旧実測配置イメージは比較元として保持する。

The opt-in `ENABLE_A` implementation includes Zaamo and Zalrsc. Core enables it
only with synchronous data memory and strict physical addressing. The model
selects it with `--a-extension --strict-memory --hardware-memory-rtl` and the
board top is `HardwareCompactAtomicTestTop`. This configuration passed routed
timing, integrated JTCK checks, checked bitgen and 267 exact-bitstream hardware
tests at 124.8 MHz on 2026-09-22 (JST). The runner still defaults to the older
console image; select the A-enabled bitstream explicitly as shown below.

## Architectural contract

- All nine AMO operations and LR/SC support W and D, and every aq/rl encoding.
  LR requires rs2=x0. Reserved operations/widths trap as illegal instructions.
  `misa.A` is set only in the enabled configuration; writes retain its fixed value.
- The atomic PMA covers `0x80000000..0x8003efff`, the 252 KiB RAM. Reset aliases
  and all MMIO are excluded. Full 64-bit address checks precede memory requests.
- Natural alignment is required: four bytes for W, eight for D. Misalignment
  reports load-address-misaligned (4) for LR, store/AMO-address-misaligned (6)
  otherwise. An aligned address outside atomic RAM reports access fault 5/7.
  MTVAL is the original address and MEPC is the atomic instruction's PC.
  Permission/alignment checks also apply to a failed SC with no reservation.
- W operations ignore rs2[63:32], return sign-extended old memory in rd, and
  write only four bytes. SC returns 0 on success and 1 on failure. rd=x0 still
  performs the memory operation and LR reservation update.
- A reservation covers exactly the naturally aligned word/doubleword from the
  latest LR. SC must match its physical address and width. Every SC consumes
  the reservation, including failure. A new LR replaces it. Any ordinary store,
  AMO, trap entry, MRET or reset clears it. Conservative clearing by unrelated
  stores is permitted; constrained LR/SC loops without interruption succeed.
  The reservation stores RAM address bits 17:2 and the access width; the full
  address and alignment checks still precede every SC, including failed SC.
- The board has one hart and no DMA writer. JTAG RAM writes are rejected while
  `run_q` is set; stopping the CPU holds it in reset. Console RX does not write
  RAM. A future external RAM writer must invalidate reservations and serialize
  against the whole AMO; this implementation does not claim multi-master support.

These rules implement the relevant
[Zaamo](https://raw.githubusercontent.com/riscv/riscv-isa-manual/main/src/unpriv/zaamo.adoc)
and [Zalrsc](https://raw.githubusercontent.com/riscv/riscv-isa-manual/main/src/unpriv/zalrsc.adoc)
requirements with a stricter single-hart memory ordering policy.

## Completion and ordering

Atomic instructions carry a separate pipeline tag but use the existing load
interlock and store-data forwarding. They do not issue the ordinary EX-to-CM
request. Instead `AtomicLsu` owns CM through read, registered arithmetic and
write acknowledgment. Each transfer toggles the existing request token exactly
once. Failed SC and faults issue no memory request. The ordinary LSU path adds
no pipeline stage or wait cycle when the A extension is enabled.

Ordinary EX operands retain the qualified core's shared arithmetic/logic
forwarding mux. The timing work below changes atomic control and arithmetic
without replacing that ordinary forwarding path.

Core registers the reservation-invalidation pulse from an ordinary store's
retirement, trap entry or MRET. A following SC can start only on the next edge
and observes that pulse before checking the reservation. Trap/MRET flushing
also precedes any new atomic request. The standalone LSU still applies its
invalidate input immediately. This cuts the completion-to-trap-to-reservation
feedback path without adding a memory wait or changing the reservation contract.

CM also captures valid atomic ownership and ordinary memory ownership on entry.
The flags hold with CM and clear with its valid bit on a redirect. They replace
repeated valid/type/exception decoding on the frontend's atomic/memory wait
paths; ordinary faulting memory operations keep their existing token-wait rule.

SC/AMO shares the ordinary CM write-data register and memory write bus. On the
AMO store-issue edge, Core captures the registered ALU result into that register
while toggling the request token. SC retains its already captured source value.
This removes a separate 64-bit
atomic write buffer and the memory-output data mux. The original AMO source is
already captured by the arithmetic unit before the CM register is reused.
The LSU result has a separate enabled register. The arithmetic unit exposes
its captured old operand directly while its response is valid, avoiding a
second old-value register; backpressure keeps that operand stable. Operand capture
zero-extends W inputs before arithmetic. A separate stage registers the sum and
signed/unsigned comparisons. The next stage registers a bitwise result and
one-hot operation selectors; the final stage selects the write value. This adds
one arithmetic pipeline cycle while keeping opcode decoding and comparison off
the final 64-bit result selection. Total transaction latency also depends on
CPU/memory clock alignment. LR and SC do not use this arithmetic pipeline.

The A-enabled board supplies the registered load acknowledgment and raw store
acknowledgment separately. Atomic read/store states check only their own token;
the ordinary LSU selects using its registered CM operation. This removes the
atomic write indication from the response-selection-to-frontend hold path.
Configurations without split responses retain the original shared-token input.

The backend remains held until completion. Earlier memory work has already
completed, and younger memory work cannot issue before the atomic finishes.
This orders all memory/MMIO operations for every aq/rl combination. FENCE remains
an ordered no-op on this single-hart memory system. Interrupts drain EX and CM
and therefore observe either side of the complete AMO, never its internal read
or write phase. An older branch redirect cancels an atomic before it issues;
a prediction repair belonging to the atomic preserves its CM transaction.

Only one instruction retires for an AMO, and its rd result is the original value.
The committed memory trace uses the actual AMO write value; failed SC has no store
trace. The store indication is cleared on consumption so a successor, including
a board `tohost` observer, cannot see a previous atomic's write state.
Successful SC/AMO registers the store acknowledgment and retires on the next
CPU edge. This keeps the memory acknowledgment off the atomic frontend-hold
path. `data_store_retire_ready` gates both the board's terminal `tohost` capture
and model observers, aligning counter capture with architectural retirement
(including the preserved-CM redirect case). Ordinary stores retain their
existing acknowledgment/retirement edge.

The LSU accepts only operations validated by the decoder. Among those encodings,
funct5 bits 1:0 distinguish LR/SC from all nine AMOs, so the transaction state
machine need not repeat the upper funct5 legality checks. Native and mapped
LSU tests exercise every AMO in both widths, check the two-access sequence and
verify that each invalidates an older LR reservation at another address.

The EX frontend-stop predicate (exceptions and system barriers) is decoded
with the EX payload. Frontend enables that already exclude a pending redirect
use raw backend-wait conditions; retirement and recovery retain the
redirect-discard mask. Atomic
completion uses explicit fault, failed-SC, LR-read and store-acknowledgment
events. In the DONE state, consumption depends on CM ownership and redirect
disposition; it permits successful retirement when the redirect preserves CM,
while a fault waits for the pending redirect to clear. These changes retain
the previous transaction edges and measured instruction cycle counts.

For the A-enabled core, fetch-cursor registers have explicit write enables.
Accepted predictor and redirect handshakes still determine the update edge,
while earlier valid/taken and pending-redirect predicates select the address
value. Backend completion therefore does not select data on the wide cursor
mux. The default configuration retains its existing implementation.

Normal memory-request eligibility is captured with the EX payload, and system
serialization with the CM payload. Under the existing no-wait/no-redirect
gate, request issue uses those tags and the live permission result directly.
Atomic results enter CompletionUnit through the load-result input: every legal
A encoding carries the load tag, and atomic results are already XLEN formatted.
This removes the extra result mux from ordinary arithmetic/shift/compare
forwarding without changing the retirement or dependency edges.

The offset permission checker decodes the access-size mask (0/1/3/7) in
parallel and tests the low address bits against that mask. This removes the
size-selection chain from timer alignment checks while retaining the same
byte-range checks, fault boundaries and memory transaction edges. Native and
mapped checks compare the result with bytewise effective-address permissions.
The page-end carry is decoded directly from the access size and low three
address bits, eliminating the second ripple addition after the low address
sum. Exhaustive lane/size checks and native/mapped bytewise permission tests
pass; request and retirement edges remain unchanged.

The compact predictor keeps the allocation token's occupancy in a register.
Accepting a response selects the next token's occupancy, including retirement
on that edge; retiring the current token frees it for the following edge.
Reset and recovery clear the occupancy. This removes the eight-slot lookup
from the response/fetch handshake without changing token reuse or throughput.
The request-ready predicate also factors out redundant guards while retaining
the original speculative RAM-read enables.

## Software and validation

`kernel-atomics.elf` is built explicitly with the `atomics` feature. It verifies
`misa.A`, then runs 32 AMO fetch-adds and 32 constrained LR/SC increments across
two timer-preemptible tasks. Per-task bitmaps detect duplicate or missing AMO
results; both counters, the syscall workload and prior fault/context checks
must complete. This variant builds `core` for `riscv64imac-unknown-none-elf` and uses Rust
`AtomicU64::fetch_add(Ordering::SeqCst)` plus an explicit constrained LR/SC loop.
The other variants retain the A-free `riscv64im` target; the per-image
disassembly audit admits A instructions only for the atomic variant. It must not be run as a success test on the
old console bitstream.

See [diagnostic commands](tests/rv64-a/README.md). Model success and synthesis
mapping are not routed timing or physical qualification. Placement/routing
attempts and unchanged timing constraints are recorded under the local ignored
`toolchain/measurements/2026-09-21-rv64-a/` directory.

Placement seeds must retain the qualified clock-buffer sites. The pre-promotion
mapped graph omits those buffers, so matching only graph cells loses their
placements. Apply this after constructing a placement seed:

```sh
python3 riscv/tools/preserve_clock_buffer_sites.py --graph GRAPH.json \
  --initial-placement SEED.json --output SEED-with-clocks.json
```

The board-specific profile in `qualified-clock-buffer-sites.json` identifies
PLL/JTAG output pins and resolves their current generated net names. This only
preserves placement; the routed timing and JTCK checks remain mandatory. In the
registered-completion candidate, restoring these sites changed the worst hold
slack from -435 ps to +90 ps with the same clocks and 200 ps reserves. Later
candidate 120 also closed setup timing.

### Model results (2026-09-21)

- All 24 A diagnostic ELFs pass both serialized clock orders and simultaneous
  edges (72 runs). Native and ECP5-mapped atomic ALU/LSU tests also pass.
- The 238 existing regression ELFs pass both clock orders (476 runs), with
  exactly the same commit and cycle counts as the qualified console baseline.
- All five kernel variants pass their three clock evaluations (15 runs).
  The atomic kernel records mcycle=127145 and minstret=41092 with simultaneous
  edges. This is an expanded workload, not a like-for-like performance comparison.
- The IRQ diagnostic trace contains 1080 atomic-wait samples and 89 samples of
  an interrupt pending during that wait, including 41 during the store phase.
  All 64 interrupts are accepted outside atomic wait; none bisects an AMO.
- The original XVC endpoint responds. The qualified console image still runs
  the task, access-fault and syscall kernels on hardware. The latest syscall run
  records mcycle=125412 and minstret=41206; the console transcript matches all
  686 bytes from the model.

Physical route reuse is checked against the current design's complete endpoints.
The diagnostic implementation flow retains immutable imports during route ECOs
and verifies their exact trees before emitting timing evidence. Placement and
hold-repair feedback are disabled for those imports; any remaining setup or hold
failure still prevents qualification. Timing models, clock periods and reserves
are unchanged.

The preceding alignment-mask candidate's best completed routed result has
setup slack -114 ps and hold slack +90 ps at the original clock periods with
200 ps setup/hold reserves. It is not eligible for bitstream qualification. The separately frozen direct
page-end carry implementation has passed the model checks above.
Candidates 119 and 120 pass routed STA at
0 ps / +90 ps with the same reserves. Candidate 120 also passes the integrated
JTCK check, checked bitgen and exact-bitstream hardware qualification.
The access-width predecode experiment is separately frozen and model-verified;
its best completed routed result is setup -490 ps and hold +90 ps under
the same reserves. Its two extra registers were removed from the selected RTL
because both measured layouts were worse. Clock constraints are unchanged.
A separately tested tool variant can release up to 32 neighboring data routes
when they block a local placement trial; fixed routes stay protected, and the
complete rerouted candidate must strictly improve full STA before acceptance.

The A-enabled image passes physical timing and hardware qualification at the
original configured clock frequency. Candidate 119 is timing-only evidence;
candidate 120 is the hardware-qualified artifact.

The candidate with predecoded CM ownership and EX frontend stops, factored AMO
selection, explicit completion events, registered predictor availability
and separate fetch-cursor data/write enables, with parallel alignment-mask decoding and direct page-end carry decoding,
maps to 36,952 cells before global clock promotion, including 9,783 registers, 150 BRAMs
and 16 DSPs. The ordinary regression's cycle/commit counts remain unchanged.
These are mapping/resource measurements, not routed timing results.

### Hardware qualification (2026-09-22 JST)

The selected frozen source is `riscv/build/atomic-103-source`, implemented by
candidate 120 using the tested V6 placement-owner tool. CPU/memory/JTCK periods
remain 8013/16026/166666 ps (configured 124.8/62.4/6 MHz). Native STA checks
20,618 setup and 20,618 hold endpoints, with four reviewed endpoint exceptions.
After the unchanged 200 ps setup/hold reserves, WNS is 0 ps and WHS is +90 ps;
nominal margins are therefore +200/+290 ps. There is no extra setup margin
beyond the specified reserve. The integrated JTCK cross-check agrees for all
515 setup and 515 hold checks. No independent oscillator measurement was made.

The exact generated image passes 24 A diagnostics, 238 legacy ELFs, four
kernel variants and one console run: **267/267 hardware executions**. Every
legacy ELF is matched to the previous console qualification by SHA-256:
all retired instruction counts agree, and every cycle difference is within
±1. The 24 A diagnostics likewise agree with the simultaneous-edge model in
instruction count and within ±1 cycle. These are project diagnostics, not a
claim of official A architectural certification.

| Kernel | Hardware mcycle | Hardware minstret | mcycle minus simultaneous model |
| --- | ---: | ---: | ---: |
| tasks | 86,652 | 30,725 | -1 |
| access-faults | 92,235 | 31,949 | 0 |
| syscalls | 125,413 | 41,206 | 0 |
| atomics | 127,145 | 41,092 | 0 |

The console's complete 686-byte transcript matches the model, including the
8 fault records, MEI receive response and completion of both tasks. Console
cycles include host waiting and are not fixed-work performance measurements.

- Bitstream: `build/riscv-hardware-compact-atomic-124p8-20260922.bit`
- Bitstream SHA-256: `3128297a36c001211e2c02fca5a354afd297c22962af068ebb6aedc7cddbf1b8`
- Checkpoint: `build/atomic-120.texo.json`
- Checkpoint SHA-256: `b3d03c5d92d495dcbd4a09314b1e0356db6261c1af863d29feba4d427b2296e3`
- [Qualification and exact inputs](../toolchain/measurements/2026-09-21-rv64-a/atomic-120.texo.qualification/qualification.json)
- [Hardware comparison and log hashes](../toolchain/measurements/2026-09-21-rv64-a/atomic-120.texo.qualification/hardware-review.json)
- [Physical input manifest](../toolchain/measurements/2026-09-21-rv64-a/atomic-120-physical-input-manifest.json)

From the repository root, program and run the atomic OS with:

```sh
target/release/hardware-riscv-tests --xvc 127.0.0.1:3721 \
  --bitstream riscv/build/riscv-hardware-compact-atomic-124p8-20260922.bit \
  riscv/build/rust-kernel/kernel-atomics.elf
```

The next OS/ISA stage remains U-mode/PMP isolation, followed by S-mode/Sv39.
Those privilege and address-translation features are not part of this image.
