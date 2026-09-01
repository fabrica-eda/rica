# RISC-V executable tests

`rv64i-subset/` contains self-checking RV64 ELF tests assembled independently
of the Rust trace regression. Each ELF starts at address zero and reports its
result by storing to the global `tohost` symbol:

- `1`: pass
- `(test_number << 1) | 1`: fail

Build and run all seven bundled tests with:

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

## EVN hardware runner

The same ELF contract is supported by the direct-JTAG hardware harness:

```sh
cargo run --locked --offline --release -p verify-riscv --bin hardware-riscv-tests
```

The default command programs `build/riscv-hardware-test-125-texo.bit` and then
runs all seven bundled tests. `--no-program` reuses the programmed image,
`--bitstream path` selects another image, and `--run-tck count` changes the CPU
execution window. `--expect-tohost value` selects an exact diagnostic mailbox
value. Positional ELF paths select individual or external tests.
Each ELF must fit in the 4 KiB reset-trampoline window
`0x00000000..0x00000fff` and the 252 KiB test window
`0x80000000..0x8003efff`.

The runner writes generated SVF files under `build/hardware-rv64`. A successful
result covers the entire board path: JTAGG ER1, 128-bit command transport,
BRAM loading, PLL/reset, RV64 execution, and `tohost` readback. The
timing-closed 256 KiB wrapper has physically passed all seven bundled
RV64I/RV64C smoke tests and all 51 enabled ACT4 RV64I tests on the EVN at
124.8 MHz. This includes the redirect-to-load/load-use regression in
`control.elf`. Official ACT4 Zca ELFs have not yet been generated locally, so
the bundled `compressed.elf` is the current physical compressed-ISA coverage.

## Architectural test integration

The current [RISC-V Architectural Certification Test framework][act4] is
tracked at a fixed revision in `third_party/riscv-arch-test`. Initialize it
after cloning Rica with:

```sh
git submodule update --init
```

ACT4 generates self-checking ELFs with expected results derived from the Sail
model. Rica's configuration enables the base integer and Zca compressed
suites. Generate both catalogs with:

```sh
make -C third_party/riscv-arch-test \
  CONFIG_FILES=../../riscv/act4/test_config.yaml EXTENSIONS=I,Zca
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

The Zca configuration, reviewed enable list, and runner selection are in
place, but official ACT4 Zca ELF generation and execution have not yet been
completed locally. The required ACT4 generator toolchain is unavailable in
the current environment, so this is plumbing rather than a Zca ACT4 pass
claim.

The runner discovers the selected suite's `I-*-00.S` or `Zca-*-00.S` files
directly from the submodule. The reviewed lists are
`act4-rv64-enabled.txt` and `act4-rv64-zca-enabled.txt`. All 51 tests in the
pinned RV64I catalog pass both the ordinary Core simulator and the synchronous
HardwareMemory RTL model. Tests added by a later submodule update are printed
as `IGNORE` until they are reviewed and enabled, instead of silently
disappearing from the suite. No official test source is copied, renamed, or
patched in Rica.

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
- `FENCE.I` for the current uncached instruction interface
- `ECALL`, `EBREAK`, `MRET`, illegal-instruction traps, and the implemented
  machine-mode CSRs through the Zicsr register and immediate forms
- dependent ALU forwarding, a load-use dependency, and a taken redirect whose
  target load immediately feeds a dependent branch
- explicit RV64C integer arithmetic, shifts, loads/stores, branches, jumps,
  `C.EBREAK`, stack-pointer forms, and mixed 16/32-bit fetches
- a 32-bit instruction beginning at `PC[1] = 1`, including one that crosses an
  eight-byte instruction-memory line boundary

Passing the pinned catalog is a regression result rather than an external
certification claim. A bundled test reports through `tohost` and then enters a
terminal loop; `ECALL` is handled architecturally as a machine-mode exception
rather than as a simulation halt.

[act4]: https://github.com/riscv/riscv-arch-test
