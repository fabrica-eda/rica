# RISC-V executable tests

`rv32i-subset/` contains self-checking RV32 ELF tests assembled independently
of the Rust trace regression. Each ELF starts at address zero and reports its
result by storing to the global `tohost` symbol:

- `1`: pass
- `(test_number << 1) | 1`: fail

Build and run every bundled test with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv32-riscv-tests
```

The runner can also execute externally generated self-checking ELFs:

```sh
cargo run --locked --offline -p verify-riscv --bin rv32-riscv-tests -- \
  --max-cycles 1000000 path/to/test.elf [path/to/another.elf ...]
```

An input ELF must be little-endian RV32, contain a `tohost` symbol, and use the
Core's currently implemented word memory interface. For a nonzero ELF entry,
the loader places a two-instruction `LUI`/`JALR` reset trampoline at address
zero; ELFs that already occupy that trampoline are rejected. On failure or
timeout, the runner prints the most recent commit trace.

The bundled build locates LLVM 20's `llvm-mc` and Rust's bundled `rust-lld`.
Set `LLVM_MC` or `RUST_LLD` to override either executable.

## Architectural test integration

The current [RISC-V Architectural Certification Test framework][act4] is
tracked at a fixed revision in `third_party/riscv-arch-test`. Initialize it
after cloning Rica with:

```sh
git submodule update --init
```

ACT4 generates self-checking ELFs with expected results derived from the Sail
model. Once those ELFs have been generated, run its complete RV32I catalog
through Rica's runner with:

```sh
cargo run --locked --offline -p verify-riscv --bin rv32-riscv-tests -- \
  --act4 --act4-elf-dir path/to/act4/elfs
```

The runner discovers every `I-*-00.S` test directly from the submodule. Tests
absent from `act4-enabled.txt` are printed as `IGNORE`; enabled tests must have
a matching ELF and must pass. New tests added by a later submodule update are
therefore ignored by default instead of silently disappearing from the suite.
No official test source is copied, renamed, or patched in Rica.

`act4-enabled.txt` is intentionally empty today. ACT4's common RV32I bootstrap
uses `SLLI` and `SRLI` while initializing registers, so even an `ADD` test is
not runnable on the current subset Core yet. Add a test stem only after both
the instruction under test and all shared harness instructions are supported.

The local `rv32i-subset/` programs remain fast bring-up smoke tests, not a
replacement for or fork of ACT4. They currently cover:

- `LUI`, `AUIPC`, `JAL`, `JALR`
- `BEQ`, `BNE`
- `LW`, `SW`
- `ADDI`, `XORI`, `ORI`, `ANDI`
- `ADD`, `SUB`, `XOR`, `OR`, `AND`
- dependent ALU forwarding and a load-use dependency

Full ACT4 RV32I runs must wait for the missing shifts, signed/unsigned compare
and branches, byte/halfword memory operations, `FENCE`, and the required
exception behavior. Until then, passing the smoke suite means only the listed
subset works; it does not claim RV32I certification.

[act4]: https://github.com/riscv/riscv-arch-test
