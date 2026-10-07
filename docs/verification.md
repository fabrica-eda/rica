# Verification

A change is accepted only when it passes every applicable level below. Timing
and hardware results are recorded for the exact bitstream; a passing simulation
is never treated as hardware qualification.

| Level | What runs |
| --- | --- |
| Unit RTL | Rust tests in `riscv/verify/tests` (native and ECP5-mapped), Veryl native testbenches (`rica-tb`) |
| Architectural tests | ACT4 catalogs from the pinned `third_party/riscv-arch-test` |
| Project ELFs | Bundled assembly suites, generated tests, privileged/Sv39 diagnostics, Rust kernels |
| Clock-order models | Every ELF in both serialized CPU/memory orders, plus the simultaneous-clock model for cycle counts |
| Mutation testing | For privileged features: targeted RTL mutants must each be caught by a test |
| Physical | Routed STA, JTCK check, checked bitgen, then the same ELFs on the board |

## ELF contract

Tests are little-endian RV64 ELFs with a global `tohost` symbol. They store `1`
on success and `(test_number << 1) | 1` on failure, then loop. For a nonzero
entry point the loader writes a short `LUI`/`JALR` trampoline at address 0.
`ECALL` is an ordinary exception, not a simulation halt.

## Bundled suites

Built and run by `rv64-riscv-tests`; sources are in `riscv/tests/`.

| Option | Suite |
| --- | --- |
| *(none)* | `rv64i-subset/`: eight smoke tests covering RV64I, Zca, Zicsr, FENCE.I, traps, counters and forwarding |
| `--memory-tests` | [`rv64-memory/`](../riscv/tests/rv64-memory/README.md): streaming, copy and pointer-chasing |
| `--cache-tests` | [`rv64-cache/`](../riscv/tests/rv64-cache/README.md): self-modifying code and capacity probes |
| `--benchmarks` | [`rv64-benchmarks/`](../riscv/tests/rv64-benchmarks/README.md): ALU, branch, call and load/store microbenchmarks |
| `--predictor-tests` | [`rv64-predictor/`](../riscv/tests/rv64-predictor/README.md): BTB conflicts and nested calls |
| `--m-tests`, `--m-benchmarks` | [`rv64-m/`](../riscv/tests/rv64-m/README.md), [`rv64-m-benchmarks/`](../riscv/tests/rv64-m-benchmarks/README.md) |
| `--zba-tests`, `--zbb-tests`, `--zbs-tests` | Generated B-extension instruction tests |

Built by scripts ([building-and-simulation.md](building-and-simulation.md)):
[`rv64-a/`](../riscv/tests/rv64-a/README.md) (A extension), `rv64-os/`
(interrupts, timer context switch, boot contract), `rv64-u/` (PMP),
`rv64-s/` (S-mode and Sv39), the C workloads and the Rust kernels.

The privileged diagnostics check each expected trap in full (cause, `xepc`,
`xtval`, which handler receives it, where execution resumes and in which mode),
and fail if the wrong handler sees a trap:

| ELF | Coverage |
| --- | --- |
| `supervisor.elf` | Delegation, SRET, CSR visibility, interrupt routing, TVM/TW/TSR, delegated access faults and atomic faults |
| `sv39-data.elf` | 4 KiB/2 MiB/1 GiB pages, hit and miss permission paths, U/SUM/MXR, Svade, reserved bits, misaligned superpages, canonical form, PPN width, PMA and PMP on leaves and PTEs, page crossing, atomics, MPRV, SFENCE.VMA, `satp` switching |
| `sv39-fetch.elf` | Execution from remapped pages, X/U/A/invalid PTEs, non-canonical PC, page-crossing 32-bit instructions on non-adjacent frames, second-parcel faults, PMP on frames and PTE reads, SFENCE.VMA remapping |
| `pmp-machine.elf`, `pmp-cache.elf` | MPRV, locked entries, partial matches, permission-cache hits and misses |

## ACT4

The official [RISC-V architectural tests](https://github.com/riscv/riscv-arch-test)
are pinned as a submodule. Rica's configurations are in `riscv/act4/`, and the
reviewed lists of enabled tests are `riscv/tests/act4-rv64-*-enabled.txt`. Tests
added by a submodule update are reported as `IGNORE` until reviewed.

| Catalog | Tests | Configuration |
| --- | ---: | --- |
| RV64I | 51 | `act4/test_config.yaml` |
| Zca | 32 | `act4/test_config.yaml` |
| M | 13 | `act4/test_config_m.yaml` |
| Zba | 8 | `act4/test_config_zba.yaml` |
| Zbb | 24 | `act4/test_config_zbb.yaml` |
| Zbs | 8 | `act4/test_config_zbs.yaml` |

Generate a catalog inside the submodule with ACT's own tooling, then run it:

```sh
cd third_party/riscv-arch-test
uv run --offline act ../../riscv/act4/test_config.yaml --workdir work --test-dir tests --extensions I
cd ../..
cargo run --locked --release -p verify-riscv --bin rv64-riscv-tests -- --act4
cargo run --locked --release -p verify-riscv --bin rv64-riscv-tests -- --act4-suite Zca
```

Expected signatures come from the Sail model. Passing these catalogs is a
regression result, not certification.

## Clock-order models

The board memory runs on its own clock. The board-memory model runs every ELF
twice, with coincident CPU and memory edges evaluated in each order
(`mem->cpu` and `cpu->mem`), so a request or response that depends on one
evaluation order cannot pass by accident. The simultaneous-clock model
(`--timed-clocks`) evaluates coincident edges together, as the hardware does,
and is the reference for cycle counts: on hardware, fixed-work ELFs match it
exactly or within a few cycles.

## Mutation testing

S-mode and Sv39 were checked by mutating the RTL (removing a condition,
swapping a priority, dropping a flush) and confirming that the tests fail:

| Feature | Mutants | Detected |
| --- | ---: | --- |
| S-mode | 19 | All, by `supervisor.elf` |
| Sv39 data translation | 27 | All, by `sv39-data.elf` |
| Sv39 instruction translation | 24 | 21 by `sv39-fetch.elf`, 1 by the I-cache unit test; 2 are unobservable defensive logic |

The mutation scripts are not part of the repository; the mutants were not
re-run after the registered PMA/PMP checker change of 2026-10-07.

## Regression expectations

- Cycle and commit counts of fixed-work ELFs must not change unless a change is
  intended to affect them; differences are reviewed per ELF.
- New CPU tests that depend on optional features read `misa` and adjust their
  expectations (for example, `system.elf` with and without U/S).
- Interrupt- and timer-driven programs are self-checking but their counts depend
  on timing; compare them only against the simultaneous-clock model at the same
  clock profile.
