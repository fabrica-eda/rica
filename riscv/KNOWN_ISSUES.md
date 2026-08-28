# RISC-V implementation findings

## EX forwarding must not close an ALU-to-ALU combinational loop

Directly wiring the current EX ALU result through the forwarding priority mux
into the next EX operand produced an 8.72 ns mapped path. Removing forwarding
met timing but inserted a bubble into every RAW-dependent ALU pair, which is
not an acceptable CPU architecture.

The RV64 implementation now computes a low 32-bit sum and both possible high
32-bit sums, then selects the high half with the low carry. Arithmetic and
logical results are captured into dedicated bypass registers.
Consumer metadata selects that registered result before the next ALU.
Consecutive RAW ALU dependencies therefore retain CPI 1 without an
ALU-to-ALU combinational recurrence. Only loads, whose value is not present in
EX, stall once.

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

## Timing and throughput are separate gates

The nominal clock target is 150 MHz, but an implementation is not accepted
solely because it meets a 6.667 ns period. Regressions must report at least:

- clocks per committed instruction for the architectural trace
- steady-state ALU instruction throughput
- taken-branch penalty
- load-use penalty
- mapped resource counts and post-route timing

The current core commits the 20-instruction mixed trace in 39 clocks and keeps
the first eight dependent ALU/store instructions at one commit per clock.
Texo revision `0cf4b8a` closes the complete speed-8 EVN wrapper at 150 MHz with
+0.013 ns setup and +0.290 ns hold slack, checking all 2781 endpoints. The
earlier eight-bit-count mapping reached 148.92, 159.36, 150.74, and 155.30 MHz
in nextpnr seeds 1 through 4. This seed spread remains a QoR concern, but it is
no longer confused with the accepted Texo board run. The retired 250 MHz
design met timing while taking fifteen clocks per instruction; it remains in
Texo as a physical-design regression precisely so this CPU is not optimized
around that result.
