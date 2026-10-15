# Building and simulation

## Development shell

Rica uses a Nix flake for its toolchain. On x86_64 or aarch64 Linux (WSL2
included), with flakes and `nix-command` enabled:

```sh
nix develop
cargo test --workspace --locked
```

The shell provides Rust (from `rust-toolchain.toml`, currently 1.98.1),
rustfmt, Clippy, rust-analyzer, C/C++ compilers, CMake, pkg-config, OpenSSL,
Python (with `cbor2` and `zstandard`) and [mbx](https://github.com/tignear/nix-packages).
Plain `cargo` runs through mbx's shared build cache. With direnv installed,
`direnv allow` activates the shell automatically. Update the pins with
`nix flake update`.

Struo, Texo and Celox are ordinary crates.io dependencies declared in
`Cargo.toml`; `Cargo.lock` records the exact versions.

The architecture tests are a git submodule:

```sh
git submodule update --init
```

## Simulation runners

`verify-riscv` (package `verify-riscv`) provides three simulation binaries:

| Binary | Purpose |
| --- | --- |
| `verify-riscv` | Core and board-memory RTL simulation of ELFs, register-file and mapping checks |
| `rv64-riscv-tests` | Builds and runs the bundled assembly suites and ACT4 catalogs |
| `hardware-riscv-tests` | Runs ELFs on the FPGA ([hardware-testing.md](hardware-testing.md)) |

Build them once in release mode:

```sh
cargo build --locked --release -p verify-riscv
```

### Models

- **Core model** (default): the core with immediate instruction and data memory.
  Fast; used for architectural regression of the bundled suites.
- **Board-memory model** (`--hardware-memory-rtl`): the core with the real
  `HardwareMemory` RTL, I-cache and dual-clock memory. Each ELF runs twice, once
  with coincident CPU/memory edges evaluated memory-first and once CPU-first, so
  that neither order can hide a handshake bug.
- **Simultaneous-clock model** (`--hardware-memory-rtl --timed-clocks`): Celox's
  `tick_events` evaluates coincident edges together on the native backend. Its
  `mcycle`/`minstret` match the FPGA, so use it for cycle counts. It compiles one
  model per ELF and needs explicit ELF paths.

### Feature flags

The model is configured with flags matching the board parameters:

| Flag | Feature |
| --- | --- |
| `--compact-predictor`, `--icache-kib 4` | Working-profile front end (`--icache-kib` needs `--hardware-memory-rtl`) |
| `--m-extension`, `--dsp-multiplier`, `--early-divide` | M extension |
| `--zba-extension`, `--zbb-extension`, `--zbs-extension`, `--early-bitmanip` | B extension |
| `--a-extension` | A extension (needs `--strict-memory`) |
| `--machine-interrupts`, `--machine-console`, `--strict-memory` | Platform |
| `--u-mode`, `--s-mode`, `--sv39` | Privilege; `--sv39` implies `--s-mode` |
| `--fast-lsu 6`, `--early-taken-fetch` | LSU and fetch timing |
| `--kernel-console` | Host side of the console kernel's ping/pong protocol |
| `--project PATH` | Read RTL and `platform-user.json` from another project snapshot |
| `--max-cycles N` | Timeout |

The working profile:

```sh
target/release/verify-riscv --hardware-memory-rtl --compact-predictor --icache-kib 4 \
  --m-extension --dsp-multiplier --zba-extension --zbb-extension --zbs-extension \
  --early-bitmanip --early-taken-fetch --early-divide --machine-interrupts \
  --machine-console --strict-memory --a-extension --u-mode --sv39 --fast-lsu 6 \
  --max-cycles 2000000 ELF...
```

Add `--timed-clocks` for cycle-accurate counts. Without a predictor flag the
core uses the compact predictor.

## Building test programs

### Assembly suites

`rv64-riscv-tests` assembles its suites with LLVM 20's `llvm-mc` and Rust's
bundled `rust-lld` (override with `LLVM_MC` / `RUST_LLD`) and runs them:

```sh
cargo run --locked --release -p verify-riscv --bin rv64-riscv-tests             # bundled RV64I/C/system tests
cargo run --locked --release -p verify-riscv --bin rv64-riscv-tests -- --m-tests --max-cycles 1000000
```

Other suites: `--benchmarks`, `--memory-tests`, `--cache-tests`,
`--predictor-tests`, `--m-benchmarks`, `--zba-tests`, `--zbb-tests`,
`--zbs-tests`, and the ACT4 catalogs (`--act4`, `--act4-suite NAME`). Outputs go
to `riscv/build/`. See [verification.md](verification.md).

Generators and builders for the remaining diagnostics live in `riscv/tools/`:

| Script | Output |
| --- | --- |
| `build_rv64_a.py` | A-extension diagnostics (`riscv/build/rv64-a/`) |
| `build_rv64_machine_interrupts.py` | `machine_interrupts.elf`, `timer_context_switch.elf` |
| `build_rv64_zba_c.py`, `build_rv64_zbb_c.py`, `build_rv64_zbs_c.py`, `build_rv64_divide_c.py` | Paired C workloads with and without the extension |
| `generate_rv64_m_tests.py`, `generate_rv64_m_benchmarks.py`, `generate_rv64_z*_tests.py` | Generated instruction tests |

C workloads need Clang 20; set `RISCV_CLANG` to its path.

### Kernels and privileged diagnostics

```sh
rustup component add rust-src --toolchain 1.98.1
python3 riscv/kernel/build.py       # M-mode kernels -> riscv/build/rust-kernel/
python3 riscv/kernel/build-user.py  # U/S kernels and PMP/S-mode/Sv39 tests -> riscv/build/user-kernel/
python3 riscv/kernel/check.py       # build, audit and simulate the M-mode kernels
```

`build-user.py` produces `kernel-user.elf`, `kernel-supervisor.elf`, and the
assembly diagnostics `pmp-machine.elf`, `pmp-cache.elf`, `system.elf`,
`supervisor.elf`, `sv39-data.elf` and `sv39-fetch.elf`. Both scripts write a
manifest with compiler arguments and source/ELF hashes, and audit the
disassembly against an allowed instruction set. See [kernels.md](kernels.md).

## Veryl native testbenches

Some unit tests are Veryl testbenches run by Celox:

```sh
cargo run --locked -p rica-tb -- native                       # all testbenches
cargo run --locked -p rica-tb -- native performance_counter   # one testbench
cargo test --locked -p rica-tb                                # all backends
```

`riscv/tests/suite.txt` lists each testbench and its sources; they are read at
run time, so HDL edits need no Rust rebuild.

## Rust RTL tests

`riscv/verify/tests/` holds the larger RTL tests: native and ECP5-mapped
equivalence for units such as the PMP checker, CSR file, page-table walker,
atomic units, ALU, multiplier and JTAG transport, plus predictor and pipeline
integration tests.

```sh
cargo test --locked --release -p verify-riscv
cargo test --locked --release -p verify-riscv --test page_table_walker
```

## I2C debug target

```sh
cargo run --locked --release -p verify-i2c
```

See [i2c/README.md](../i2c/README.md).

## Linux in the core RTL simulator

`linux-sim` runs a single-hart RV64IMAC/Sv39 Linux Image on Rica's `Core`
through Celox. Its platform has 128 MiB of host-backed RAM and immediate
instruction memory, with acknowledged data transactions. This is a separate
simulation platform: the FPGA's 252 KiB RAM, board memory timing and hardware
image are not enlarged by this harness.

The CPU executes the firmware, kernel and userspace instructions. The host
supplies memory, a timer and a byte console; it does not emulate instructions
or intercept Linux system calls. `riscv/linux/firmware.c` provides the minimal
single-hart SBI services needed here, forwards machine timer interrupts to S,
and emulates `rdtime` in an M-mode illegal-instruction handler. Other unexpected
firmware traps stop the run. This firmware is a bring-up fixture, not a general
SBI implementation.

Use Linux 6.12 source and Clang/LLD with the RISC-V backend, plus the kernel's
usual host build dependencies (make, flex, bison and bc). On Nix, point
`RISCV_CLANG` at the unwrapped Clang binary and `RISCV_LLD` at `ld.lld`;
`LLVM` may be set to the LLVM bin directory with a trailing slash when the
LLVM utilities are not on PATH. Build output belongs in `work/`, outside
Cargo's managed `target/`.

```sh
mkdir -p work/linux-boot
curl -fL https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.tar.xz \
  -o work/linux-boot/linux-6.12.tar.xz
tar -xJf work/linux-boot/linux-6.12.tar.xz -C work/linux-boot
bash riscv/linux/build-kernel.sh work/linux-boot/linux-6.12 work/linux-boot/kernel-build
python3 riscv/linux/prepare.py work/linux-boot
cargo build --locked --release -p verify-riscv --bin linux-sim
target/release/linux-sim work/linux-boot/kernel-build/arch/riscv/boot/Image \
  work/linux-boot/firmware.elf work/linux-boot/platform.dtb \
  work/linux-boot/initramfs.cpio 200000000 \
  > work/linux-boot/console.log 2> work/linux-boot/simulation.log
```

The kernel config starts from `tinyconfig` and enables Sv39, ELF loading,
initramfs and SBI console support, with FPU, vector, SMP and tracing disabled.
The crypto framework is enabled so `CRYPTO_MANAGER_DISABLE_TESTS` can disable
long startup hash self-tests during repeated RTL bring-up runs.
`prepare.py` creates a DTB, a minimal static `/init`, a newc initramfs and a
manifest of firmware/toolchain hashes. The init program writes
`RICA_LINUX_USERSPACE_OK` through Linux's `write` syscall, then requests poweroff.
The verified Linux 6.12 run reached this marker and normal poweroff in
67,035,047 cycles (about four minutes on the bring-up host). The string is
first accessed by the kernel's user-copy routine, exercising demand paging.
The runner returns success only after observing that marker and a successful
SBI shutdown. A cycle limit, unexpected firmware trap or shutdown without the
marker is a failure; a Linux banner alone is not success.

The fixed physical layout is firmware at `0x80000000` (first 2 MiB reserved),
Linux at `0x80200000`, initramfs at `0x86000000`, and DTB at `0x87e00000`.
The timer is 1 MHz, advancing once every 100 simulated CPU cycles (a
100 MHz simulated CPU). Set `LINUX_SIM_TIMER_DIVISOR` identically for
`prepare.py` and the runner to change this ratio. `LINUX_SIM_TRACE_EVERY=N`
logs architectural register observations and recent commits every N cycles.
`LINUX_SIM_TRACE_AFTER_USER=1` logs each retirement and data transaction after
the first userspace instruction for fault diagnosis (potentially large logs).
Very low CPU/timer ratios can leave Linux spending most of its time servicing
timer interrupts. MMIO at
`0x02001000` writes console bytes and `0x02001008` reports firmware termination;
these host fixture registers are not the FPGA JTAG-console register map.
The `RAM_END` RTL parameter defaults to the existing FPGA bound; the runner
sets it to `0x88000000`. It must be a 4 KiB-aligned exclusive end within the
32-bit physical space. Expanded RAM also widens LR/SC reservation addresses.

Regression checks for both the default and expanded bounds:

```sh
cargo test --locked --release -p verify-riscv \
  --test physical_access --test offset_access --test atomic_lsu \
  --test sv39_fault_recovery
```

Linux bring-up exposed a late-fault recovery bug: a data page fault at a high
virtual PC could lose redirect priority to compact-predictor repair, skipping
the load and continuing with stale register data. CM translation and atomic
faults now suppress prediction repair. The focused recovery test checks load,
store and atomic faults, saved exception state and discarded younger work.
