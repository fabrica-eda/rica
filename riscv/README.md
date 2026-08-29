# RISC-V implementation

This directory contains only the CPU being developed and its reusable
building blocks:

- `src/PipelineInterfaces.veryl`: typed decode and execute stage contracts
- `src/Decoder.veryl`: RV64I-subset instruction decoder
- `src/ExecuteUnit.veryl`: two-slice carry-select integer ALU
- `src/Core.veryl`: pipeline state, hazard control, forwarding, traps, and commit
- `src/CsrFile.veryl`: machine-mode CSR and trap state
- `src/RegisterFile.veryl`: synchronous two-read/one-write register-file
  candidate
- `src/Top.veryl`: 124.8 MHz ECP5 EVN PLL/LED bring-up wrapper
- `verify/`: architectural, synthesis, mapping-equivalence, and optional
  post-map regressions

The frozen non-overlapped `Core250` timing stress design has moved to Texo's
`examples/core250-qor`; it is no longer the CPU architecture or QoR fixture
owned by this repository.

## Performance target

The nominal CPU timing target is 125 MHz (8.000 ns). The board clock is
124.8 MHz (8.013 ns). Fmax is not accepted as a performance result by itself.
The current overlapped implementation has:

- CPI 1 through consecutive dependent ALU instructions using registered EX
  bypasses
- one localized wait for load-use, shift-use, and compare-use dependencies;
  independent shifts and comparisons still enter every clock
- synchronous BRAM register reads without a fixed per-instruction tax
- pipelined 16-bit comparison chunks completed in EX2, keeping the comparison
  reduction and data-dependent flush fanout off the ALU critical path
- a registered instruction-memory address, keeping the next-PC addition and
  board ROM decode in separate timing stages without reducing fetch throughput
- 64-bit architectural registers and data with a 32-bit physical address
  space

The architectural regression currently commits 58 instructions in 102 clocks
(1.759 CPI), including RV64 word operations, load-use and shift-use
dependencies, and taken BEQ, JAL, and JALR redirects. The independent RV64
comparison regression completes 16 commits in 46 clocks; four immediately
dependent SLT-to-branch pairs each pay the intended single wait.

The EVN has a 12 MHz reference and an integer ECP5 PLL. The board configuration
uses `CLKI_DIV=1`, a 12 MHz phase-detector frequency, a 624 MHz VCO, and a
divide-by-five `CLKOS` output to generate 124.8 MHz. Exact 125 MHz and the older
126 MHz configuration require an input divider greater than one and put the
phase detector below its data-sheet range, so neither is used. The board
disables unused commit payload registers
while retaining `commit_valid` for its heartbeat; the verification build keeps
the full architectural commit trace. Struo estimates the core at 7.160 ns.
nextpnr 0.6 seed 4 routes the complete hardware wrapper at 125.88 MHz and
passes its derived 124.8 MHz clock constraint with 68 ps setup slack. The
resulting bitstream passes all six bundled RV64 hardware tests, including the
machine-mode CSR/trap regression.

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

Export either the current core or the complete board wrapper's 125 MHz-targeted
ECP5 mapping for nextpnr diagnostics:

```sh
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv -- Top
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv -- HardwareTestTop
nextpnr-ecp5 --um5g-85k --package CABGA381 --speed 8 \
  --json riscv/build/riscv-hardware-test-125.nextpnr.json \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --textcfg riscv/build/riscv-hardware-test-125.bit.config \
  --freq 125 --seed 4
ecppack riscv/build/riscv-hardware-test-125.bit.config \
  riscv/build/riscv-hardware-test-125.bit --compress --freq 62.0
```

`HardwareTestTop` adds 4 KiB of JTAG-I2C-loadable Harvard BRAM, CPU run/reset
control, and a `tohost` result mailbox. After routing and packing
`riscv/build/riscv-hardware-test-125.bit`, program it and run the bundled RV64
ELFs on the EVN with:

```sh
cargo run --locked --offline --release -p verify-riscv \
  --bin hardware-riscv-tests
```

Pass `--no-program` to reuse the current SRAM image, or pass ELF paths after
the options to run external self-checking programs. The host validates the
debug identity, loads both instruction and data BRAM through ER1, starts the
CPU, waits, and checks the status and 64-bit `tohost` result. The physical test
RAM accepts `0x00000000..0x000007ff` and
`0x80000000..0x800007ff`.

The board wrapper binds the EVN's 12 MHz reference to a 124.8 MHz EHXPLLL. Its
ROM increments x31 immediately after reset and then enters a delay/branch loop
before the next increment. D5 through D10 show x31[5:0] as an active-low binary
count, D11 toggles from a separately divided retired-instruction count, and D12
stays lit while the PLL is locked.
These independently expose register writeback, pipeline commit progress, and
clock/reset status. In particular, D5 must light almost immediately; later
binary-count changes additionally prove that the delay branch and redirect
completed. The RTL regression uses the same ROM control-flow shape with 4096
delay iterations and requires every ADDI/BNE pair to retire before x31 may
advance. `Top` registers the x31 polarity conversion, so its display
trails by one CPU cycle without putting those LED pads directly on the
CPU's combinational output path. The PLL lock alone releases CPU reset; the
generic SW4 input is not required for autonomous bring-up.
Use the same PLL binding for the Texo flow:

```sh
../texo/target/release/texo pnr riscv --top HardwareTestTop --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --pll-binding riscv/pll-12-to-124p8.json \
  --synthesis-goal-mhz 125 --jtagg-prefix jtag \
  --output riscv/build/riscv-hardware-test-125.texo.json
```

Generate a native bitstream only after the Texo run reports timing closure:

```sh
../texo/target/release/texo bitgen riscv/build/riscv-hardware-test-125.texo.json \
  --bit riscv/build/riscv-hardware-test-125-texo.bit
```

The command automatically downloads and verifies the corrected ECP5 v4 target
pack; no local architecture override is needed. The currently pinned Texo
revision does not yet close this enlarged CSR/trap implementation: its timing
optimizer reached negative setup slack and then restarted placement for a
physical-synthesis candidate. nextpnr is therefore the current sign-off and
hardware-tested flow; an unfinished Texo run must not be reported as closure.

JTAG detection, SRAM programming, ELF upload, CPU execution, and mailbox
readback are verified on the LFE5UM5G-85 EVN at 124.8 MHz. The complete `alu`,
`compare`, `control`, `memory`, `shift`, and `system` hardware suite passes on
the current nextpnr bitstream. Use `--um5g-85k`, not `--85k`: the latter targets
LFE5U and produces a bitstream with the wrong device ID and configuration
semantics.
