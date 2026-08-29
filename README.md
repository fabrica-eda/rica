# RICA RISC-V bring-up

This workspace now contains the RISC-V implementation under `riscv/` and
the I2C block used to establish host-driven hardware debug under `i2c/`.

The former fifteen-cycle 250 MHz `Core250` design was a useful synthesis,
placement, routing, BRAM, PLL, JTAGG, setup, and hold stress case, but it is
not the CPU architecture being developed here. That frozen design and its
structural regression have moved to Texo's `examples/core250-qor`.

Run the active CPU's RTL, synthesis, and mapping-equivalence regression with:

```sh
cargo run --locked --offline -p verify-riscv
```

The pipelined core's expensive post-map Celox regression is available
explicitly with `cargo run --locked --offline -p verify-riscv -- --post-map`.

The nominal CPU target is 125 MHz. The data-sheet-valid board PLL runs at
124.8 MHz. Performance work is measured as instruction
throughput, not Fmax alone: consecutive dependent ALU instructions sustain one
instruction per clock through registered EX bypasses, while load-, shift-, and
compare-use dependencies wait only for their producing stage. Board BRAM runs
at 62.4 MHz behind response-token handshakes and a two-line instruction buffer.
The current nextpnr seed-6 route reaches 126.23 MHz in the CPU domain and also
passes explicit 8.012 ns related-clock checks in both directions. The same
bitstream passes all six bundled ELFs on the physical EVN, both over direct USB
and through an openFPGALoader XVC server. Texo revision `18b4932` improves this
design to -0.313 ns WNS at the same target, but does not yet close 125 MHz.

Run the I2C regressions with:

```sh
cargo run --locked --release --offline -p verify-i2c
```

See [riscv/README.md](riscv/README.md) for the CPU plan and
[i2c/README.md](i2c/README.md) for the debug protocol.
