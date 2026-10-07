# Rust test kernels

`riscv/kernel/` contains small `no_std` Rust kernels that exercise the CPU the
way an operating system does: trap handling, preemption, system calls, faults
and isolation. They are self-checking: success writes `1` to `tohost`, a failed
check or panic writes an odd value.

| ELF | Mode | Requires | What it checks |
| --- | --- | --- | --- |
| `kernel-tasks.elf` | M | Interrupts | Two tasks on 16 KiB stacks, ECALL yields and timer preemption of both, stack guards, register-live arithmetic |
| `kernel-access-faults.elf` | M | Strict memory | The same tasks after recovering from eight access faults (high-address aliases, RAM-boundary crossings, invalid jump targets, fetches at the last parcel) |
| `kernel-syscalls.elf` | M | Strict memory | Fault recovery plus the syscall ABI below; 32 shared increments checked by per-task bitmaps |
| `kernel-console.elf` | M | Console | Live log output and an interrupt-driven `ping`/`pong` exchange with the host |
| `kernel-atomics.elf` | M | A extension | The syscall workload plus real AMO and LR/SC increments from both preemptible tasks |
| `kernel-user.elf` | M + U | U, PMP, A | Two isolated U tasks; the M-mode scheduler switches PMP on every context switch |
| `kernel-supervisor.elf` | M + S + U | S, Sv39 | Minimal M firmware, an S-mode kernel, and two U tasks in separate page tables |

Each task must observe at least one real timer preemption before it may finish;
a fixed number of yields alone could align all ticks with one task.

## Building and checking

```sh
rustup component add rust-src --toolchain 1.98.1
python3 riscv/kernel/build.py        # M-mode kernels -> riscv/build/rust-kernel/
python3 riscv/kernel/build-user.py   # kernel-user, kernel-supervisor -> riscv/build/user-kernel/
python3 riscv/kernel/check.py        # build, audit and run the M-mode kernels in all clock models
```

The M-mode kernels build `core` for `riscv64im-unknown-none-elf` (adding C,
Zicsr and Zifencei); `kernel-atomics`, `kernel-user` and `kernel-supervisor`
use `riscv64imac-unknown-none-elf`. The scripts use `-Z build-std=core` with
`RUSTC_BOOTSTRAP=1` scoped to that one Cargo command. Each writes a manifest
(`build.json`) with compiler arguments and source/ELF hashes and audits the
disassembly: F/D/V instructions are rejected and A is allowed only where the
target CPU has it.

The kernels read the timebase frequency from MMIO (`0x0200_bff0`) and compute a
50 µs scheduling quantum at boot, so they run at any clock profile.

## M-mode kernels

`src/entry.S` owns startup and a 288-byte trap frame holding x1–x31 and machine
trap state; `src/main.rs` holds the scheduler and the workloads. The trap
handler runs with interrupts disabled and returns the frame pointer to restore.
There is no allocator or MMU.

### Syscall ABI (`syscalls` feature)

`a7` selects the operation and `a0` carries the argument and result. ECALL
advances the saved PC by four. These are M-mode tasks sharing one address space.

| `a7` | Operation | `a0` on return |
| --- | --- | --- |
| 0 | Yield | Unchanged |
| 1 | Exit task | Does not return |
| 2 | Task identity | 0 or 1 |
| 3 | Add `a0` to a shared u64 counter | Previous value |
| 4 | Read platform time | Timer ticks |
| other | Unknown | −38 (`ENOSYS`) |

### Console kernel

`kernel-console.elf` uses the [JTAG console](platform.md#jtag-console). The boot
code and the non-nested trap handler own the transmitter. The host sends
`ping\n`; the RX interrupt handler checks it and replies `pong` before either
task can finish. Run it with `--kernel-console` in the simulator or the hardware
runner. Its cycle counts include host waiting and are not a performance metric.

## U-mode kernel (`kernel-user.elf`)

Two U tasks share a read/execute code page and each own a read/write data and
stack page. Timer interrupts and ECALL yields switch tasks; the M-mode handler
reprograms PMP before returning. Trap frames go on a kernel stack found through
`mscratch`, never through the user stack pointer. The tasks probe and expect
faults for: privileged CSRs, MRET, WFI and counter access; loads, stores and
atomics to kernel memory, the other task and MMIO; execution from data pages;
and instructions and stores that cross a PMP boundary.

## S-mode kernel (`kernel-supervisor.elf`)

`src/supervisor_entry.S` is a minimal M-mode firmware: it delegates exceptions
and the supervisor timer interrupt, enters S, and offers SBI-like ECALLs to set
the timer and to finish (it forwards MTI as STIP). The S kernel in
`src/supervisor.rs` gives each task its own root page table. Only the task's text
page (VA `0x10000`) and data page (VA `0x20000`) are user-accessible; the kernel
(a 1 GiB identity mapping) and the timer are S-only. Task switches write `satp`
and execute `SFENCE.VMA`.

It repeats the U-mode kernel's probes, which now show up as page faults:
privileged instructions and `rdcycle` (illegal instruction), kernel variables
and the other task's frames (13/15), timer MMIO (15), executing the data page
(12), an instruction that runs onto an unmapped page (12, with `stval` at the
next page), and a store that crosses a page (misaligned, 6). It also checks
preemption, yields and that an invalid user stack pointer cannot corrupt the
kernel stack.

On the 112 MHz profile it takes 512,104 cycles, about 2.2 times the U-mode
kernel's 231,189, mostly because the TLBs hold only 4 KiB of the kernel's
identity mapping per entry.
