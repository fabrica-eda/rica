# Machine interrupt platform

`HardwareCompactInterruptTestTop` enables `ENABLE_INTERRUPTS=1` with the existing
compact predictor, 4 KiB instruction cache, `FAST_LSU=6`, DSP M, B and early division.
The qualification status and exact bitstream are recorded in
[the interrupt measurement record](../toolchain/measurements/2026-09-10-rv64-machine-interrupts/README.md).
The default hardware runner selects
`build/riscv-hardware-compact-interrupt-124p8-20260910.bit`, qualified at
124.8 MHz with 200 ps setup/hold reserves and 326 physical executions.
Its SHA-256 is `48f9939da65666197fc5f70ede7690ea1d7515bea00bacb017b87f5087861466`.

## Interrupt interface and architectural boundary

Core accepts three **CPU-clock synchronous levels** in `machine_interrupts[2:0]`:
external, timer, software. An asynchronous external source requires platform
synchronization before this input. The board connects the memory-domain timer
and MSIP through a CPU-domain sampling register; the clocks are related 2:1.
The board has no external device/controller connected yet and ties MEIP low.

With this configuration, `mie` (0x304) implements bits 11, 7 and 3. Other bits
read zero and ignore writes. `mip` (0x344) exposes the corresponding source
levels even when masked. All six CSR forms are legal on both addresses, but
MIP writes do not acknowledge these read-only bits. MMIO controls MSI/MTI;
an external controller must acknowledge MEI. Priority is **MEI > MSI > MTI**.
`mcause[63]` distinguishes interrupts and `mtval` is zero on interrupt entry.
`mtvec` supports direct mode, with low two bits WARL zero. MPP remains M-only.

The core stops issuing RX instructions while an enabled level is pending.
Existing EX/CM owners finish, including acknowledged stores and multiply/divide;
only then can an interrupt enter. An in-flight synchronous exception takes its
normal trap first. Older prediction repairs complete before interrupt entry.
The next architectural PC is tracked at retirement, independently of fetch
queues and cache availability. This preserves taken targets and 16-bit alignment
in MEPC, with no replay of an already issued store or retired instruction.
A withdrawn level is rechecked and cannot produce a stale interrupt.

Writes to `mstatus`, `mie` or `mip` serialize younger instructions through a
fallthrough redirect. MRET already serializes through its return redirect.
The resulting CSR state is therefore evaluated before a successor executes.
The handler can re-enable interrupts explicitly for nesting. Entering a trap
saves MIE in MPIE and clears MIE; MRET restores MIE from MPIE and sets MPIE.
`WFI` remains a legal NOP, including while masked.

Latency includes the remaining EX/CM operations, the registered redirect and
instruction-cache access. A memory device that never acknowledges also blocks
interrupt entry; bus timeout/fault handling is a separate platform requirement.
Ordinary instructions with no enabled pending level keep their existing pipeline
schedule. Writes to the three interrupt-control CSRs pay serialization cost.

## Memory-mapped time and software interrupt

The **0x0200_0000–0x0200_ffff** window selects the timer peripheral and never
writes its BRAM alias. Instruction fetch does not access this peripheral.

| Address | Register | Access/reset |
| --- | --- | --- |
| 0x0200_0000 | MSIP | Read/write bit 0, all other bits zero; reset 0 |
| 0x0200_4000 | MTIMECMP | Read/write 64 bits; reset 0xffff_ffff_ffff_ffff |
| 0x0200_bff0 | Timebase frequency | Read-only 64-bit value **62,400,000 Hz** |
| 0x0200_bff8 | MTIME | Read/write 64 bits; reset 0, increments each memory edge |

The target clocks are CPU **124.8 MHz**, memory/timebase **62.4 MHz**, JTCK **6 MHz**.
A write to MTIME takes priority over its increment on that edge. Comparison uses
unsigned 64-bit values, and MTIME wraps modulo 2^64. MTIP is a registered
`mtime >= mtimecmp` comparison followed by the CPU-domain level register.
Changes become visible eventually; handlers must tolerate a short pending tail
and a repeated MTI after increasing MTIMECMP and returning.

Aligned 64-bit loads/stores access MTIME/MTIMECMP atomically. Byte enables are
little-endian. A smaller access reads/writes the addressed lanes; reserved bits
of MSIP ignore writes. A transfer crossing an eight-byte register slot reads
zero / ignores writes for lanes beyond that slot. Software should use naturally
aligned accesses. Reserved slots read zero and ignore writes. Frequency-register
writes are ignored. Held requests perform each MMIO write once, using the same
request/response token ownership as RAM.

Holding the CPU in reset (`run=0`, PLL loss or system reset) resets the timer and
MSIP while preserving the debug-loadable RAM. Debug loading while stopped does
not advance time. The board freezes MTIME after capturing a nonzero `tohost`
result, alongside its performance counters. Native CPU-only runs use `run` to
reset time and do not contain this test-result freeze.

## Boot memory and current limits

The JTAG loader installs a reset trampoline below 0x1000 and the ELF starting
at 0x8000_0000. Main RAM is **0x8000_0000–0x8003_efff (252 KiB)**; the last 4 KiB
of the 256 KiB BRAM holds the reset region. Test stacks must remain inside main
RAM. There are no required boot arguments or UART protocol yet; the self-check
ELFs report via the loader-selected `tohost` address.

This is an **M-mode platform**, without A, S/U privilege, PMP or virtual memory.
Internal PC/EPC/address truncation and out-of-range aliases outside the timer
window remain. Strict RAM/MMIO bounds and access faults are the next step,
before exposing memory protection or Sv39. This map does not claim complete
CLINT/ACLINT, PLIC, SBI, OpenSBI or Linux compatibility. A CSR `time` alias is not
implemented; software reads MMIO MTIME and the frequency register.

## Strict physical access configuration

`HardwareCompactAccessFaultTestTop` additionally selects `STRICT_MEMORY=1`.
The qualified interrupt image described above keeps the original setting until
the new image completes its own timing and hardware qualification.

| Architectural range (inclusive) | Data | Fetch |
| --- | --- | --- |
| 0x0000_0000–0x0000_0fff | Read/write reset RAM | Execute |
| 0x8000_0000–0x8003_efff | Read/write main RAM | Execute |
| 0x0200_0000–0x0200_ffff | Timer, naturally aligned transfers | Access fault |
| Everything else, including any nonzero upper 32 bits | Access fault | Access fault |

This is a platform access-type/bounds check, not PMP or page translation. Every
byte of a RAM transfer must fit in its region; otherwise no memory request is
issued and no part of a store is written. Unaligned RAM transfers within a region
remain supported. Unaligned timer transfers raise access faults (5 for loads,
7 for stores); a handler must not emulate them as several peripheral accesses.
Reserved aligned timer slots retain their zero/ignored behavior.

Strict mode keeps architectural PC, control targets, MEPC and MTVAL at 64 bits.
MTVEC retains all upper bits and forces direct mode; MEPC clears only bit zero.
Access faults report causes 1/5/7. MTVAL is the original inaccessible address,
or the first inaccessible byte when a transfer crosses a region boundary.
For a 32-bit instruction at 0x8003_effe, MEPC is 0x8003_effe and MTVAL is
0x8003_f000; a 16-bit instruction there can retire before the next fetch faults.
A jump to an inaccessible target still retires its link; the fetch at that
target owns the exception. Invalid fetched bytes cannot become illegal-opcode
exceptions, execute MMIO or override an older exception/branch repair.

Fetch faults travel with their queued instruction, so wrong-path prefetches do
not trap. The instruction-memory interface returns an inert line for an unmapped
32-bit request, allowing boundary fetches to complete. The core performs the
64-bit check before accepting the instruction. Data validation precedes the
request token, and CM drains an older request before taking the exception.
CM fault recovery discards speculative predictor entries and younger WB state.
The memory backend still receives relocated physical BRAM addresses; its private
ports and the stopped debug loader are not architectural CPU accesses.

Predictor tags/targets and physical memory ports remain 32 bits because permitted
execution is in the physical regions above. Sv39 will require translation and a
new predictor tag/target policy in addition to this full-width trap state.
The optional configuration and its evidence are tracked in the
[access-fault measurement directory](../toolchain/measurements/2026-09-10-rv64-access-faults/).
These exception-value and unsupported unaligned-MMIO choices follow
[Machine ISA 1.13](https://docs.riscv.org/reference/isa/v20260120/priv/machine.html).

## Reproduce functional tests

Build the two self-checking programs with:

```sh
RISCV_CLANG=toolchain/worktrees/isa/clang-20-package/extracted/usr/lib/llvm-20/bin/clang \
  python3 riscv/tools/build_rv64_machine_interrupts.py
```

Use the usual hardware-memory model flags and add `--machine-interrupts`.
The `machine_interrupts.elf` test checks masks, read-only MIP levels, MSI/MTI
priority, precise MEPC and immediate re-entry after MRET. The
`timer_context_switch.elf` test switches two tasks 16 times, saving/restoring
all x1–x31 registers and MEPC, and checks each task's register values and progress.
The execution time of this second test is timer driven, so its instruction count
is not a fixed-work performance benchmark.
Both programs pass the serialized clock orders, coincident-edge simulation and
three physical runs. On hardware, the context-switch program retires 21,393
instructions in 100,290–100,291 CPU cycles; the timed model gives 100,291 cycles.

The native/mapped timer oracle covers 10,248 stimulus edges per backend:
byte lanes/masks, reserved slots, reset, freeze, wraparound and pending latency.
CPU tests also inject MEI/MSI/MTI directly, including 180 arrival placements across
compact, TAGE and static prediction, delayed memory, division, taken branches,
compressed instructions, withdrawn requests and synchronous exception priority.

Architectural rules were checked against
[Machine ISA 1.13](https://docs.riscv.org/reference/isa/priv/machine.html),
particularly machine interrupt CSRs, trap/return state and memory-mapped timers.
