# Branch-predictor diagnostics

Build with `rv64-riscv-tests --predictor-tests --compact-predictor --max-cycles 100000`,
which checks correctness on the single-clock core model. For performance, run
the generated ELFs on the board-memory model:

```sh
target/release/verify-riscv --hardware-memory-rtl --compact-predictor --icache-kib 4 \
  riscv/build/rv64-predictor/btb_conflict.elf riscv/build/rv64-predictor/call_nested.elf
```

- `call_nested`: one outer function is called from two places and calls an inner
  function. The return address alternates, which separates a BTB that remembers
  only the last target from a return-address stack. It also uses compressed
  `PC+2` return addresses.
- `btb_conflict`: a jump and a conditional branch 64 bytes apart, repeated. They
  collide in a 32-entry table indexed by halfword PC and separate in the
  64-entry table.

These diagnostics target specific mechanisms and are not part of the
four-program benchmark mean. Reported cycles include setup and result checks.
