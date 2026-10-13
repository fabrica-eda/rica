# Rica

Rica is a 64-bit RISC-V CPU written in [Veryl](https://veryl-lang.org/) for the
Lattice ECP5 **LFE5UM5G-85F** (the LFE5UM5G-85F-EVN evaluation board). It is a
scalar, in-order core with M/S/U privilege levels and Sv39 virtual memory, and it
is developed together with an open FPGA toolchain:

- [Struo](https://github.com/fabrica-eda/struo) synthesizes Veryl to ECP5 primitives.
- [Texo](https://github.com/fabrica-eda/texo) places, routes, runs static timing
  analysis (STA) and generates bitstreams.
- [Celox](https://github.com/celox-sim/celox) simulates the RTL.
- [tapir](https://github.com/fabrica-eda/tapir) programs the board and serves JTAG
  over XVC.

Every image described as "on hardware" below was built with this toolchain,
closed timing in routed STA, and passed its test suite on a physical board.

## Status

| Profile | Board top | Clocks (CPU/memory) | ISA and platform | Hardware result |
| --- | --- | --- | --- | --- |
| **Working profile** | `HardwareCompactUserTestTop` | 112 / 56 MHz | RV64IMAC (C = Zca), Zba/Zbb/Zbs, Zicsr, Zifencei; M/S/U, PMP, **Sv39** | 274 ELFs pass (2026-10-07) |
| Atomic M-mode profile | `HardwareCompactAtomicTestTop` | 124.8 / 62.4 MHz | RV64IMAC (C = Zca), Zba/Zbb/Zbs; M-mode only | 267 runs pass (2026-09-22) |

The working profile's clock is set by [`riscv/platform-user.json`](riscv/platform-user.json).
The 274-ELF suite covers the pinned official RISC-V architecture tests (ACT4:
RV64I, Zca, M, Zba, Zbb, Zbs), the project's own instruction, cache, predictor,
memory, interrupt, PMP, S-mode and Sv39 diagnostics, and four Rust test kernels,
including an S-mode kernel that runs isolated user tasks in their own page tables.
On hardware, `mcycle` matches the simultaneous-clock simulation model within one
cycle for the Sv39 tests and kernels.

Not implemented on the FPGA: F/D/V, hypervisor, multiple harts, PLIC/AIA,
production SBI firmware, and Linux boot. A separate
[Linux RTL simulation harness](docs/building-and-simulation.md#linux-in-the-core-rtl-simulator)
provides 128 MiB of host-backed RAM and minimal SBI firmware. Linux 6.12 has
booted through a static userspace `/init`, console output and normal poweroff
on this RTL simulation platform.
See [docs/history.md](docs/history.md) for the hardware milestones and measurements.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | Pipeline, front end, branch prediction, caches, execution units, LSU |
| [docs/privileged.md](docs/privileged.md) | M/S/U, CSRs, traps and delegation, PMP, Sv39 |
| [docs/platform.md](docs/platform.md) | Memory map, timer, console, boot, JTAG debug transport, clocks |
| [docs/building-and-simulation.md](docs/building-and-simulation.md) | Development shell, building tests and kernels, simulation |
| [docs/fpga-flow.md](docs/fpga-flow.md) | Synthesis, place-and-route, timing sign-off, checked bitgen |
| [docs/hardware-testing.md](docs/hardware-testing.md) | Programming the board and running tests on hardware |
| [docs/verification.md](docs/verification.md) | Test suites, ACT4, clock-order models, mutation testing |
| [docs/kernels.md](docs/kernels.md) | The Rust test kernels |
| [docs/timing-characterization.md](docs/timing-characterization.md) | On-board delay measurements behind the STA library |
| [docs/design-notes.md](docs/design-notes.md) | Hazards and timing lessons that shaped the RTL |
| [docs/toolchain-issues.md](docs/toolchain-issues.md) | Tool defects found while bringing up Rica |
| [docs/history.md](docs/history.md) | Milestones, dates and key numbers |
| [i2c/README.md](i2c/README.md) | The JTAG-to-I2C debug target used for early board bring-up |

## Repository layout

| Path | Contents |
| --- | --- |
| `riscv/src/` | CPU and board RTL (Veryl) |
| `riscv/verify/` | Simulation runners, the hardware test runner and Rust RTL tests (`verify-riscv`) |
| `riscv/tests/` | Self-checking assembly tests, ACT4 enable lists and Veryl native testbenches |
| `riscv/kernel/` | Rust `no_std` test kernels |
| `riscv/tools/` | Texo launchers, bitgen wrapper, test generators and build scripts |
| `riscv/characterization/` | On-board delay measurement and STA-library fitting scripts |
| `riscv/act4/` | ACT4 configurations for Rica |
| `i2c/` | Debug I2C target and its verifier |
| `toolchain/` | Veryl testbench runner (`rica-tb`) and the Texo launcher crate |
| `third_party/riscv-arch-test` | Official architecture tests (git submodule) |

## Quick start

Requirements: Linux (x86_64 or aarch64, WSL2 works) with Nix flakes enabled.

```sh
git clone --recurse-submodules <this repository>
cd rica
nix develop                      # pinned Rust, Python, compilers and mbx
cargo test --workspace --locked  # Rust and RTL unit tests
```

Build and run the bundled RV64 tests in simulation:

```sh
cargo run --locked --release -p verify-riscv --bin rv64-riscv-tests
```

Run an ELF on the working profile's RTL model (both CPU/memory clock orders):

```sh
cargo build --locked --release -p verify-riscv
target/release/verify-riscv --hardware-memory-rtl --compact-predictor --icache-kib 4 \
  --m-extension --dsp-multiplier --zba-extension --zbb-extension --zbs-extension \
  --early-bitmanip --early-taken-fetch --early-divide --machine-interrupts \
  --machine-console --strict-memory --a-extension --u-mode --sv39 --fast-lsu 6 \
  --max-cycles 2000000 path/to/test.elf
```

Place and route the working profile, then follow
[docs/fpga-flow.md](docs/fpga-flow.md) for timing sign-off and bitgen and
[docs/hardware-testing.md](docs/hardware-testing.md) to run it on the board.

## License

Rica is licensed under the [MIT License](LICENSE). The
`third_party/riscv-arch-test` submodule keeps its own licenses.
