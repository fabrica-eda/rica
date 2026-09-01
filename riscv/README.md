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
- `src/InstructionBuffer.veryl`: two-line CPU-side instruction buffer and
  memory-response capture stage
- `src/HardwareMemory.veryl`: 256 KiB unified board BRAM running at half the
  CPU frequency
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
- response-token-driven data-memory waits; no fixed load/store cycle count
- a 124.8 MHz CPU domain and 62.4 MHz BRAM domain with registered bundled-data
  crossings
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
uses `CLKI_DIV=1`, a 12 MHz phase-detector frequency, and a 624 MHz VCO.
`CLKOS` divides by five for the 124.8 MHz CPU clock and `CLKOS2` divides by ten
for the 62.4 MHz memory clock. Exact 125 MHz and the older 126 MHz configuration
require an input divider greater than one and put the phase detector below its
data-sheet range, so neither is used. The board disables unused commit payload
registers while retaining `commit_valid` for its heartbeat; the verification
build keeps the full architectural commit trace.

nextpnr does not enforce timing between different divided outputs of the same
ECP5 PLL. The report validator therefore checks both ordinary per-domain Fmax
and the two related-clock boundaries. Although memory runs at half the CPU
frequency, the phase-aligned clocks can have an intervening CPU edge, so both
directions must meet the 8.012 ns CPU period. In the current seed-6 route, the
CPU is 126.23/124.81 MHz, memory is 111.47/62.40 MHz, CPU-to-memory is
6.410/8.012 ns, and memory-to-CPU is 5.982/8.012 ns. The instruction payload
is captured first in the CPU domain; the wider data selection cone is first
registered in the memory domain. The response toggle changes only with that
registered data, so load/store completion follows the actual response rather
than an assumed pipeline length.

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
submodule. All 51 tests in the pinned RV64I catalog are enabled by
`tests/act4-rv64-enabled.txt` and pass both the ordinary Core simulator and the
synchronous hardware-memory RTL model:

```sh
git submodule update --init
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --act4
```

It also accepts externally generated self-checking ELFs. See
`tests/README.md` for the `tohost` contract, ACT4 ELF directory option, and the
exact implemented ISA subset.

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
ECP5 mapping for netlist diagnostics. This does not run the system's old
nextpnr 0.6 binary; production placement, STA, and bitgen use Texo below.

```sh
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv -- Top
cargo run --locked --offline -p verify-riscv --bin nextpnr-riscv -- HardwareTestTop
```

`HardwareTestTop` adds 256 KiB of direct-JTAG-loadable unified true-dual-port BRAM,
CPU run/reset control, and a `tohost` result mailbox. The first physical port
serves rising-edge 64-bit instruction-line fetches or debug writes while reset;
the second serves rising-edge LSU reads and writes. Both ports run at 62.4 MHz;
the CPU-side instruction buffer consumes two 32-bit instructions from each
line. After timing closure, routing, and packing
`riscv/build/riscv-hardware-test-125-texo.bit`, program it and run the bundled RV64
ELFs on the EVN with:

```sh
cargo run --locked --offline --release -p verify-riscv \
  --bin hardware-riscv-tests
```

Pass `--no-program` to reuse the current SRAM image, or pass ELF paths after
the options to run external self-checking programs. The host validates the
debug identity, loads the unified BRAM through ER1, starts the CPU, waits, and
checks the status and 64-bit `tohost` result. The physical test RAM accepts a
4 KiB reset trampoline at `0x00000000..0x00000fff` and 252 KiB of ACT4 memory
at `0x80000000..0x8003efff`. The timing-critical high region maps directly to
BRAM; the trampoline is relocated to the final physical 4 KiB.

When the USB adapter is attached to a Windows-side XVC server (including the
D2XX bridge used for the EVN),
the same runner programs the FPGA and drives the direct ER1 protocol
over TCP:

```sh
cargo run --locked --offline --release -p verify-riscv \
  --bin hardware-riscv-tests -- --xvc HOST:PORT
```

`--act4` selects all 51 pinned ACT4 RV64I ELFs. ER1 carries fixed 128-bit
commands (8-byte masked memory writes, control, and response selection), while
JTDO1 returns one stable selected response bit throughout each scan. ECP5's
registered JTDI/JSHIFT timing requires exactly 128 physical bits here; adding
the dummy bit used by the former two-bit bridge misaligns this full frame.

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
cargo install --git https://github.com/fabrica-eda/texo.git \
  --rev f8c57e09bbc0b96825652dc3b76ac5e9e1e9661d \
  --root target/texo-git --locked texo-cli
target/texo-git/bin/texo pnr riscv --top HardwareTestTop --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --pll-binding riscv/pll-12-to-124p8.json \
  --synthesis-goal-mhz 125 --jtagg-prefix jtag \
  --placement-weight-exponent 2 \
  --register-enable-fanout ff_core.csr_file.*=4 \
  --output riscv/build/riscv-hardware-test-125.texo.json
```

Generate a native bitstream only after the Texo run reports timing closure:

```sh
target/texo-git/bin/texo bitgen riscv/build/riscv-hardware-test-125.texo.json \
  --bit riscv/build/riscv-hardware-test-125-texo.bit
```

The command automatically downloads and verifies the corrected ECP5 v4 target
pack; no local architecture override is needed. The pinned Texo revision is
`f8c57e09bbc0b96825652dc3b76ac5e9e1e9661d` (Texo 0.1.6). On the current
256 KiB direct-JTAG wrapper maps to 16,461 ECP5 cells (13,545 implemented
cells, including 136 DP16KD and one JTAGG). Placement weight exponent 2 closes
124.8 MHz at +0.014 ns setup WNS, zero setup TNS, +0.290 ns hold WNS, and zero
hold TNS with Texo 0.1.6.

The Texo bitstream passes JTAG detection, SRAM programming, direct ER1 ELF
upload, CPU execution, and 64-bit mailbox readback for all six bundled RV64
ELFs and all 51 enabled ACT4 RV64I ELFs on the current 256 KiB wrapper at
124.8 MHz through the Windows D2XX XVC transport.
Use `--um5g-85k`, not `--85k`: the latter targets LFE5U and produces a
bitstream with the wrong device ID and configuration semantics.
