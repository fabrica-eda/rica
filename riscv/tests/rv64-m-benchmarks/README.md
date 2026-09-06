# Matched arithmetic workloads

`generate_rv64_m_benchmarks.py` generates four self-checking programs. Each
processes the same 256 pairs of 64-bit integers, calls an arithmetic function
and verifies the accumulated result against Python integer arithmetic.
The second operand alternates between 8, 16, 32 and 64 significant bits.
Both variants include the same input loads, call/return and checksum loop.

| Pair | RV64I implementation | RV64M implementation |
| --- | --- | --- |
| `soft_multiply` / `m_multiply` | Shift/add, stopping when the multiplier becomes zero | MUL |
| `soft_divide` / `m_divide` | Normalize divisor, then shift/subtract quotient bits | DIVU |

The software functions are explicit reference routines in the generator;
these results do not represent every compiler's runtime library. Both are
operand dependent, while the present M unit uses fixed iteration counts.
Use cycles/time for equal work to compare these programs. Their instruction
counts differ, so IPC or MIPS alone cannot express the acceleration.

```sh
python3 riscv/tools/generate_rv64_m_benchmarks.py
target/release/rv64-riscv-tests --m-benchmarks --compact-predictor --max-cycles 1000000
target/release/verify-riscv --hardware-memory-rtl --compact-predictor \
  --icache-kib 4 --fast-lsu 5 --m-extension --max-cycles 1000000 \
  riscv/build/rv64-m-benchmarks/*.elf
```

Only the `soft_` ELFs can run on the baseline image without M. Hardware runs
must preserve each ELF hash and use the same CPU/memory clocks.
