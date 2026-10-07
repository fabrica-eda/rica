# RV64 architecture microbenchmarks

These self-checking, multiplication-free RV64 programs isolate four costs:

| Program | Work | Expected result |
| --- | --- | --- |
| `alu_chain` | 256 iterations of 16 dependent ADDIs | 4096 |
| `branch_pattern` | 512 iterations of a repeating N,N,T,T branch | 256 increments |
| `call_return` | 256 compressed indirect calls/returns with PC+2 links | 768 |
| `load_store` | 256 load-use/store dependencies to one aligned word | 256 |

Run `rv64-riscv-tests --benchmarks --max-cycles 100000`, then repeat with
`--bimodal` and `--static-predictor`. The runner builds the same ELFs for all configurations, compiles
one model per configuration, and creates fresh simulator state per ELF so
predictor training cannot leak between measurements. Every program checks its
result before reporting through the existing `tohost` protocol.

The ordinary Core model has immediate instruction/data access. Its reported
interval runs from reset release through the terminal store request; its
commit count excludes that store. Run the generated
`riscv/build/rv64-benchmarks/*.elf` through `verify-riscv --hardware-memory-rtl`
and repeat with both predictor flags for the actual 2:1 clock ratio and instruction
buffer. That model includes the acknowledged terminal store in its commit
count and includes predictor reset scrubbing in its cycle count. It also
reports the first commit cycle and the inclusive first-to-last-commit
execution window. Use that window when comparing execution without the
dynamic predictors' 2048-cycle reset sweep. This removes startup latency;
it does not pretrain the predictor or warm a cache. Both coincident-edge
orders are tested. Compare like intervals and memory models.

These are diagnostic workloads, not a representative application score.
In particular, `load_store` measures one-word dependency latency, not cache
capacity or streaming bandwidth.
