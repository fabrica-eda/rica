# RISC-V implementation

This directory contains only the CPU being developed and its reusable
building blocks:

- `src/PipelineInterfaces.veryl`: typed decode and execute stage contracts
- `src/Decoder.veryl`: RV64I-subset instruction decoder
- `src/ExecuteUnit.veryl`: two-slice carry-select integer ALU
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
- a pipelined 64-bit branch equality reduction, keeping the comparison and
  data-dependent flush fanout off the ALU critical path
- a registered instruction-memory address, keeping the next-PC addition and
  board ROM decode in separate timing stages without reducing fetch throughput
- 64-bit architectural registers and data with a 32-bit physical address
  space

The architectural regression currently commits 20 instructions in 39 clocks,
including RV64 word operations, a load-use dependency, and taken BEQ, JAL,
and JALR redirects.

For the LFE5UM5G-85F speed-8 build, Struo reports a 6.560 ns mapped estimate.
Texo revision `0cf4b8a` routes the complete EVN wrapper at the 6.667 ns target
with 0.000 ns setup slack and +0.290 ns hold slack, covering all 2732 modeled
endpoints. A repeated run produced the same slacks. Routing the exact same
`Top` mapping with nextpnr-ecp5 0.6 produced 148.92, 159.36, 150.74, and
155.30 MHz for seeds 1 through 4: three of four seeds close at 150 MHz. The
Texo result is the board-level acceptance run; the seeded nextpnr spread is
retained as a placement-QoR diagnostic rather than hidden behind a best seed.

## Verification

Run RTL simulation, Struo mapping, and equivalence checks:

```sh
cargo run --locked --offline -p verify-riscv
```

Build and run the independent self-checking RV64 ELF suite:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests
```

The official ACT4 tests are pinned as the `third_party/riscv-arch-test`
submodule. The runner discovers the complete RV64I catalog there and reports
tests not yet enabled by `tests/act4-rv64-enabled.txt` as `IGNORE`:

```sh
git submodule update --init
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --act4
```

It also accepts externally generated self-checking ELFs. See
`tests/README.md` for the `tohost` contract, ACT4 ELF directory option, and the
exact implemented ISA subset; this is not yet a full RV64I certification
claim.

Compiling the post-map simulation model can take several minutes. Run that
regression explicitly when needed:

```sh
cargo run --locked --offline -p verify-riscv -- --post-map
```

Test only the reusable synchronous register file:

```sh
cargo run --locked --offline -p verify-riscv -- --regfile-only
```

Export either the current core or the complete board wrapper's 150 MHz-targeted
ECP5 mapping for nextpnr diagnostics:

```sh
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv -- Top
```

The board wrapper binds the EVN's 12 MHz reference to a 150 MHz EHXPLLL and
runs a small ROM program whose x31 binary count is shown on the eight
active-low LEDs. `Top` registers the polarity conversion, so the human-visible
display trails x31 by one 150 MHz cycle without putting the LED pads directly
on the CPU's combinational output path.
Build and program it with the pinned Texo revision using:

```sh
../texo/target/release/texo pnr riscv --top Top --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --pll-binding riscv/pll-12-to-150.json \
  --synthesis-goal-mhz 150 \
  --output riscv/build/riscv-top-150.texo.json
../texo/target/release/texo bitgen riscv/build/riscv-top-150.texo.json \
  --bit riscv/build/riscv-top-150.bit
openFPGALoader -b ecp5_evn --write-sram riscv/build/riscv-top-150.bit
```

JTAG device detection, native bitgen, and SRAM programming of this timing-closed
RV64 build have been verified on the LFE5UM5G-85 EVN. Host-visible CPU state is
not yet wired into I2C/JTAG; the independently verified transport remains under
`../i2c`.
