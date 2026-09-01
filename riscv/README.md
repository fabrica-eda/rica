# RISC-V implementation

This directory contains only the CPU being developed and its reusable
building blocks:

- `src/PipelineInterfaces.veryl`: typed decode and execute stage contracts
- `src/CompressedDecoder.veryl`: RV64C/Zca integer decompressor to canonical
  32-bit RV64I instructions
- `src/Decoder.veryl`: canonical RV64I-subset instruction decoder
- `src/ExecuteUnit.veryl`: two-slice carry-select integer ALU
- `src/Core.veryl`: scalar one-wide pipeline state, hazard control, forwarding,
  traps, and commit
- `src/CsrFile.veryl`: machine-mode CSR and trap state
- `src/PerformanceCounter64.veryl`: exact split-carry 64-bit event counter
- `src/PerformanceEventCounters4.veryl`: four 64-bit pipeline-event counters
  sharing one selected-word incrementer
- `src/RegisterFile.veryl`: synchronous two-read/one-write register-file
  candidate
- `src/InstructionBuffer.veryl`: two-line CPU-side instruction buffer and
  halfword-addressable 32-bit instruction window
- `src/CompressedDecoderTimingTop.veryl`: registered out-of-context timing
  fixture for the decompressor alone
- `src/HardwareMemory.veryl`: 256 KiB unified board BRAM running at half the
  CPU frequency
- `src/Top.veryl`: 124.8 MHz ECP5 EVN PLL/LED bring-up wrapper
- `verify/`: architectural, synthesis, and mapping-equivalence regressions

The frozen non-overlapped `Core250` timing stress design has moved to Texo's
`examples/core250-qor`; it is no longer the CPU architecture or QoR fixture
owned by this repository.

## Performance target

The nominal CPU timing target is 125 MHz (8.000 ns). The board clock is
124.8 MHz (8.013 ns). Fmax is not accepted as a performance result by itself.
The current overlapped implementation has:

- a scalar, one-wide frontend: it accepts at most one 16- or 32-bit
  instruction per clock and contains no dual-issue or superscalar logic
- registered raw-fetch and decompression stages, with the PC advancing by two
  or four bytes and 32-bit instructions supported across an eight-byte line
  boundary
- CPI 1 through consecutive dependent ALU instructions using registered EX
  bypasses
- one localized wait for load-use, shift-use, and compare-use dependencies;
  independent shifts and comparisons still enter every clock
- one empty EX predecessor for each CSR instruction, keeping architectural
  CSR ordering and counter carry logic out of the ordinary ALU path
- response-token-driven data-memory waits; no fixed load/store cycle count
- a 124.8 MHz CPU domain and 62.4 MHz BRAM domain with registered bundled-data
  crossings
- pipelined 16-bit comparison chunks completed in EX2, keeping the comparison
  reduction and data-dependent flush fanout off the ALU critical path
- a registered instruction-memory address, keeping the next-PC addition and
  board ROM decode in separate timing stages without reducing fetch throughput
- 64-bit architectural registers and data with a 32-bit physical address
  space

The architectural regression currently commits 58 instructions in 110 clocks
(1.897 CPI), including RV64 word operations, load-use and shift-use
dependencies, and taken BEQ, JAL, and JALR redirects. The independent RV64
comparison ELF completes 16 commits in 51 clocks. The bundled ELF suite now
contains eight programs; all eight pass both the ordinary simulator and both
coincident-edge serializations of the dual-clock HardwareMemory model. This
includes the 143-commit compressed test in 234 ordinary-Core clocks and a
dedicated counter/readback test.

The RV64 `mcycle` and `minstret` counters are writable at 0xB00/0xB02; the
standard 0xC00 `cycle` and 0xC02 `instret` addresses are read-only aliases.
Two custom machine CSRs expose implementation event counts without widening
the four-bit internal CSR index: 0x7C0 selects a counter and 0x7C1 reads or
writes its 64-bit value. Selector 0 is a dependency bubble, 1 is a data-memory
wait, 2 is redirect generation/application, and 3 is a front-end bubble.
These are control events rather than a decomposition of CPI: a front-end
bubble can coexist with an older retirement. At most one class advances per
clock using the controller's priority order: memory, redirect, dependency,
then front end. `PerformanceCounter64` uses parallel 32-bit low/high increments
for the architectural counters. `PerformanceEventCounters4` stores four words
but selects only the active word before a shared incrementer. The event inputs
are registered for timing; a delayed freeze preserves the final pending event.

The EVN has a 12 MHz reference and an integer ECP5 PLL. The board configuration
uses `CLKI_DIV=1`, a 12 MHz phase-detector frequency, and a 624 MHz VCO.
`CLKOS` divides by five for the 124.8 MHz CPU clock and `CLKOS2` divides by ten
for the 62.4 MHz memory clock. Exact 125 MHz and the older 126 MHz configuration
require an input divider greater than one and put the phase detector below its
data-sheet range, so neither is used. The board disables unused commit payload
registers while retaining `commit_valid` for its heartbeat; the verification
build keeps the full architectural commit trace.

The production timing flow checks ordinary per-domain paths and both
related-clock boundaries. Although memory runs at half the CPU frequency, the
phase-aligned clocks can have an intervening CPU edge, so both directions must
meet the 8.012 ns CPU period. The instruction payload is captured first in the
CPU domain; the wider data selection cone is first registered in the memory
domain. The response toggle changes only with that registered data, so
load/store completion follows the actual response rather than an assumed
pipeline length.

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
submodule at `f7ba06c3d1a01428b172fe851ab56931a0624fee`. All 51 tests in the
pinned RV64I catalog are enabled by `tests/act4-rv64-enabled.txt` and pass the
ordinary Core simulator and the synchronous hardware-memory RTL model:

```sh
git submodule update --init
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --act4
```

The same runner selects the complete 32-test Zca catalog and its independent
enable list with `--act4-suite Zca`. At the pinned submodule revision, all 32
official self-checking Zca ELFs pass both the ordinary Core simulator and the
dual-clock synchronous HardwareMemory model. The Zca ELF set digest is
`8938083a2a0e5cd9ae7b74c6b1f6be9c8afec0da593b07478c8385a421e2a12e`.
Existing `--act4` invocations continue to select the RV64I catalog.

The dual-clock harness deliberately runs every supplied ELF twice, serializing
coincident edges as both `mem->cpu` and `cpu->mem`. This prevents either a
same-edge request or response shortcut from hiding a handshake bug:

```sh
cargo run --locked --offline -p verify-riscv -- --hardware-memory-rtl
```

It also accepts externally generated self-checking ELFs. See
`tests/README.md` for the `tohost` contract, ACT4 ELF directory option, and the
exact implemented ISA subset.

Test only the reusable synchronous register file:

```sh
cargo run --locked --offline -p verify-riscv -- --regfile-only
```

To measure only the registered compressed-decode cone at 126 MHz, run the
dedicated fixture. This is a decompressor check, not proof that the complete
CPU meets the same frequency:

```sh
target/texo-git/bin/texo pnr riscv --top CompressedDecoderTimingTop \
  --package CABGA381 --speed 8 --lpf riscv/core-126.lpf \
  --allow-unconstrained-io --synthesis-goal-mhz 126 \
  --output riscv/build/compressed-decoder-126.texo.json
```

The current fixture closes at +0.607 ns setup WNS and +0.323 ns hold WNS.

`HardwareTestTop` adds 256 KiB of direct-JTAG-loadable unified true-dual-port BRAM,
CPU run/reset control, and a `tohost` result mailbox. The first physical port
serves rising-edge 64-bit instruction-line fetches or debug writes while reset;
the second serves rising-edge LSU reads and writes. Both ports run at 62.4 MHz;
the CPU-side instruction buffer exposes a halfword-aligned window spanning up
to two adjacent lines. After timing closure, routing, and packing
`riscv/build/riscv-hardware-test-125-texo.bit`, program it and run the eight
bundled RV64 ELFs on the EVN with:

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

`tools/windows_d2xx_xvc.py` provides that bridge without replacing the
FT2232H's Windows D2XX driver. It uses only Python's standard library and
`ftd2xx.dll`, validates that the selected device is interface A of an
FT2232H, and defaults to loopback-only listening. For WSL, obtain the Windows
gateway address with `ip route show default`, then start the script from
Windows PowerShell with that address explicitly:

```powershell
python.exe \\wsl.localhost\DISTRO\path\to\rica\riscv\tools\windows_d2xx_xvc.py `
  --listen WSL_GATEWAY --port 3721
```

Use the same `WSL_GATEWAY:3721` with `--xvc`. Do not bind the bridge to
`0.0.0.0`; JTAG access must remain limited to the WSL virtual network or
loopback.

`--act4` selects all 51 pinned ACT4 RV64I ELFs;
`--act4 --act4-suite Zca` selects previously generated Zca ELFs. ER1 carries
fixed 128-bit commands (8-byte masked memory writes, control, and response
selection), while JTDO1 returns one stable selected response bit throughout
each scan. ECP5's
registered JTDI/JSHIFT timing requires exactly 128 physical bits here; adding
the dummy bit used by the former two-bit bridge misaligns this full frame.
Only a terminal `tohost` store acknowledged by the synchronous memory response
token is captured. On that edge the store still retires; the following frozen
snapshot therefore includes its `minstret` increment and every preceding
memory-wait event. Response banks 5/6 expose `mcycle`, `minstret`, and the four
pipeline-event counters. The XVC runner reads and validates all six 64-bit
values, then prints a `METRIC` line including CPI for every ELF. Protocol
version 0x05 identifies this ACK-gated response layout.

After programming once, run the two physical ACT4 catalogs with:

```sh
cargo run --locked --offline --release -p verify-riscv \
  --bin hardware-riscv-tests -- --no-program --xvc HOST:PORT --act4
cargo run --locked --offline --release -p verify-riscv \
  --bin hardware-riscv-tests -- \
  --no-program --xvc HOST:PORT --act4 --act4-suite Zca
```

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
target/texo-git/bin/texo pnr riscv --top HardwareTestTop \
  --device LFE5UM5G-85F --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf \
  --pll-binding riscv/pll-12-to-124p8.json \
  --synthesis-goal-mhz 125 --jtagg-prefix jtag \
  --placement-weight-exponent 4 \
  --register-enable-fanout 'ff_core.csr_file.mcause_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mepc_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mscratch_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mstatus_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mtval_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mtvec_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.stall_selector_q[*]=4' \
  --register-enable-fanout 'ff_core.csr_file.mcycle_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.mcycle_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.minstret_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.minstret_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.event_counters.low_q[*][*]=8' \
  --register-enable-fanout 'ff_core.csr_file.event_counters.high_q[*][*]=8' \
  --output riscv/build/riscv-hardware-test-125-ce8.texo.json
```

Generate a native bitstream only after the Texo run reports timing closure:

```sh
target/texo-git/bin/texo bitgen riscv/build/riscv-hardware-test-125-ce8.texo.json \
  --bit riscv/build/riscv-hardware-test-125-texo.bit
```

The command automatically downloads and verifies the corrected ECP5 v4 target
pack; no local architecture override is needed. The pinned Texo revision is
`f8c57e09bbc0b96825652dc3b76ac5e9e1e9661d` (Texo 0.1.6). The final
`riscv-hardware-test-125-ce8.texo.json` checkpoint closes the 125 MHz-targeted
flow (124.8 MHz PLL clock) at +11 ps setup WNS with zero setup TNS and +290 ps
hold WNS with zero hold TNS. It contains 20,588 Struo-mapped cells, 16,771
implemented cells, 18,497 nets, and 228,641 routed PIPs. STA checks 10,406 of
10,926 endpoints; the 520 unchecked endpoints belong to the separate
JTCK/JTAG shift domain, while the CPU, memory, and related-clock boundaries are
checked. This timing model adds no separate PLL jitter or clock uncertainty,
so +11 ps is model closure rather than an independent silicon guardband.

The generated `riscv-hardware-test-125-texo.bit` has SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`.
On the EVN at 124.8 MHz this exact counter-enabled bitstream passes the bundled
suite 8/8, the pinned ACT4 RV64I catalog 51/51, and the pinned ACT4 RV64Zca
catalog 32/32. Every run also passes the host's six-counter consistency checks.
The dedicated `performance.elf` readback reports high halves
`[16, 8, 1, 2, 3, 4]` for mcycle, minstret, dependency, memory wait, redirect,
and front-end empty respectively, exercising every selector and both 32-bit
response banks in hardware.
