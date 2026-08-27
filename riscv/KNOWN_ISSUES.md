# RISC-V implementation findings

## EX forwarding must not close an ALU-to-ALU combinational loop

Directly wiring the current EX ALU result through the forwarding priority mux
into the next EX operand produced an 8.72 ns mapped path. Removing forwarding
met timing but inserted a bubble into every RAW-dependent ALU pair, which is
not an acceptable CPU architecture.

The implementation now computes a 16+16-bit carry-select result and captures
arithmetic and logical results into dedicated bypass registers. Consumer
metadata selects that registered result before the next ALU. Consecutive RAW
ALU dependencies therefore retain CPI 1 without an ALU-to-ALU combinational
recurrence. Only loads, whose value is not present in EX, stall once.

## Data-dependent flushes must be registered

Even after fixing the bypass data path, using the EX branch comparison to
flush every younger pipeline register in the same cycle routed at only
133.60 MHz. The branch decision now terminates at `redirect_pending_q`; the
following cycle squashes wrong-path EX/MEM valid bits and redirects fetch.
This keeps branch-data fanout out of the register-enable network.

## Synchronous memory latency must remain local

The reusable `RegisterFile` infers two 32x32 1R1W memories so that two
independent read ports can map to four ECP5 `DP16KD` blocks. Its synchronous
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

The current core commits the 16-instruction mixed trace in 34 clocks and keeps
the first eight dependent ALU/store instructions at one commit per clock. Its
core-only speed-8 nextpnr result is 164.47 MHz. The retired 250 MHz design met
timing while taking fifteen clocks per instruction; it remains in Texo as a
physical-design regression precisely so this CPU is not optimized around that
result.
