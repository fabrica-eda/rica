# RV64 instruction-cache checks

These self-checking ELFs use the board's main RAM at `0x80000000`, including
an automatically installed reset trampoline. Build and run them with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- \
  --static-predictor --cache-tests --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin verify-riscv -- \
  --static-predictor --icache-kib 8 --hardware-memory-rtl \
  riscv/build/rv64-cache/*.elf
```

`--icache-kib` accepts 0 (the legacy two-line buffer), 2, 4 or 8. The hardware
runner uses the real synchronous BRAM organization and runs both orderings of
coincident CPU/memory edges. The first command uses an immediate-memory Core
model; only the second command measures instruction-cache behavior.

- `fence_i.S` warms and modifies executable instructions, then checks that
  `FENCE.I` discards the old cache and pipeline contents. It covers the next
  sequential instruction, a 32-bit instruction spanning two lines, a compressed
  instruction at the final halfword, and nonzero reserved FENCE.I operands.
  Its repeated invalidations measure correctness and expose scrub overhead;
  it is not included in the throughput score.
- `capacity_3k.S` executes eight passes over 768 uncompressed ADDI instructions.
  Its loop body is larger than 2 KiB and fits in 4 KiB.
- `capacity_6k.S` executes four passes over 1536 uncompressed ADDI instructions.
  Its loop body is larger than 4 KiB and fits in 8 KiB. The backward jump uses
  JAL because the loop exceeds a conditional branch's immediate range.

The capacity probes each check a final sum of 6144. They show capacity and
cold-fill costs, rather than representing a general application workload.
