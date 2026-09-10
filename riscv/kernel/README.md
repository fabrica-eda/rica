# Rica Rust kernel

A small `no_std` kernel for the single-hart M-mode platform. It boots through the
existing ELF/JTAG loader, saves x1–x31 and machine trap state, and runs two tasks
on separate 16 KiB stacks. ECALL yields and timer interrupts switch the saved
stack frame. The Rust handler returns the frame pointer that assembly restores.
There is no allocator, dependency crate, atomic emulation, S/U mode or MMU yet.

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
`riscv64im-unknown-none-elf`, adding C, Zicsr and Zifencei. This avoids the A/F/D
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
- `build.json` and disassembly: compiler/source/ELF hashes and an instruction
  allow-list audit. Encoded A/F/D/V instructions are rejected.

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

It builds and audits both kernels, builds the repository's model runner, and
runs both serialized clock orders plus simultaneous CPU/memory edges. Reports,
input hashes and logs are written to `riscv/build/rust-kernel/check.json` and
`check-*.log`. This checks the current RTL; physical qualification is separate.

From the repository root:

```sh
target/release/verify-riscv --hardware-memory-rtl \
  --machine-interrupts --strict-memory --compact-predictor --icache-kib 4 \
  --fast-lsu 6 --dsp-multiplier --zba-extension --zbb-extension --zbs-extension \
  --early-bitmanip --early-taken-fetch --early-divide --max-cycles 1000000 \
  riscv/build/rust-kernel/kernel-tasks.elf \
  riscv/build/rust-kernel/kernel-access-faults.elf
```

Add `--timed-clocks` for atomic coincident CPU/memory edges. Both ELFs now pass
three hardware runs on the qualified **124.8 MHz** strict-memory image,
`HardwareCompactAccessFaultTestTop`. Tasks retires **30,725 instructions in
86,653 cycles** in every run; access-faults retires **31,949 instructions in
92,234 / 92,235 / 92,235 cycles**, recovering from all eight faults before
completing both tasks. The medians match the current simultaneous-edge model.
The previous interrupt image ran the same task ELF in 86,644 cycles; this
comparison includes the revised interrupt-claim behavior. Timer-driven run
lengths are integration measurements, not fixed-work IPC benchmarks.

The hardware runner defaults to
`build/riscv-hardware-compact-access-faults-124p8-20260910.bit`.
See the [qualification and counters](../../toolchain/measurements/2026-09-10-rv64-access-faults/PERFORMANCE.md).

The next CPU/kernel pairs are A plus atomics, U/PMP plus syscall and isolation,
then S/Sv39 plus page tables and process switching. See the
[OS/ISA plan](../RV64_OS_ISA_PLAN.md) and [platform map](../RV64_MACHINE_PLATFORM.md).
