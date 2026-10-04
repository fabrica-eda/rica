# RISC-V implementation

The saved timing-closed U/PMP reference now uses **84 MHz CPU and 84 MHz memory**.
It passes 536 native runs, the simultaneous U kernel, 31 simultaneous memory
tests, and measured STA at **setup +26 ps / hold +1 ps**, retaining the measured
library, 194 ps guards and four reviewed timing exceptions. Route ownership and
515 JTCK setup/hold checks pass. On 2026-10-04 the bitstream passed all 268
validation ELFs on the FPGA, including the U kernel, whose mcycle/minstret
equal the simultaneous-clock model. The matching source, PLL, software,
checkpoint and hardware logs are archived under
`toolchain/measurements/2026-10-03-rv64-u-84`.
The clock increase from the preserved 78 MHz reference is 7.7%; same-work RTL
execution takes about 7% less time. This is a simulation comparison, not a board
measurement. Details are in [RV64_USER_PLATFORM.md](RV64_USER_PLATFORM.md).

The working U profile runs the CPU at **112 MHz with 56 MHz memory** (VCO 672 MHz;
CPU/memory outputs have zero phase). It precomputes PMP range validity, caches
return-stack tops on the existing update edge, shortens CSR write gating, replaces
bulk RAS recovery copies with eight sharing flags, caches PMP fetch and data
permissions per uniform 4 KiB page ([permission cache](RV64_PMP_PERMISSION_CACHE.md)),
adds with four 16-bit carry-select slices and selects data-fault MTVAL at trap entry
([112/56 MHz timing](RV64_112_TIMING.md)). With the Texo routability fix pinned in
`Cargo.toml`, a plain flow without seed placement or route hints closes it at
**setup +10 ps / hold +1 ps** with the measured models, 194 ps guards and four
reviewed exceptions. The bitstream passed all 269 validation ELFs on the FPGA on
2026-10-05; the same flow and hardware suite also passed at 104/52 and 108/54 MHz.
The 84 MHz result applies to its archived source and checkpoint; changing this
development profile's clock alone does not reproduce that implementation.
The earlier 78 MHz archive remains unchanged.

See [RV64_ARCHITECTURE_PLAN.md](RV64_ARCHITECTURE_PLAN.md) for the ECP5
architecture study. The current board is **scalar RV64 with full M and B
(Zba/Zbb/Zbs)**, integer compressed instructions (Zca), Zicsr and Zifencei.
`HardwareCompactAccessFaultTestTop` combines a compact 64-entry BTB, 2-bit
prediction, an eight-entry RAS and a 4 KiB synchronous instruction cache.
It enables `FAST_LSU=6`, early bit manipulation, taken fetch and division,
DSP multiplication, machine interrupts and strict physical memory checks.

The qualified configuration preserves **64-bit PC, control targets, MEPC and
MTVAL**, rejects upper-address aliases and checks every byte before memory
side effects. Out-of-range loads/stores and demanded instruction fetches trap;
wrong-path speculative fetches do not produce architectural traps. Ordinary
load/store latency is unchanged. Carry-select prediction comparisons run in
parallel before choosing a one-bit match, without adding a pipeline stage.

The platform implements precise M-mode interrupts, `mie/mip`, MSIP/MTIME/MTIMECMP,
ECALL/MRET and legal NOP `WFI`. Both Rust `no_std` kernels pass three hardware
runs: two timer-preempted tasks preserve all integer context, and the fault
variant recovers from eight access faults before completing those tasks.
External interrupt levels have CPU tests; the board connects software and timer
interrupts. **A, U/S modes, PMP and Sv39 remain future work.**
See the [platform contract](RV64_MACHINE_PLATFORM.md) and [Rust kernel](kernel/README.md).

CPU **124.8 MHz / memory 62.4 MHz** is retained. With **200 ps reserved for both
setup and hold**, STA gives **+2 ps setup / +90 ps hold** (nominal +202/+290 ps).
All **19,032 setup and 19,032 hold checks** pass. The independent JTCK graph
matches all 515 setup/hold endpoints at 6 MHz; the four reviewed exceptions
and uncharacterized JTAGG external boundaries are unchanged. Normal Texo
reproduces the helper checkpoint byte-for-byte and generates the tested image.
The design uses **33,474 cells, 8,991 FFs, 16 DSPs and 150 EBRs**.

The exact bitstream passes **238 distinct physical ELFs, 78 repetitions and
16 runs through the updated default runner: 332 PASS**, using Windows D2XX/XVC.
The following are medians of three identical-ELF runs, including startup and
checks. MHz is the PLL configuration, not an independent frequency measurement;
MIPS is derived from the hardware counters and that configured clock.

| Diagnostic | Previous interrupt image → current cycles | Current IPC | IPC × 124.8 MHz (MIPS) |
| --- | ---: | ---: | ---: |
| alu_chain | 5,993 → 5,993 | 0.770232 | 96.124946 |
| load_store | 4,425 → 4,424 | 0.291591 | 36.390597 |
| m_divide | 21,445 → 21,445 | 0.120308 | 15.014409 |
| small | 15,389 → 15,388 | 0.151677 | 18.929244 |
| wide | 17,332 → 17,333 | 0.134656 | 16.805123 |
| words | 17,835 → 17,835 | 0.130866 | 16.332111 |

All 35 fixed workloads retain identical ELF bytes and retired counts, with
median cycle differences within ±1 and a geometric-mean speed ratio of
**1.000008×**. Ordinary throughput is preserved. Timer-driven kernels are
reported separately: tasks takes 86,653 cycles / 30,725 instructions and
access-faults takes a median 92,235 cycles / 31,949 instructions.

Fresh checks of this RTL pass **61 CPU tests**, Veryl check, **476 strict /
468 disabled serialized model runs**, 35 unchanged fixed-work timed diagnostics,
two IRQ diagnostics and both timed kernels. The unchanged memory module also
has 18 memory/cache tests and 2,056 continuous-fetch response checks across
native/WASM, strict/legacy and FAST_LSU=0/6 configurations from its last change.
[Source-to-test correspondence](../toolchain/measurements/2026-09-10-rv64-access-faults/carry-match-functional-validation.json).

Select the current model with `--hardware-memory-rtl --compact-predictor --icache-kib 4
--fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension
--early-bitmanip --early-taken-fetch --early-divide --machine-interrupts --strict-memory`.
Add `--timed-clocks` for coincident CPU/memory edges. The no-flag Core/model
retains the legacy comparison configuration. The default hardware image is
`build/riscv-hardware-compact-access-faults-124p8-20260910.bit`, SHA-256
`e100548bbb96f8739d2126c560c4789cd6679a62a6fdf198d33aba6b04996912`.

`verify-riscv --project PATH` reads RTL and the U-platform profile from an
explicit project snapshot. ELF paths remain explicit, so archived candidates
can be checked without replacing the working project's RTL or clock settings.

The custom OS target is **M/S/U modes and Sv39**. The [OS/ISA plan](RV64_OS_ISA_PLAN.md)
continues from the qualified interrupt/access-fault foundation to external I/O,
A, U-mode/PMP and supervisor virtual memory. The placement/routing fixes now run
through the normal `texo pnr` CLI and flow API in Texo `2639484`. They retain the
qualified image byte-for-byte, with another eight hardware checks passing.
See the [integration, CLI options and source bundle](../toolchain/measurements/2026-09-10-texo-closure-integration/README.md).

See [current qualification and tool changes](../toolchain/measurements/2026-09-10-rv64-access-faults/README.md),
[current hardware IPC/clock](../toolchain/measurements/2026-09-10-rv64-access-faults/PERFORMANCE.md),
[previous interrupt measurements](../toolchain/measurements/2026-09-10-rv64-machine-interrupts/PERFORMANCE.md),
[integer ISA history](RV64_INTEGER_ISA_PLAN.md), [DSP evidence](RV64_DSP_PLAN.md),
and [earlier JTAG repair](RV64_JTAG_FIX.md). New placements require checked
bitgen and physical tests of the exact resulting hash. The
[compact predictor study](RV64_COMPACT_PREDICTOR.md),
[static 4 KiB timing record](RV64_124P8_TIMING.md), and
[earlier L1I comparison](RV64_L1I_IMPLEMENTATION.md) remain historical baselines.

The predictor configurations are available without editing RTL:

| Predictor | Core `BRANCH_PREDICTOR_CONFIG` | Hardware `PREDICTOR_MODE` | Runner flag | Board synthesis top |
| --- | --- | --- | --- | --- |
| TAGE + BTB + RAS (existing default) | 0, or 1 with reset scrubbing | 0 | none | `HardwareTestTop` |
| 2048-entry bimodal + BTB + RAS | 2, or 3 with reset scrubbing | 1 | `--bimodal` | `HardwareBimodalTestTop` |
| Always not taken, no BTB/RAS | 4 (5 also accepted; no RAM to scrub) | 2 | `--static-predictor` | `HardwareStaticTestTop` |
| **Compact 64-entry BTB + 2-bit direction + 8-entry RAS** | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 5 --m-extension` (hardware-memory RTL) | `HardwareCompactM4KTestTop` |
| **Compact predictor + DSP M multiplier** | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 5 --dsp-multiplier` (hardware-memory RTL) | `HardwareCompactDspM4KTestTop` |
| **Compact predictor + DSP M + Zba** | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 5 --dsp-multiplier --zba-extension` (hardware-memory RTL) | `HardwareCompactZbaDspM4KTestTop` |
| **Compact predictor + DSP M + Zba + Zbb** | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 5 --dsp-multiplier --zba-extension --zbb-extension` (hardware-memory RTL) | `HardwareCompactZbbDspM4KTestTop` |
| Compact predictor + DSP M + B | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 5 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension` (hardware-memory RTL) | `HardwareCompactZbsDspM4KTestTop` |
| Compact predictor + DSP M + B + early forwarding/target preview | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension --early-bitmanip --early-taken-fetch` (hardware-memory RTL) | `HardwareCompactTakenFetchTestTop` |
| Compact predictor + DSP M + B + early forwarding/target preview/division | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension --early-bitmanip --early-taken-fetch --early-divide` (hardware-memory RTL) | `HardwareCompactEarlyDivideTestTop` |
| **Current: compact predictor + DSP M + B + early forwarding/target preview/division + machine interrupts** | 8 (9 on board) | 4 | `--compact-predictor --icache-kib 4 --fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension --early-bitmanip --early-taken-fetch --early-divide --machine-interrupts` (hardware-memory RTL) | `HardwareCompactInterruptTestTop` |
| Compact 32-entry BTB + RAS | 24 (25 on board) | 12 | `--compact-predictor-32` | `HardwareCompact32Cached4KTestTop` |
| Compact 64-entry BTB, no RAS | 40 (41 on board) | 20 | `--compact-predictor-no-ras` | `HardwareCompactNoRas4KTestTop` |
| Compact 64-entry BTB + RAS, no direction prediction | 72 (73 on board) | 36 | `--compact-predictor-no-direction` | generic `HardwareTestTop` parameter |

Hardware wrappers enable reset scrubbing; the compact predictor always scrubs
its table. The no-flag Core/model default remains the legacy TAGE comparison.
The qualified hardware-runner image uses the compact 64-entry configuration;
comparison variants do not inherit its physical qualification.

The `HardwareCached*TestTop` capacity-comparison wrappers use static prediction
and the same memory, PLL and debug interface. `ICACHE_INDEX_BITS=7/8/9` selects 2/4/8 KiB in HardwareTestTop and
HardwareCoreTestTop; zero selects the legacy two-line instruction buffer.

| Instruction cache | Board synthesis top | Hardware-model flags |
| --- | --- | --- |
| 2 KiB | `HardwareCached2KTestTop` | `--static-predictor --icache-kib 2` |
| **4 KiB (timing closed in Texo)** | `HardwareCached4KTestTop` | `--static-predictor --icache-kib 4` |
| 8 KiB | `HardwareCached8KTestTop` | `--static-predictor --icache-kib 8` |

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --static-predictor --cache-tests --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin verify-riscv -- \
  --static-predictor --icache-kib 4 --hardware-memory-rtl riscv/build/rv64-cache/*.elf
```

`--icache-kib` requires `--hardware-memory-rtl`; the ordinary Core ELF model
has immediate instruction memory and does not measure cache performance.
The cache suite includes self-modifying code and 3/6 KiB capacity probes.

Compare the self-checking integer microbenchmarks with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --benchmarks --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --bimodal --benchmarks --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --static-predictor --benchmarks --max-cycles 100000
```

The predictor flags also select that configuration for the bundled ELF suite,
ACT4, `verify-riscv --rtl-only`, and `verify-riscv --hardware-memory-rtl`.
Use the board synthesis top in the table with `nextpnr-riscv` for a full-board map.

This directory contains only the CPU being developed and its reusable
building blocks:

- `src/PipelineInterfaces.veryl`: typed decode and execute stage contracts
- `src/CompressedDecoder.veryl`: RV64C/Zca integer decompressor to canonical
  32-bit RV64I instructions
- `src/Decoder.veryl`: canonical integer decoder with optional full RV64M
- `src/MulDivUnit.veryl`: iterative radix-4 multiplication and restoring division
- `src/ExecuteUnit.veryl`: two-slice carry-select integer ALU
- `src/Core.veryl`: scalar one-wide pipeline state, hazard control, forwarding,
  traps, and commit
- `src/BranchPredictor.veryl`: prediction request/response tokens, speculative
  history/RAS state, retirement training, and redirect recovery
- `src/BranchTargetBuffer.veryl`: synchronous two-way target/type cache
- `src/TageDirectionPredictor.veryl`: synchronous bimodal plus four-table TAGE
- `src/TageTaggedTableMemory.veryl`: canonical dual-port tagged-table storage
- `src/StaticBranchPredictor.veryl`: one-entry fallthrough response with backpressure and flush
- `src/CompactBranchPredictor.veryl`: two-stage synchronous BTB, direction counters,
  eight prediction tokens and speculative/committed RAS
- `src/HardwareCompactTestTop.veryl`: compact-predictor board configurations
- `src/CsrFile.veryl`: machine-mode CSR and trap state
- `src/PerformanceCounter64.veryl`: exact split-carry 64-bit event counter
- `src/PerformanceEventCounters4.veryl`: four 64-bit pipeline-event counters
  sharing one selected-word incrementer
- `src/RegisterFile.veryl`: synchronous two-read/one-write register-file
  candidate
- `src/InstructionBuffer.veryl`: two-line CPU-side instruction buffer and
  halfword-addressable 32-bit instruction window
- `src/InstructionCache.veryl`: two-bank synchronous 2/4/8 KiB L1I, tagged
  refill, successor prefetch, parcel-length predecode, and FENCE.I invalidation
- `src/HardwareCachedTestTop.veryl`: selectable full-board static-predictor
  configurations for instruction-cache capacity comparisons
- `src/CompressedDecoderTimingTop.veryl`: registered out-of-context timing
  fixture for the decompressor alone
- `src/HardwareMemory.veryl`: 256 KiB unified board BRAM running at half the
  CPU frequency
- `src/Top.veryl`: 124.8 MHz ECP5 EVN PLL/LED bring-up wrapper
- `verify/`: architectural, synthesis, and logical-mapping regressions

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
- a three-slot head/middle/tail fetch FIFO, which absorbs the predictor's
  registered hit path while preserving one accepted sequential instruction per
  clock
- CPI 1 through consecutive dependent ALU instructions using registered EX
  bypasses
- one localized wait for load-use, shift-use, and compare-use dependencies;
  independent shifts and comparisons still enter every clock
- one empty EX predecessor for each CSR instruction, keeping architectural
  CSR ordering and counter carry logic out of the ordinary ALU path
- response-token-driven data-memory waits; no fixed load/store cycle count
- optional M operations hold younger pipeline stages until completion; older
  redirects cancel the operation, and dependent consumers receive only its result
- a 124.8 MHz CPU domain and 62.4 MHz BRAM domain with registered bundled-data
  crossings
- pipelined 16-bit comparison chunks completed in EX2, keeping the comparison
  reduction and data-dependent flush fanout off the ALU critical path
- a registered instruction-memory address, keeping the next-PC addition and
  board ROM decode in separate timing stages without reducing fetch throughput
- 64-bit architectural registers and data with a 32-bit physical address
  space

### Branch prediction

The scalar frontend accepts one prediction request per clock. A 128-set,
two-way BTB supplies control type and target. Conditional direction comes from
a 2048-entry two-bit bimodal table plus four 512-entry, 16-bit tagged TAGE
tables with geometric history lengths 4/12/36/108. The 16-entry RAS follows
the RISC-V x1/x5 call, return, and coroutine hints, including compressed PC+2
links. All prediction memories read synchronously. Their response first enters
a compact metadata decision register; BTB hits are classified into a second
elastic output register, while the common BTB-miss/not-taken case is served
directly from the ordered decision register. A non-taken response can accept
its sequential replacement request on the same edge. A held taken response
instead blocks requests, then cancels the decision and raw fall-through
shadows on its acceptance edge; the target request can launch on the next
edge.

This FPGA-oriented TAGE variant has no `USE_ALT_ON_NA` counter: a weak tagged
provider always selects its prediction-time true alternate (the next-longest
tag hit, or bimodal). On an eligible misprediction, the two-bit LFSR probes
exactly one longer-history bank independently of that row's free state. A free
chosen row is allocated; a busy chosen row only decrements its own two-bit
usefulness and does not fall through to another bank. Every 2^18 committed
conditional branches, a background sweep clears alternating usefulness bits,
so two sweeps age surviving entries to zero while preserving lookup throughput.
Current S2 training stalls all four aging banks for one cycle. A registered
training write owns only its target bank; pending aging read/modify/write state
is held and then resumed, rather than committing a stale row after training.

Eight tokens retain prediction-time metadata until retirement. Retire training
is pipelined into the table write ports, with same-row forwarding preserving
back-to-back updates. Recovery restores the committed GHR/RAS frontier and
applies the resolved action. That frontier rule is valid only for this scalar,
in-order pipeline, where no unresolved older control transfer remains hidden
when a younger one recovers. A two-wide or out-of-order implementation must
attach a predictor checkpoint to each ROB branch; it cannot reuse this single
committed-frontier shortcut.

The architectural regression currently commits 58 instructions in 118 clocks
(2.034 CPI), including RV64 word operations, load-use and shift-use
dependencies, and taken BEQ, JAL, and JALR redirects. The independent RV64
ELF suite has the following ordinary-Core results:

| ELF | clocks | commits |
| --- | ---: | ---: |
| `alu` | 101 | 30 |
| `compare` | 84 | 16 |
| `compressed` | 438 | 143 |
| `control` | 97 | 27 |
| `memory` | 108 | 37 |
| `performance` | 235 | 108 |
| `shift` | 147 | 37 |
| `system` | 546 | 180 |

All eight also pass both coincident-edge serializations of the dual-clock
HardwareMemory model. `performance` is the dedicated counter/readback test.

The RV64 `mcycle` and `minstret` counters are writable at 0xB00/0xB02; the
standard 0xC00 `cycle` and 0xC02 `instret` addresses are read-only aliases.
Two custom machine CSRs expose implementation event counts without widening
the four-bit internal CSR index: 0x7C0 selects a counter and 0x7C1 reads or
writes its 64-bit value.

| 0x7C0 selector | board debug index | Counter |
| ---: | ---: | --- |
| — | 0 | `mcycle` |
| — | 1 | `minstret` |
| 0 | 2 | dependency bubble, including M-unit wait when enabled |
| 1 | 3 | data-memory wait |
| 2 | 4 | redirect generation/application |
| 3 | 5 | empty fetch/decode front end |
| 4 | 6 | retired conditional branch |
| 5 | 7 | conditional direction wrong |
| 6 | 8 | retired control transfer with a BTB miss |
| 7 | 9 | retired return/coroutine that used the BTB fallback because the RAS was empty |
| 8 | 10 | architectural next-PC prediction repair |

The first four are control events rather than a decomposition of CPI: a
front-end bubble can coexist with an older retirement. At most one of those
four classes advances per clock using the controller's priority order: memory,
redirect, dependency, then front end. Predictor events are independent and may
overlap one another and a pipeline class. Repair counts Core recovery only;
predictor slot discard for traps/MRET is not a repair. `PerformanceCounter64`
uses parallel 32-bit low/high increments for the architectural and independent
predictor counters. `PerformanceEventCounters4` stores the four pipeline words
but selects only the active word before a shared incrementer. Every event input
is registered for timing; a delayed freeze preserves the final pending event.

The EVN has a 12 MHz reference and an integer ECP5 PLL. The board configuration
uses `CLKI_DIV=1`, a 12 MHz phase-detector frequency, and a 624 MHz VCO.
`CLKOS` divides by five for the 124.8 MHz CPU clock and `CLKOS2` divides by ten
for the 62.4 MHz memory clock. Exact 125 MHz and the older 126 MHz configuration
require an input divider greater than one and put the phase detector below its
data-sheet range, so neither is used. The board disables unused commit payload
registers while retaining `commit_valid` for its heartbeat; the verification
build keeps the full architectural commit trace.

The earlier nextpnr-based timing flow checks ordinary per-domain paths and applies a
separate one-CPU-period setup guard to both divided-clock boundaries. Although
memory runs at half the CPU frequency, the board's zero-phase clock contract
can have an intervening CPU edge, so both directions must meet the 8.012 ns CPU
period derived from nextpnr's reported 124.812782 MHz constraint. The nearby
8.013 ns figure is the rounded reciprocal of the human-readable 124.8 MHz PLL
frequency. The guard validates the exact 5:10 PLL configuration and reported
frequencies, but it is not general generated-clock STA or an independent
hold/jitter/skew analysis. The instruction payload is captured first in the
CPU domain; the wider data selection cone is first registered in the memory
domain. The response toggle changes only with that registered data, so
load/store completion follows the actual response rather than an assumed
pipeline length.

The earlier full-TAGE physical implementation did not close this
target. Its 31,450-cell board map reaches 89.598 MHz with
official nextpnr 0.11.1 static seed 5. Its worst path runs from an automatically
retimed `fetch_line_q[20]` bit to a tagged TAGE table `WEA`, with 2.000 ns of
logic and 9.161 ns of routing. It also fails the one-CPU-period memory-to-CPU
guard at 9.205/8.012 ns. Texo 0.1.6 implements the same logical map at
-1.813 ns setup WNS and reports another retimed fetch-line capture endpoint.
A controlled no-retime build is worse at 75.763 MHz, and moving reset scrub
to the tagged tables' other port merely moves the bottleneck and reaches
83.836 MHz; both experiments were reverted. These are placement-sensitive
negative optimization results, not proof of a confirmed tool defect. No
bitstream is generated from these failed runs; hashes and full path breakdowns
are retained in
[`../toolchain/measurements/2026-09-02-rica-qor.md`](../toolchain/measurements/2026-09-02-rica-qor.md).

## Verification

Run source RTL simulation and Struo logical mapping:

```sh
cargo run --locked --offline -p verify-riscv
```

This checks Struo's proof-signed internal mapping transformations, but it is
not a complete mapped functional comparison. Passing `--post-map` additionally
runs the expensive ECP5-primitive simulation. The old deterministic
true-dual-port rejection and 512-way read-mux overflow have focused fixes; the
current Struo integration pin is recorded in the root `Cargo.toml`.
An interactive complete release-mode post-map native build was stopped after
more than five minutes without a bounded completion signal; it is not a
required default gate and is tracked conservatively by STR-007 in
[`TOOLCHAIN_ISSUES.md`](../TOOLCHAIN_ISSUES.md).

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

`--act4-suite M` selects the complete 13-test RV64M catalog, enables M in the
Core model, and uses the separate `act4/test_config_m.yaml` configuration.
The custom `--m-tests` suite checks every instruction and corner case;
`--memory-tests` and `--m-benchmarks` build the added memory and equal-work
arithmetic workloads:

```sh
cargo run --locked --offline --release -p verify-riscv --bin rv64-riscv-tests -- --m-tests --max-cycles 1000000
cargo run --locked --offline --release -p verify-riscv --bin rv64-riscv-tests -- --memory-tests --max-cycles 1000000
cargo run --locked --offline --release -p verify-riscv --bin rv64-riscv-tests -- --m-benchmarks --max-cycles 1000000
cargo run --locked --offline --release -p verify-riscv --bin verify-riscv -- \
  --hardware-memory-rtl --compact-predictor --icache-kib 4 --fast-lsu 5 \
  --m-extension --max-cycles 1000000 riscv/build/rv64-m/*.elf
```

The implemented compressed subset is integer Zca. Adding M does not add
floating-point, atomic or supervisor-mode instructions. `ENABLE_M=0` rejects
M opcodes and leaves `misa.M` clear; `ENABLE_M=1` makes it read-only one.

The optional `ENABLE_ZBA` implementation adds all eight RV64 Zba instructions.
The [integer ISA plan](RV64_INTEGER_ISA_PLAN.md) records its physical qualification
and the subsequent Zbb/Zbs qualification. The default hardware image enables all three B subsets. `--zba-tests` builds the custom
operand/alias tests, `--act4-suite Zba` selects the complete eight-test Sail-backed
catalog, and `--zba-extension` enables Zba for external ELF inputs in either model
runner. Use `act4/test_config_zba.yaml` to build the separate `rica-rv64im-zba`
catalog. Its Sail configuration enables Zba independently of the earlier catalogs.

`tools/build_rv64_zba_c.py` builds identical C workloads with and without Zba,
retaining compiler commands, input checksums, disassembly and ELF hashes. Set
`RISCV_CLANG` to the Clang 20 executable. Performance comparisons between different
ISA settings use elapsed cycles for the same work, alongside IPC and clock.

`ENABLE_ZBB` adds all 24 RV64 Zbb instructions. `--zbb-tests` builds the custom
operand and alias tests; `--act4-suite Zbb` selects the complete 24-test catalog
built with `act4/test_config_zbb.yaml`. Add `--zbb-extension` for external ELF
inputs in either model runner. Zbb results reach the existing WB register;
dependent consumers wait until that value can be forwarded. The standalone
oracle checks RTL and mapped results, reserved encodings, and instruction bytes
against LLVM 20's assembler. `tools/build_rv64_zbb_c.py` retains paired C builds
for population/zero counts, rotations, and byte operations.

`ENABLE_ZBS` adds the eight RV64 single-bit instructions. `--zbs-tests` builds
the eight custom instruction tests plus the read-only `misa.B` check, enabling
Zba/Zbb/Zbs together for that catalog. `--act4-suite Zbs` selects all eight
Sail-backed tests built with `act4/test_config_zbs.yaml`; `--zbs-extension` enables
Zbs for external ELFs. `misa.B` is set only when all three B subsets are enabled.
`tools/build_rv64_zbs_c.py` retains paired C builds for bit updates, extraction,
and immediate forms. Zbs has passed RTL/mapped and complete functional-model
regressions, 124.8 MHz setup/hold timing and byte-exact bitstream reproduction.
That Zbs-stage image passed 310 physical executions; the current image is qualified above.

For cycle comparisons, `verify-riscv --hardware-memory-rtl --timed-clocks` uses
Celox's timed scheduler to evaluate coincident CPU/memory edges together. Explicit
ELF paths are required. This diagnostic compiles a timed model per ELF; the normal
cached native catalog runner retains both serialized clock orders as functional
stress tests. The timed diagnostic prints architectural `mcycle`/`minstret`, which
align with the hardware's CM retirement boundary; the terminal store has not yet
appeared at the later external commit port when the runner stops.

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
riscv/tools/texo pnr riscv --top CompressedDecoderTimingTop \
  --package CABGA381 --speed 8 --lpf riscv/core-126.lpf \
  --allow-unconstrained-io --synthesis-goal-mhz 126 \
  --output riscv/build/compressed-decoder-126.txcp
```

The last recorded fixture result was +0.607 ns setup WNS and +0.323 ns hold
WNS; it is not a fresh run of the current Texo dependency.

`HardwareTestTop` adds 256 KiB of direct-JTAG-loadable unified true-dual-port BRAM,
CPU run/reset control, and a `tohost` result mailbox. The first physical port
serves rising-edge 64-bit instruction-line fetches or debug writes while reset;
the second serves rising-edge LSU reads and writes. Both ports run at 62.4 MHz;
the CPU-side instruction frontend exposes a halfword-aligned window spanning
up to two adjacent lines. The hardware runner defaults to the physically
verified `riscv/build/riscv-hardware-compact-access-faults-124p8-20260910.bit`,
with eight-byte masked writes. Its build command, hash and checked bitgen flow
are in the [current image report](../toolchain/measurements/2026-09-10-rv64-access-faults/README.md). Program it and run the eight
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
FT2232H, and defaults to loopback-only listening. On the verified Windows 11
/ Ubuntu-24.04 WSL setup, Windows loopback is reachable from WSL. Start the
bridge using Windows Python (Linux Python cannot load D2XX):

```powershell
python.exe \\wsl.localhost\DISTRO\path\to\rica\riscv\tools\windows_d2xx_xvc.py `
  --listen 127.0.0.1 --port 3721 --device-index 0
```

Use `--xvc 127.0.0.1:3721` from WSL. Check the actual Windows/WSL network
configuration if loopback is unavailable; in mirrored networking, the Linux
default gateway can be the LAN router rather than the Windows host. For NAT
networking, bind to the Windows-side WSL virtual interface and use that
address from WSL. Keep the listener on loopback or the WSL virtual interface.

The hardware runner defaults to **eight bytes per masked WRITE64 frame** on
`riscv-hardware-compact-access-faults-124p8-20260910.bit`.
`--write-bytes 1` retains compatibility with older qualified images; `2` and `4`
remain selectable. The falling-edge receiver passed the former 48 corruption
probes and all 100 ELFs with eight-byte writes. This reduces loading frames;
CPU/memory clocks remain 124.8/62.4 MHz. See the [current image record](../toolchain/measurements/2026-09-10-rv64-access-faults/README.md)
and [historical failure reproducer](../toolchain/mre/README.md).

`--act4` selects all 51 pinned ACT4 RV64I ELFs;
`--act4 --act4-suite Zca` selects previously generated Zca ELFs, and
`--act4 --act4-suite M` selects the 13 M ELFs for an M-enabled image. ER1 carries
fixed 128-bit commands (8-byte masked memory writes, control, and response
selection), while JTDO1 returns one stable selected response bit throughout
each scan. ECP5's
falling-edge receiver registers CE/SHIFT to align with delayed JTDI and
consumes exactly 128 physical bits. UPDATE is sampled as data in the same
JTCK domain; the host needs no dummy bit.
Only a terminal `tohost` store acknowledged by the synchronous memory response
token is captured. On that edge the store still retires; the following frozen
snapshot therefore includes its `minstret` increment and every preceding
memory-wait event. Response banks 5/6 expose the low/high halves of all eleven
counters in the table above. SELECT address bits [11:8] choose the counter and
retain the existing response-bank/bit selector in the low byte. The XVC runner
reads and validates all eleven 64-bit values, then prints a `METRIC` line
including CPI for every ELF. The 128-bit frame, response banks, and protocol
version 0x05 remain unchanged.

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
The selected compact build is reproduced from the command recorded in
[DSP qualification evidence](../toolchain/measurements/2026-09-07-rv64-dsp/cpu-07-start.json).
The following legacy TAGE command records the earlier timing comparison;
it is not the selected compact implementation. It uses the same PLL binding:

```sh
cargo install --git https://github.com/fabrica-eda/texo.git \
  --rev 7b372da4997c924b48cc09327ef006b7af753e65 \
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
  --register-enable-fanout 'ff_core.csr_file.predictor_conditional_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_conditional_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_direction_wrong_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_direction_wrong_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_btb_miss_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_btb_miss_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_ras_fallback_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_ras_fallback_counter.high_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_repair_counter.low_q[*]=8' \
  --register-enable-fanout 'ff_core.csr_file.predictor_repair_counter.high_q[*]=8' \
  --output riscv/build/riscv-hardware-test-125-plruclean.texo.json
```

For current board images, include `--clock-constraints jtck-clock-constraints.json`
and `--timing-exceptions timing-coverage-exceptions.json` in the native PnR
flow, using paths relative to its working directory. Run
`tools/check_integrated_jtck_sta.py` on the resulting checkpoint before normal
bitgen. The complete pinned commands and boundary scope are in the
[JTCK report](RV64_JTCK_LSU_PLAN.md). The raw legacy command below is retained
only to explain the older flow:

```sh
target/texo-git/bin/texo bitgen riscv/build/riscv-hardware-test-125-plruclean.texo.json \
  --bit riscv/build/riscv-hardware-test-125-predictor.bit
```

The command automatically downloads and verifies the corrected ECP5 v4 target
pack; no local architecture override is needed. The pinned Texo revision is
`7b372da4997c924b48cc09327ef006b7af753e65`: Texo 0.1.6 plus the final head of
[PR #59](https://github.com/fabrica-eda/texo/pull/59), with every internal
Struo dependency locked to Rica's `eff404dff0c4806064e2a4e0f59564b1079c001d`
integration revision. A fresh run at that exact pin implemented 30,095 cells,
31,588 nets, and 367,697 PIPs in 271.19 seconds, but failed setup with
-1.813 ns WNS and -0.870057 us TNS across 1,687 endpoints. Mapping equivalence
passed; bitgen and hardware testing were correctly skipped. The physical
checkpoint below remains attributed to the older `f8c57e09` revision.

### Archived pre-predictor hardware checkpoint

The WNS, utilization, hash, and board pass totals below belong to the former
six-counter design. That artifact contains neither the current BTB/TAGE/RAS
frontend nor the eleven-counter ABI and is not a qualification result for the
current RTL. The current compact predictor has separate qualification in the
[implementation report](RV64_COMPACT_PREDICTOR.md); the historical numbers
below do not apply to that image.

The recorded result for the historical checkpoint formerly written as
`riscv-hardware-test-125-ce8.texo.json` closed the 125 MHz-targeted flow
(124.8 MHz PLL clock) at +11 ps setup WNS with zero setup TNS and +290 ps hold
WNS with zero hold TNS. It contained 20,588 Struo-mapped cells, 16,771
implemented cells, 18,497 nets, and 228,641 routed PIPs. STA checked 10,406 of
10,926 endpoints; the 520 unchecked endpoints belonged to the separate
JTCK/JTAG shift domain, while the CPU, memory, and related-clock boundaries are
checked. The ignored JSON was not retained immutably and has since been reused
by an experimental predictor build, so these metrics do not describe the file
currently on disk. This timing model added no separate PLL jitter or clock uncertainty,
so +11 ps is model closure rather than an independent silicon guardband.

The generated `riscv-hardware-test-125-texo.bit` has SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`.
On the EVN at 124.8 MHz this exact historical bitstream passed the bundled
suite 8/8, the pinned ACT4 RV64I catalog 51/51, and the pinned ACT4 RV64Zca
catalog 32/32. At that pre-predictor-counter checkpoint, every run also passed
the host's six-counter consistency checks. Its dedicated `performance.elf`
readback reported high halves
`[16, 8, 1, 2, 3, 4]` for mcycle, minstret, dependency, memory wait, redirect,
and front-end empty respectively. The current regression expects
`[16, 8, 1, 2, 3, 4, 5, 6, 7, 8, 9]` after rebuilding the bitstream, but that
expected signature is not a claim of fresh hardware validation.
