# RISC-V implementation

This directory contains only the CPU being developed and its reusable
building blocks:

- `src/PipelineInterfaces.veryl`: typed decode and execute stage contracts
- `src/Decoder.veryl`: RV32I-subset instruction decoder
- `src/ExecuteUnit.veryl`: carry-select integer ALU and branch comparison
- `src/Core.veryl`: pipeline state, hazard control, forwarding, and commit
- `src/RegisterFile.veryl`: synchronous two-read/one-write register-file
  candidate
- `src/Top.veryl`: 150 MHz ECP5 EVN PLL/LED bring-up wrapper
- `verify/`: architectural, synthesis, mapping-equivalence, and optional
  post-map regressions

The frozen non-overlapped `Core250` timing stress design has moved to Texo's
`examples/core250-qor`; it is no longer the CPU architecture or QoR fixture
owned by this repository.

## Performance target

The nominal CPU target is 150 MHz (6.667 ns). Fmax is not accepted as a
performance result by itself. The current overlapped implementation has:

- CPI 1 through consecutive dependent ALU instructions using registered EX
  bypasses
- one localized load-use stall
- synchronous BRAM register reads without a fixed per-instruction tax
- registered branch squash control, keeping data-dependent flush fanout off
  the critical path

The architectural regression currently commits 16 instructions in 34 clocks,
including a load-use dependency and taken BEQ, JAL, and JALR redirects.

For the LFE5UM5G-85F speed-8 out-of-context build, Struo reports a 5.72 ns
post-retiming estimate. nextpnr-ecp5 0.6 routes the same mapping at 164.47 MHz
(6.080 ns), passing 150 MHz by 0.586 ns. Texo is pinned to revision `b452fb7`,
which includes common-clock-path pessimism removal and endpoint-coverage
hardening. On that revision Texo reports +0.185 ns setup and +0.290 ns hold
slack with all 1746 modeled endpoints checked. The pre-CPPR `d776ddc` result
must not be used for this design.

The complete EVN wrapper, including the 12-to-150 MHz PLL, ROM, and package
pins, closes at +0.026 ns setup and +0.290 ns hold with all 1914 modeled
endpoints checked. This board-level margin is valid but narrow.

## Verification

Run RTL simulation, Struo mapping, and equivalence checks:

```sh
cargo run --locked --offline -p verify-riscv
```

Compiling the post-map simulation model can take several minutes. Run that
regression explicitly when needed:

```sh
cargo run --locked --offline -p verify-riscv -- --post-map
```

Test only the reusable synchronous register file:

```sh
cargo run --locked --offline -p verify-riscv -- --regfile-only
```

Export the current core's 150 MHz-targeted ECP5 mapping for nextpnr diagnostics:

```sh
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv
```

The board wrapper binds the EVN's 12 MHz reference to a 150 MHz EHXPLLL and
runs a small ROM program whose x31 binary count is shown on the eight
active-low LEDs. The firmware counts x31 downward from all ones, so the visible
LED pattern counts upward without adding output-inversion LUTs.
Build and program it with the pinned Texo revision using:

```sh
texo pnr riscv --top Top --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --pll-binding riscv/pll-12-to-150.json \
  --synthesis-goal-mhz 150 \
  --output riscv/build/riscv-top-150.texo.json
texo bitgen riscv/build/riscv-top-150.texo.json \
  --bit riscv/build/riscv-top-150.bit
openFPGALoader -b ecp5_evn --write-sram riscv/build/riscv-top-150.bit
```

JTAG device detection, native bitgen, and SRAM programming have been verified
on the LFE5UM5G-85 EVN. Host-visible CPU state is not yet wired into I2C/JTAG;
the independently verified transport remains under `../i2c`.
