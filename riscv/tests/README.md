# riscv/tests

Self-checking test programs and testbenches. How they fit into verification is
described in [docs/verification.md](../../docs/verification.md).

| Path | Contents | Built by |
| --- | --- | --- |
| `rv64i-subset/` | Eight smoke tests: RV64I, Zca, Zicsr, FENCE.I, traps, counters, forwarding | `rv64-riscv-tests` |
| `rv64-memory/`, `rv64-cache/`, `rv64-benchmarks/`, `rv64-predictor/` | Memory, I-cache, microbenchmark and predictor diagnostics | `rv64-riscv-tests --memory-tests` / `--cache-tests` / `--benchmarks` / `--predictor-tests` |
| `rv64-m/`, `rv64-m-benchmarks/` | M instruction tests and equal-work M benchmarks | `rv64-riscv-tests --m-tests` / `--m-benchmarks` |
| `rv64-zba/`, `rv64-zbb/`, `rv64-zbs/` | Generated B-extension tests | `rv64-riscv-tests --zba-tests` / `--zbb-tests` / `--zbs-tests` |
| `rv64-*-c/` | Paired C workloads with and without an extension | `riscv/tools/build_rv64_*_c.py` |
| `rv64-a/` | A-extension diagnostics | `riscv/tools/build_rv64_a.py` |
| `rv64-os/` | Interrupts, timer context switch, boot contract | `riscv/tools/build_rv64_machine_interrupts.py` |
| `rv64-u/`, `rv64-s/` | PMP, S-mode and Sv39 diagnostics | `riscv/kernel/build-user.py` |
| `act4-rv64-*-enabled.txt` | Reviewed lists of enabled ACT4 tests | — |
| `suite.txt`, `*.veryl` | Veryl native testbenches | `cargo run -p rica-tb -- native` |

## ELF contract

Each program is a little-endian RV64 ELF with a global `tohost` symbol. It
stores `1` on success or `(test_number << 1) | 1` on failure and then loops.
Programs must fit in the reset region (`0x0000_0000`–`0x0000_0fff`) and main
RAM (`0x8000_0000`–`0x8003_efff`). For a nonzero entry point the loader writes a
short `LUI`/`JALR` trampoline at address 0; an ELF that already occupies it is
rejected. On failure or timeout the simulators print the most recent commit
trace.

External ELFs that follow this contract can be run directly:

```sh
target/release/rv64-riscv-tests --max-cycles 1000000 path/to/test.elf
target/release/verify-riscv --hardware-memory-rtl [feature flags] path/to/test.elf
```

The assembly suites use LLVM 20's `llvm-mc` and Rust's bundled `rust-lld`; set
`LLVM_MC` or `RUST_LLD` to override them. The `rv64i-subset` RV64I sources use
`.option norvc` so that compressed coverage cannot change their instruction
streams.
