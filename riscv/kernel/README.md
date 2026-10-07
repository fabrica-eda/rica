# Rica test kernels

Small `no_std` Rust kernels that exercise traps, preemption, system calls,
faults, PMP isolation and Sv39 page tables. See
[docs/kernels.md](../../docs/kernels.md) for what each kernel checks and how to
build and run it.

```sh
rustup component add rust-src --toolchain 1.98.1
python3 riscv/kernel/build.py        # M-mode kernels
python3 riscv/kernel/build-user.py   # U-mode and S-mode kernels, privileged diagnostics
python3 riscv/kernel/check.py        # build and simulate the M-mode kernels
```
