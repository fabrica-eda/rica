# A-extension diagnostics

Build with `python3 riscv/tools/build_rv64_a.py`. Outputs and a hash manifest go to
`riscv/build/rv64-a/`; the 18 AMO programs are generated from a Python integer
oracle. Handwritten programs cover LR/SC, reservation invalidation, precise
faults, illegal encodings, upper-word byte lanes, wrong-path AMOs and timer IRQs.
The compiler is selected by `RISCV_CLANG` or `clang`; Nix wrappers are resolved
to their original compiler for cross assembly.

```sh
target/release/verify-riscv --hardware-memory-rtl --strict-memory --a-extension \
  --machine-interrupts --compact-predictor --icache-kib 4 --fast-lsu 6 \
  --dsp-multiplier --zba-extension --zbb-extension --zbs-extension \
  --early-bitmanip --early-taken-fetch --early-divide --max-cycles 1000000 \
  riscv/build/rv64-a/*.elf
```

Add `--timed-clocks` for simultaneous edges. The ordinary command runs both
serialized CPU/memory clock orders. These are self-checking ISA/platform tests,
not a substitute for ACT/Sail or routed timing/hardware qualification.

The AMO programs check all four aq/rl combinations, old-value results, memory
results, rd=x0 side effects, destination/source aliases and immediate consumers.
`atomic-interrupts` sweeps 64 timer delays around AMO execution and requires one
update and one interrupt per iteration. `atomic-faults` checks PC/cause/mtval and
that mispredicted AMOs have no memory effect. `atomic-illegal` tests reserved
widths/funct5 and LR with nonzero rs2. `atomic-offset` checks AMO.W/LR.W/SC.W at
byte offset 4 without corrupting the adjacent word.
`atomic-tohost` terminates through an AMO store to exercise the board's counter
capture boundary. Its simultaneous-edge model reports minstret=7, including
the terminal AMO; compare that count when qualifying the hardware image.

`atomic_decoder`, `atomic_alu`, and `atomic_lsu` Rust tests independently cover
encoding legality and native/ECP5 mapped execution. LSU tests include delayed
responses, response backpressure, reset/invalidation, failed-SC permission
checks and clearing the store indication before a successor can observe it.
