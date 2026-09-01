# RISC-V executable tests

`rv64i-subset/` contains self-checking RV64 ELF tests assembled independently
of the Rust trace regression. Each ELF starts at address zero and reports its
result by storing to the global `tohost` symbol:

- `1`: pass
- `(test_number << 1) | 1`: fail

Build and run every bundled test with:

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

The default command programs `build/riscv-hardware-test-125-texo.bit` and then runs
all bundled tests. `--no-program` reuses the programmed image,
`--bitstream path` selects another image, and `--run-tck count` changes the CPU
execution window. `--expect-tohost value` selects an exact diagnostic mailbox
value. Positional ELF paths select individual or external tests.
Each ELF must fit in the 4 KiB reset-trampoline window
`0x00000000..0x00000fff` and the 252 KiB test window
`0x80000000..0x8003efff`.

The runner writes generated SVF files under `build/hardware-rv64`. A successful
result covers the entire board path: JTAGG ER1, 128-bit command transport,
BRAM loading, PLL/reset, RV64 execution, and `tohost` readback. The timing-
closed 256 KiB wrapper passes all six bundled smoke tests and all 51 enabled
ACT4 RV64I tests on the EVN at 124.8 MHz.

## Architectural test integration

The current [RISC-V Architectural Certification Test framework][act4] is
tracked at a fixed revision in `third_party/riscv-arch-test`. Initialize it
after cloning Rica with:

```sh
git submodule update --init
```

ACT4 generates self-checking ELFs with expected results derived from the Sail
model. Once those ELFs have been generated, run its complete RV64I catalog
through Rica's runner with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- \
  --act4 --act4-elf-dir path/to/act4/elfs
```

The runner discovers every `I-*-00.S` test directly from the submodule. All 51
tests in the pinned RV64I catalog are enabled in `act4-rv64-enabled.txt` and
pass both the ordinary Core simulator and the synchronous HardwareMemory RTL
model. Tests added by a later submodule update are printed as `IGNORE` until
they are reviewed and enabled, instead of silently disappearing from the
suite. No official test source is copied, renamed, or patched in Rica.

The local `rv64i-subset/` programs remain fast bring-up smoke tests, not a
replacement for or fork of ACT4. They currently cover:

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
- dependent ALU forwarding and a load-use dependency

Passing the pinned catalog is a regression result rather than an external
certification claim. A bundled test reports through `tohost` and then enters a
terminal loop; `ECALL` is handled architecturally as a machine-mode exception
rather than as a simulation halt.

[act4]: https://github.com/riscv/riscv-arch-test
