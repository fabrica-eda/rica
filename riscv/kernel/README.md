# Rica Rust kernel

A small `no_std` kernel for the single-hart M-mode platform. It boots through the
existing ELF/JTAG loader, saves x1–x31 and machine trap state, and runs two tasks
on separate 16 KiB stacks. ECALL yields and timer interrupts switch the saved
stack frame. The Rust handler returns the frame pointer that assembly restores.
The default kernel has no allocator, dependency crate, atomic emulation or MMU.
The separate `rica-user` workload exercises the opt-in U/PMP platform described below.

Current development kernels read `TIMEBASE_HZ` at `0x0200bff0` and calculate a
50 µs scheduling quantum once at boot. The U-platform redesign uses the clock
profile in [`platform-user.json`](../platform-user.json); native and simultaneous
CPU/memory models use that same profile. Rebuilt kernels and the redesigned
platform require their own qualification; historical hardware results below
refer only to the recorded ELF/bitstream hashes.

The optional `syscalls` workload adds task identity, monotonic timer reads and
a shared fetch-add service, serialized by the existing non-nested trap handler.
It runs on the same qualified 124.8 MHz console image; this service does not
require or advertise the RISC-V A extension.

`src/entry.S` owns startup and the 288-byte trap-frame ABI. `src/main.rs` owns the
scheduler, exception records and self-checking workload. Interrupts stay disabled
inside the handler; static scheduler state has a single non-nested owner. The
task entry never returns. Panic or failed checks report an odd failure code via
`tohost`; success reports 1.

## Build

Use the repository's Rust **1.98.1** and install its matching source component:

```sh
rustup component add rust-src --toolchain 1.98.1
python3 riscv/kernel/build.py
```

The script builds `core` and compiler builtins for
`riscv64im-unknown-none-elf`, adding C, Zicsr and Zifencei. The opt-in `kernel-atomics` variant instead builds `core` for
`riscv64imac-unknown-none-elf` so Rust `AtomicU64` is available. The ordinary
variants avoid the A/F/D
requirements of `riscv64gc-unknown-none-elf`. It scopes `RUSTC_BOOTSTRAP=1` to
the pinned Cargo command for `-Z build-std=core`; it does not change the default
toolchain. [Rust's target documentation](https://doc.rust-lang.org/rustc/platform-support/riscv64im-unknown-none-elf.html)
describes this target's `core` support and lack of prebuilt target artifacts.

The reproducible build is offline. If Cargo's standard-library dependencies are
not cached yet, populate them once from the kernel directory:

```sh
cd riscv/kernel
RUSTC_BOOTSTRAP=1 cargo +1.98.1 build --release --locked -Z build-std=core \
  --target-dir ../build/kernel-target
```

Outputs are under `riscv/build/rust-kernel/`:

- `kernel-tasks.elf`: boot, 32 explicit yields, timer preemption of both tasks,
  stack guards, register-live arithmetic and final progress checks.
- `kernel-access-faults.elf`: the same tasks after eight recoverable faults:
  high-address load/store aliases, crossing load/store at the RAM boundary,
  two full-width invalid jump targets, and 16/32-bit fetches at the last parcel.
- `kernel-console.elf`: access-fault recovery and the same tasks, with live
  USB/JTAG logs and MEI receive interrupts. The host sends `ping\n`; the kernel
  verifies the five bytes in the interrupt handler and replies `pong` before
  either task can finish. Requires the console-enabled platform.
- `kernel-syscalls.elf`: access-fault recovery and two preemptible tasks using
  the syscall ABI below. Each performs 16 shared increments; independent task
  bitmaps verify all 32 returned old values occur exactly once. It also checks
  task identity, monotonic time and unknown-call errors.
- `kernel-atomics.elf`: the syscall/fault workload plus real AMO and LR/SC
  increments from both preemptible tasks. Requires the qualified opt-in A CPU image;
  it deliberately fails on a CPU without `misa.A`.
- `build.json` and disassembly: compiler/source/ELF hashes and an instruction
  allow-list audit. F/D/V instructions are rejected; A is allowed only in `kernel-atomics`.

Each task waits for at least one actual timer preemption before finishing. A
fixed number of yields alone is insufficient: timer/yield periods can align so
all ticks arrive in one task. The first image exposed that assumption on both
the simultaneous-edge model and hardware; its inputs and failure are preserved
in the [measurement directory](../../toolchain/measurements/2026-09-10-rv64-access-faults/).

## Run in the CPU/memory model

Use one command for the CPU/kernel development loop:

```sh
python3 riscv/kernel/check.py
```

It builds and audits all five kernels, builds the repository's model runner, and
runs both serialized clock orders plus simultaneous CPU/memory edges. Reports,
input hashes and logs are written to `riscv/build/rust-kernel/check.json` and
`check-*.log`. This checks the current RTL; physical qualification is separate.
The scripts select Cargo and rustc through the pinned rustup toolchain explicitly,
including inside the Nix shell, so a Cargo wrapper cannot misinterpret `+1.98.1`
or select a different compiler. The host runner is built into the repository's
`target` directory even when `CARGO_TARGET_DIR` is set.

## Optional syscall ABI

Enable `syscalls` to use `a7` as the operation number and `a0` as argument/result.
Other saved registers are restored unchanged. ECALL advances the saved PC by
four bytes. These are M-mode tasks sharing one address space, not isolated users.

| a7 | Operation | a0 on return |
| --- | --- | --- |
| 0 | Yield | Unchanged |
| 1 | Exit task | Does not return |
| 2 | Task identity | 0 or 1 |
| 3 | Add a0 to the shared u64 counter, wrapping | Previous counter value |
| 4 | Read platform time | Ticks at the reported platform timebase |
| Other | Unknown operation | -38 (`ENOSYS`) |

Calls 2 and above resume the caller without rescheduling or postponing its timer
deadline. The fetch-add is atomic only with respect to these single-hart tasks,
because interrupts remain disabled throughout the handler. It is not an ISA AMO
or an SMP synchronization primitive.

The [2026-09-21 verification record](../RV64_OS_ISA_PROGRESS_20260921.md)
includes model results, hardware counters and the separate AMO implementation status.

From the repository root:

```sh
target/release/verify-riscv --hardware-memory-rtl \
  --machine-interrupts --strict-memory --compact-predictor --icache-kib 4 \
  --fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension \
  --early-bitmanip --early-taken-fetch --early-divide --max-cycles 1000000 \
  riscv/build/rust-kernel/kernel-tasks.elf \
  riscv/build/rust-kernel/kernel-access-faults.elf
```

Add `--timed-clocks` for atomic coincident CPU/memory edges. Both exact legacy
ELFs pass three hardware runs on the qualified **124.8 MHz** console image,
`HardwareCompactConsoleTestTop`. Tasks retires **30,725 instructions in
86,652 / 86,653 / 86,653 cycles**; access-faults retires **31,949 instructions
in 92,235 cycles** in all three runs, recovering from all eight faults before
completing both tasks. The medians match the current simultaneous-edge model.
Timer-driven run lengths are integration measurements, not fixed-work IPC benchmarks.

The hardware runner defaults to
`build/riscv-hardware-compact-console-124p8-20260911.bit`.
See the [qualification and counters](../../toolchain/measurements/2026-09-11-rv64-console/PERFORMANCE.md).

## Live console

The console variant uses [the USB/JTAG MMIO device](../RV64_CONSOLE_PLAN.md) at
`0x02001000`; it needs no additional board wiring. Boot and the non-nested trap
handler own the transmitter, so task preemption cannot interleave two writers.
The RX handler drains the FIFO and masks its interrupt after the expected input.
It returns the same task frame and leaves the timer deadline unchanged.

Use the model command above with `--kernel-console` and only
`riscv/build/rust-kernel/kernel-console.elf`. The host model applies output
backpressure, sends input while global interrupts are masked during boot, and
checks all eight 64-bit fault records plus the final success messages.
`--machine-console` enables the device without this kernel-specific host protocol,
for regression of ordinary ELFs on the new platform.

The console kernel passes three runs on the same qualified bitstream, producing
exactly the model's 686-byte transcript and receiving `ping\n` through MEI.
Run it on the default image with:

```sh
target/release/hardware-riscv-tests --kernel-console --xvc 127.0.0.1:3721 \
  riscv/build/rust-kernel/kernel-console.elf
```

The runner streams logs while the CPU runs and drains remaining output after
`tohost` freezes the counters. A disconnected host applies backpressure;
console waiting time is included in those counters. Compare CPU performance
using the unchanged plain ELFs, not the host-dependent console run.

A plus atomics is now qualified at 124.8 MHz. The next CPU/kernel pairs are
U/PMP plus syscall and isolation,
then S/Sv39 plus page tables and process switching. See the
[OS/ISA plan](../RV64_OS_ISA_PLAN.md) and [platform map](../RV64_MACHINE_PLATFORM.md).

See the [A platform contract](../RV64_ATOMIC_PLATFORM.md) for reservation,
ordering, fault and physical-qualification scope.


## Qualified hardware atomics (2026-09-22 JST)

The A-enabled image passes all five kernel variants at the same 124.8 MHz CPU
clock. `kernel-atomics` records mcycle=127145 and minstret=41092, matching the
simultaneous-edge model. Select its bitstream explicitly (the runner default
is still the older console image):

```sh
target/release/hardware-riscv-tests --xvc 127.0.0.1:3721 \
  --bitstream riscv/build/riscv-hardware-compact-atomic-124p8-20260922.bit \
  riscv/build/rust-kernel/kernel-atomics.elf
```

See the [timing, exact artifact hashes and 267-run hardware qualification](../RV64_ATOMIC_PLATFORM.md#hardware-qualification-2026-09-22-jst).

## U-mode isolation workload

`python3 riscv/kernel/build-user.py` builds the separate `rica-user` binary into
`riscv/build/user-kernel/kernel-user.elf`, without rebuilding the qualified
M-mode images. It requires `HardwareCompactUserTestTop` (`ENABLE_U=1`, A and
strict memory enabled). It must not be loaded onto the older M-only image.

Two U-mode tasks share an RX code page and have distinct RW data/stack pages.
The M-mode scheduler switches PMP mappings on timer interrupts and ECALLs.
The workload checks CSR privilege violations, kernel and other-task protection,
MMIO and atomic access faults, NX, accesses crossing PMP boundaries, and trap
entry with a deliberately invalid user stack pointer. See
[the U/PMP platform specification](../RV64_USER_PLATFORM.md) for the precise
architecture and qualification status. S-mode and virtual memory are not implemented.
