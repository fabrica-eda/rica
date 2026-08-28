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

The same ELF contract is supported by the JTAG-I2C hardware harness:

```sh
cargo run --locked --offline --release -p verify-riscv --bin hardware-riscv-tests
```

The default command programs `build/riscv-hardware-test-120.bit` and then runs
all bundled tests. `--no-program` reuses the programmed image,
`--bitstream path` selects another image, and `--run-tck count` changes the CPU
execution window. Positional ELF paths select individual or external tests.
Each ELF must fit in the two 2 KiB physical windows
`0x00000000..0x000007ff` and `0x80000000..0x800007ff`.

The runner writes generated SVF files under `build/hardware-rv64`. A successful
result covers the entire board path: JTAGG ER1, I2C register transport, BRAM
loading, PLL/reset, RV64 execution, and `tohost` readback. All five bundled
tests, including the directed `SW; LD; LD; BNE` memory regression, pass on the
EVN at 120 MHz.

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

The runner discovers every `I-*-00.S` test directly from the submodule. Tests
absent from `act4-rv64-enabled.txt` are printed as `IGNORE`; enabled tests must
have a matching ELF and must pass. New tests added by a later submodule update
are therefore ignored by default instead of silently disappearing from the
suite. No official test source is copied, renamed, or patched in Rica.

`act4-rv64-enabled.txt` is intentionally empty today. ACT4's common RV64I
bootstrap uses `SLLI` and `SRLI` while initializing registers, so even an
`ADD` test is not runnable on the current subset Core yet. Add a test stem only
after both the instruction under test and all shared harness instructions are
supported.

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
- dependent ALU forwarding and a load-use dependency

Full ACT4 RV64I certification still requires architectural exception and trap
behavior. The current bare-metal test convention writes `tohost` and then uses
`ECALL` as a simulation halt; it is not an implementation of the mandated
environment-call exception.

[act4]: https://github.com/riscv/riscv-arch-test
