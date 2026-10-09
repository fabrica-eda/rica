# Privileged architecture

Rica follows the RISC-V Privileged ISA 1.13. This page lists what is
implemented and the choices made where the specification leaves room. Each
feature is a build parameter (`ENABLE_INTERRUPTS`, `ENABLE_U`, `ENABLE_S`,
`ENABLE_SV39`, `ENABLE_A`); the working profile enables all of them.

## Machine mode

| Feature | Implementation |
| --- | --- |
| Traps | ECALL, EBREAK, illegal instruction, misaligned and access faults, page faults; `mcause[63]` marks interrupts |
| `mtvec` | Direct mode only (low two bits WARL zero); upper bits retained |
| `mepc`, `mtval` | 64 bits; `mepc` clears bit 0; `mtval` is the faulting address, or the first inaccessible byte when an access crosses a region |
| Interrupts | MEI, MSI, MTI (and SEI/SSI/STI with S); priority MEI > MSI > MTI > SEI > SSI > STI, M-destined first |
| `mie` / `mip` | Bits for implemented sources only; MEIP/MSIP/MTIP are read-only source levels |
| `WFI` | Legal; executes as a no-op |
| `misa` | Fixed value reflecting the build; writes are legal and ignored |
| Counters | `mcycle`, `minstret` (writable), `cycle`/`instret` aliases, custom event counters at `0x7C0`/`0x7C1` ([architecture.md](architecture.md#performance-counters)) |

There is no `time` CSR; software reads `mtime` over MMIO
([platform.md](platform.md)). Entering a trap saves MIE in MPIE and clears MIE;
MRET restores it.

Interrupts are precise. An enabled pending interrupt stops issue at RX, lets
older EX/CM work finish (including acknowledged stores and multiply/divide),
and only then enters the trap, so no store or retired instruction is replayed.
A level withdrawn before acceptance does not produce a stale trap.

## User mode and PMP

- `misa.U`, UXL fixed at 64. MPP accepts M, S (with S) or U; other values become U.
  MRET to S/U clears MPRV.
- Machine CSRs, MRET, SRET and WFI are illegal in U. U ECALL has cause 8.
- Machine interrupts stay enabled while executing below M, regardless of
  `mstatus.MIE`; `mie` still masks individual sources.
- MPRV applies to loads, stores and atomics, not to instruction fetch.
- `mcounteren` gates the `cycle`/`instret` aliases; other bits are WARL zero.
  `menvcfg` is read-only zero.

PMP: 16 architectural entries, of which entries 0–3 are programmable and 4–15
are permanently OFF. `pmpcfg0`, `pmpcfg2` and `pmpaddr0`–`pmpaddr15` exist; odd
`pmpcfg` CSRs are illegal. Physical addresses are 32 bits and the grain is four
bytes. OFF, TOR, NA4 and NAPOT, R/W/X, lowest-index priority, full-access
containment and reset-only locking (a locked TOR also locks the previous address
register) are implemented. A reserved R=0/W=1 write clears W. Unmatched S/U
accesses fail; unmatched M accesses pass PMP but still need a valid platform
region. Smepmp is not implemented.

Permission checks happen before any memory request, including for atomics, and
cover every byte. A 32-bit instruction crossing a PMP boundary checks both
parcels. PMP CSR writes serialize execution.

Two one-entry **permission caches** keep the PMP comparators off the per-access
path: one for fetch and one for data, each holding the R/W/X result for one 4 KiB
page whose permissions are uniform. A page split by a PMP boundary always uses
the exact two-stage check. A data hit shortens the permission wait from two
cycles to one; a fetch miss holds RX for one extra cycle. Both entries are
invalidated while a PMP CSR write is in CM. The checkers and both caches are in
`PmpUnit.veryl`.

## Supervisor mode

| Item | Implementation |
| --- | --- |
| Returns | `MRET` to MPP (S or U), `SRET` to SPP |
| Delegation | `medeleg` bits 0–9, 12, 13, 15 (bit 11, M-mode ECALL, is zero); `mideleg` for SSI/STI/SEI |
| Delegated traps | Taken in S via `stvec` (direct mode), updating `sepc`, `scause`, `stval`, SPP/SPIE/SIE. Traps in M are never delegated |
| S CSRs | `sstatus` (SIE/SPIE/SPP/SUM/MXR/UXL), `sie`/`sip` (delegated bits only), `stvec`, `sscratch`, `sepc`, `scause`, `stval`, `scounteren`, `senvcfg` (zero), `satp` |
| `mstatus` | SIE/SPIE/SPP/MXR/SUM/TVM/TW/TSR; SXL = UXL = 64; MPP value 10 becomes 00 |
| S interrupt pending bits | Software-owned: M writes SEIP/STIP/SSIP in `mip`, S may write only a delegated SSIP in `sip` |
| Counters | S reads `cycle`/`instret` when `mcounteren` allows; U when `mcounteren & scounteren` allows |
| Instructions | `SRET` illegal in U and in S with TSR; `SFENCE.VMA` illegal in U and in S with TVM; `WFI` illegal in U and in S with TW |

There is no hardware source for S-level interrupts. The intended use is
SBI-style: M-mode software takes the machine timer interrupt and sets
`mip.STIP` for the S-mode kernel.

PMP does not distinguish S from U, so an S-mode kernel cannot protect itself
from U tasks with PMP alone; isolation comes from Sv39 page permissions.

Misaligned atomics report misaligned exceptions (cause 4 for LR, 6 otherwise)
once the PMP check passes. Misaligned ordinary loads and stores are handled in
hardware and do not trap.

## Sv39

| Item | Choice |
| --- | --- |
| `satp.MODE` | Bare (0) and Sv39 (8). Writes of other modes are ignored entirely |
| ASID | Width 0; `satp` writes and `SFENCE.VMA` flush all TLB entries |
| `satp.PPN` | 20 bits (fits the 32-bit physical space); upper bits WARL zero |
| A/D bits | Svade: A=0, or D=0 on a store/AMO, raises a page fault; hardware never updates PTEs |
| Page sizes | 4 KiB, 2 MiB, 1 GiB; a misaligned superpage raises a page fault |
| Physical address | 32 bits. A PTE whose PPN exceeds 32 bits raises an access fault (never aliased) |
| Reserved PTE bits | Nonzero N/PBMT/reserved bits (63:54) raise a page fault |
| Canonical VAs | Bits 63:39 must equal bit 38, otherwise page fault |
| SUM, MXR | Implemented |
| MPRV | With MPP = S/U, M-mode loads/stores/atomics are translated (fetch is not) |
| Page crossing | Instructions: the second page is translated separately. Data: an access that crosses a page raises address-misaligned |
| `xtval` on faults | The faulting VA; for the second parcel of an instruction, that parcel's VA |

### TLBs and the page-table walker

The TLBs and the translation state machine are in `Sv39Translation.veryl`.

- **DTLB:** one entry. Superpages are cached at 4 KiB granularity.
- **ITLB:** two entries (page, page − 1, frame, permissions) with round-robin
  replacement, so both pages of a page-crossing instruction always coexist.
- **Walker** (`PageTableWalker.veryl`): reads up to three PTEs through the data
  port. Each PTE read is checked against PMA and PMP; a denied PTE read raises an
  access fault (1 for fetch, 5 for load, 7 for store/AMO).

A TLB miss does not issue a memory request. The instruction travels to CM like
an exception. There it is the oldest instruction and the data port is free, so CM
walks the page table, fills the TLB, and redirects to the same PC to replay the
instruction. The walker therefore stays off every fast path.

Translated loads, stores and atomics are handled in CM:
`Idle → (Walk) → Verdict → Settle → Decide → Issue → Done`. Idle checks the
canonical form, page crossing and DTLB hit; Verdict/Settle/Decide check PMA and
PMP on the physical address using registered checker inputs; Issue sends the
request. Atomics start the atomic unit with the physical address after Done.

Instruction translation: the ITLB lookup is registered, and a fetch is valid
only after three consistent cycles of page and ITLB state, because the I-cache
returns line pairs sampled two cycles earlier. Each fetch packet carries its
physical frame; FD repeats the lookup with the instruction's VA to record
non-canonical, miss, and X/U faults, and RX checks PMP and PMA on the frame.
Fault priority within an instruction: first-parcel page fault, first-parcel
access fault, second-parcel page fault, second-parcel access fault.

`satp` writes and `SFENCE.VMA` flush both TLBs and refetch younger instructions
through the same serialization path as `FENCE.I`. The I-cache is physically
tagged and is not flushed by `SFENCE.VMA`. Changes of privilege, SUM or MXR do
not need a flush, because permissions are re-evaluated from the current state.

The small, 4 KiB-granular TLBs are the main cost of Sv39: the S-mode test kernel
maps itself with a 1 GiB identity page and takes about 2.2 times the cycles of the
equivalent M-mode kernel. A TLB that holds a superpage in one entry is the main
remaining improvement.

## A extension

`ENABLE_A` adds Zaamo and Zalrsc (all nine AMOs and LR/SC, W and D, every aq/rl
encoding; LR requires rs2 = x0).

- Atomics are allowed only in main RAM (`0x8000_0000`–`0x8003_efff`); the reset
  region and MMIO raise access faults (5/7).
- Natural alignment is required. Misalignment reports cause 4 for LR and 6
  otherwise. Checks also apply to an SC that has no reservation.
- A reservation covers exactly the naturally aligned word or doubleword of the
  latest LR. Every SC consumes it, and any ordinary store, AMO, trap entry, MRET
  or reset clears it.
- The core holds the back end until an atomic completes, so all memory and MMIO
  operations are ordered for every aq/rl combination; `FENCE` is an ordered
  no-op on this single-hart system. Interrupts see an AMO either before or after,
  never between its read and write.
- JTAG debug writes are rejected while the CPU runs, and nothing else writes RAM,
  so there is no multi-master reservation support.
