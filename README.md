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

The CPU target is 150 MHz. Performance work is measured as instruction
throughput, not Fmax alone: consecutive dependent ALU instructions now sustain
one instruction per clock through registered EX bypasses, while load-use
dependencies stall once. The current core-only nextpnr result is 164.47 MHz on
the LFE5UM5G-85F speed-8 target.

Run the I2C regressions with:

```sh
cargo run --locked --release --offline -p verify-i2c
```

See [riscv/README.md](riscv/README.md) for the CPU plan and
[i2c/README.md](i2c/README.md) for the debug protocol.
