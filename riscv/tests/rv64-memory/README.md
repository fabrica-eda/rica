# Data-access diagnostics

These RV64I self-checks extend the one-word `load_store` benchmark with
read-only streaming, disjoint source/destination copying and dependent loads.
They are a separate suite and do not change the historical four-program score.

| ELF | Work |
| --- | --- |
| array_sum | Four passes over 256 distinct 64-bit values; verifies the sum |
| array_copy | Four copies of 256 words; verifies every destination word |
| pointer_chase | Four traversals of 256 nodes in a stride-73 permutation; checks the final pointer and address sum |

Build/run with `rv64-riscv-tests --memory-tests --compact-predictor --max-cycles 100000`.
Use the resulting ELFs with the hardware-memory model and hardware runner for
latency/performance comparisons. Source data is loaded before measurement;
copy verification is included in its reported execution interval. Each run
starts with fresh processor state. Keep ELF hashes and clock settings fixed.
