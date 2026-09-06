# RISC-V executable tests

`rv64i-subset/` contains self-checking RV64 ELF tests assembled independently
of the Rust trace regression. Each ELF starts at address zero and reports its
result by storing to the global `tohost` symbol:

- `1`: pass
- `(test_number << 1) | 1`: fail

Build and run all eight bundled tests with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests
```

The runner can also execute externally generated self-checking ELFs:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- \
  --max-cycles 1000000 path/to/test.elf [path/to/another.elf ...]
```

An input ELF must be little-endian RV64, contain a `tohost` symbol, fit in the
Core's 32-bit physical address space, and use its 64-bit data interface with
eight byte write enables. For a nonzero ELF entry, the loader places a short
`LUI`/`JALR` reset trampoline at address zero (three instructions for ACT4's
standard `0x80000000` entry); ELFs that already occupy that trampoline are
rejected. On failure or timeout, the runner prints the most recent commit
trace.

The bundled build locates LLVM 20's `llvm-mc` and Rust's bundled `rust-lld`.
Set `LLVM_MC` or `RUST_LLD` to override either executable.

## Predictor regressions

The compact predictor candidate is described in
[RV64_COMPACT_PREDICTOR.md](../RV64_COMPACT_PREDICTOR.md). Use
`--compact-predictor`, `--compact-predictor-32`,
`--compact-predictor-no-ras`, or `--compact-predictor-no-direction` with the
ELF and board-memory model runners. `--predictor-tests` builds the additional
[BTB collision and nested-call diagnostics](rv64-predictor/README.md).
The compact RTL suite covers the elastic lookup/decode pipeline, full tokens,
same-row forwarding, wrong-path lookup cancellation, retire-time recovery,
and compressed links plus RAS overflow/underflow. Core integration exercises
memory waits, stale BTB load/store/CSR predictions, exceptions, and FENCE.I.

The RTL predictor tests cover the current one-request-per-clock synchronous
pipeline: a two-way, 128-set BTB; a 2048x2 bimodal table plus four 512x16 TAGE
tables using 4/12/36/108-bit histories; and a 16-entry x1/x5-aware RAS. They
exercise held-response replacement rules, retire-time pipelined training and
same-row forwarding, BTB type/target repair, compressed PC+2 links, RAS
fallback, and recovery/discard token handling. Directed cases also cover
same-set cold-miss victim reservation, deferred invalidation guarded by the
physical `(set, way)` identity, same-edge replacement versus response capture,
reset scrubbing of every inferred predictor BRAM, and refreshing a masked
invalid fetch payload without issuing a predictor request or advancing the
cursor.

The focused predictor regression currently passes 60/60 cases: 23 model
cases, 36 source-RTL cases, and one complete reset-scrub case. The Core
predictor-recovery integration suite now has 29 cases, including M ownership,
dependencies and cancellation under all four main predictor configurations.

These recovery tests deliberately model the present scalar in-order core. Its
predictor may recover from the committed GHR/RAS frontier because older control
transfers resolve in order. Dual issue or out-of-order execution requires a
per-branch ROB predictor checkpoint; passing this scalar suite is not evidence
that the shortcut is safe for a two-wide core.

## EVN hardware runner

The same ELF contract is supported by the direct-JTAG hardware harness:

```sh
cargo run --locked --offline --release -p verify-riscv --bin hardware-riscv-tests
```

The default command programs the qualified compact 64-entry BTB/RAS + 4 KiB
image with M enabled and shortened loads,
`build/riscv-hardware-compact-m-load-4k-124p8-20260907.bit`, uses eight-byte
writes and runs all eight bundled tests. Its [hardware record](../RV64_LOAD_M_PLAN.md)
covers 133 distinct ELFs and 165 executions with all eleven counters at CPU
124.8 MHz / memory 62.4 MHz. Nominal setup is +206 ps and hold +290 ps;
setup remains +6 ps after the additional 200 ps reserve.
The updated default runner passed eight more basic ELFs (173 executions total).
Select other images explicitly after their timing and functional checks pass.
`--no-program` reuses the programmed image,
`--bitstream path` selects another image, and `--run-tck count` changes the CPU
execution window. `--expect-tohost value` selects an exact diagnostic mailbox
value. Positional ELF paths select individual or external tests.
Each ELF must fit in the 4 KiB reset-trampoline window
`0x00000000..0x00000fff` and the 252 KiB test window
`0x80000000..0x8003efff`.

The runner writes generated SVF files under `build/hardware-rv64`. A successful
result covers the entire board path: JTAGG ER1, 128-bit command transport,
BRAM loading, PLL/reset, RV64 execution, and `tohost` readback. The result is
accepted only after HardwareMemory acknowledges the terminal store. That ACK
also gates the frozen XVC snapshot of all eleven counters: `mcycle`, `minstret`,
the dependency/memory-wait/redirect/front-end pipeline classes, and conditional
resolved/direction wrong/BTB miss/RAS fallback/prediction repair events. SELECT
address bits [11:8] choose the counter. The runner validates and prints them as
`METRIC` records without changing the 128-bit frame, response banks 5/6, or
protocol version 0x05.

## Historical hardware checkpoint

The recorded bitstream below predates both the BTB/TAGE/RAS frontend and the
eleven-counter readback ABI. The current host asks for eleven selectors, so the
old image is not expected to pass the current hardware runner. Its hash and
pass totals are retained only as provenance; fresh PNR, bitgen, and board runs
must replace them before claiming current hardware coverage.

The recorded pre-predictor-counter bitstream generated from a checkpoint
formerly written as `build/riscv-hardware-test-125-ce8.texo.json` has SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`. On the
EVN at 124.8 MHz it physically passed the bundled suite 8/8, the pinned ACT4
RV64I catalog 51/51, and the pinned ACT4 RV64Zca catalog 32/32. Every ELF also
passed the former six-counter readback checks. That retained result predates
the predictor counters. The ignored JSON was later reused by an experimental
predictor P&R run and no longer matches those historical metrics. The current
`performance.elf` regression exercises all
eleven selectors and both 32-bit banks with distinct non-zero high halves; a
new bitstream still requires physical validation before replacing the recorded
hash and pass counts above.

## Architectural test integration

The current [RISC-V Architectural Certification Test framework][act4] is
tracked at a fixed revision in `third_party/riscv-arch-test`. Initialize it
after cloning Rica with:

```sh
git submodule update --init
```

ACT4 generates self-checking ELFs with expected results derived from the Sail
model. Rica's configurations enable the base integer, integer Zca compressed, and
M multiply/divide suites. M has a separate `rica-rv64im` work directory. From the pinned submodule, generate each catalog with:

```sh
cd third_party/riscv-arch-test
uv run --offline act ../../riscv/act4/test_config.yaml \
  --workdir work --test-dir tests --extensions I
uv run --offline act ../../riscv/act4/test_config.yaml \
  --workdir work --test-dir tests --extensions Zca
```

Run the complete RV64I catalog through Rica's runner with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- \
  --act4 --act4-elf-dir path/to/act4/elfs
```

Select the 32-test Zca catalog with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- \
  --act4 --act4-suite Zca --act4-elf-dir path/to/act4/elfs
```

Build and run the M catalog with its separate Sail/UDB configuration:

```sh
# In third_party/riscv-arch-test; install its pinned dependencies first.
uv run --offline act ../../riscv/act4/test_config_m.yaml \
  --workdir work --test-dir tests --extensions M --fast
# In the Rica root; this enables M automatically in the Core model.
cargo run --locked --offline --release -p verify-riscv --bin rv64-riscv-tests -- \
  --act4-suite M --max-cycles 1000000
```

The M configuration declares M 2.0 and its required Zmmul 1.0 subset, with
`misa.M` fixed on. All 13 generated M ELFs pass both clock orders in the
board-memory model and the qualified physical M image. Build environment
versions and source/ELF hashes are in the [measurement record](../../toolchain/measurements/2026-09-06-rv64-load-margin-m/README.md).

The pinned revision is `f7ba06c3d1a01428b172fe851ab56931a0624fee`.
Its 32 generated Zca ELFs have SHA-256 set digest
`8938083a2a0e5cd9ae7b74c6b1f6be9c8afec0da593b07478c8385a421e2a12e`
and pass both the ordinary Core simulator and the dual-clock synchronous
HardwareMemory model with no ignored tests. The digest is the SHA-256 of the
sorted `sha256sum` output for all 32 ELF files.

The runner discovers the selected suite's `I-*-00.S`, `M-*-00.S` or `Zca-*-00.S` files
directly from the submodule. The reviewed lists are
`act4-rv64-enabled.txt`, `act4-rv64-m-enabled.txt` and `act4-rv64-zca-enabled.txt`. All 51 RV64I tests and
all 32 Zca tests in the pinned catalogs pass both the ordinary Core simulator
and the synchronous HardwareMemory RTL model. The latter runs every ELF with
coincident memory/CPU edges serialized in both `mem->cpu` and `cpu->mem` order,
so neither same-edge request nor response behavior can create a false pass.
Tests added by a later submodule update are printed as `IGNORE` until they are
reviewed and enabled, instead of silently disappearing from the suite. No
official test source is copied, renamed, or patched in Rica.

The local `rv64i-subset/` programs remain fast bring-up smoke tests, not a
replacement for or fork of ACT4. The assembler enables RV64IC, while the
original RV64I sources retain `.option norvc` so compressed coverage cannot
silently alter their instruction streams. They currently cover:

- `LUI`, `AUIPC`, `JAL`, `JALR`
- `BEQ`, `BNE`, `BLT`, `BGE`, `BLTU`, `BGEU`
- `LB`, `LH`, `LW`, `LD`, `LBU`, `LHU`, `LWU`, and `SB`, `SH`, `SW`, `SD`
- `ADDI`, `XORI`, `ORI`, `ANDI`
- `ADD`, `SUB`, `XOR`, `OR`, `AND`, `SLT`, `SLTU`, `SLTI`, `SLTIU`
- `ADDIW`, `ADDW`, `SUBW`
- `SLL`, `SRL`, `SRA` and their immediate and W forms
- `FENCE` for the current in-order, uncached memory interface
- `FENCE.I`, including instruction-cache invalidation in the board-memory model
- `ECALL`, `EBREAK`, `MRET`, illegal-instruction traps, and the implemented
  machine-mode CSRs through the Zicsr register and immediate forms
- RV64 `mcycle`/`minstret`, their read-only `cycle`/`instret` aliases, and the
  custom 0x7C0 selector/0x7C1 data window for nine implementation event
  counters: selectors 0..3 are dependency, memory wait, redirect, and
  front-end bubble; selectors 4..8 are conditional resolved, direction wrong,
  BTB miss, RAS fallback, and prediction repair
- dependent ALU forwarding, a load-use dependency, and a taken redirect whose
  target load immediately feeds a dependent branch
- explicit RV64C integer arithmetic, shifts, loads/stores, branches, jumps,
  `C.EBREAK`, stack-pointer forms, and mixed 16/32-bit fetches
- a 32-bit instruction beginning at `PC[1] = 1`, including one that crosses an
  eight-byte instruction-memory line boundary

Additional bundled suites are selected with `--memory-tests`, `--m-tests`
and `--m-benchmarks`. See [memory workloads](rv64-memory/README.md),
[M correctness cases](rv64-m/README.md), and
[equal-work M performance comparisons](rv64-m-benchmarks/README.md).
M includes `MUL`, `MULH`, `MULHSU`, `MULHU`, `MULW`, `DIV`, `DIVU`, `REM`,
`REMU`, `DIVW`, `DIVUW`, `REMW`, and `REMUW`. Source-RTL tests additionally
check illegal reserved word encodings and all M encodings with M disabled.

Passing the pinned catalog is a regression result rather than an external
certification claim. A bundled test reports through `tohost` and then enters a
terminal loop; `ECALL` is handled architecturally as a machine-mode exception
rather than as a simulation halt.

[act4]: https://github.com/riscv/riscv-arch-test

## JTAG receiver regression

The falling-edge receiver is covered at RTL and ECP5-primitive level by
`jtag_transport_edges`: every frame bit, raw control/data boundary timing,
UPDATE retention, unrelated DR scans and CPU-only relock. The mapped test also
checks all 261 falling-edge FFs. Run it with:

```sh
cargo test --locked --offline -p verify-riscv --test jtag_transport_edges
```

[Physical results and the checked bitgen flow](../RV64_JTAG_FIX.md) include
100 ELFs, 134 executions and the 48 former WRITE64 corruption probes.
