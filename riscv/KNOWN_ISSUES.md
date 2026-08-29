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

The current core commits the 58-instruction mixed trace in 102 clocks and
keeps consecutive dependent one-cycle integer ALU operations at one per
clock. With the 124.8 MHz PLL, the complete wrapper reaches 131.37 MHz in
nextpnr 0.6 seed 4 and the resulting bitstream passes six complete hardware
suites (30/30). Texo currently applies a separate clock-uncertainty policy, so
its WNS is not used as a nextpnr-compatible QoR result. The retired 250 MHz
design met timing while taking fifteen clocks per instruction; it remains in
Texo as a physical-design regression precisely so this CPU is not optimized
around that result.
