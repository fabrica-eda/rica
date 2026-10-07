# Microarchitecture

Rica is a single-issue, in-order RV64 core. Throughput is measured as work per
unit time (IPC × clock), so features are only adopted when they close routed
timing at the target clock **and** do not cost more cycles than they save.

The RTL is in `riscv/src/`. `Core.veryl` holds the pipeline; the board wrappers
(`HardwareTestTop.veryl`, `HardwareCompactTestTop.veryl`) add memory, PLL, debug
transport and peripherals.

## Pipeline

```text
fetch ─► FD ─► RX ─► EX ─► CM ─► WB
(I-cache,   (decode)  (register  (EX1)  (completion /   (register
 predictor)            read)              EX2, traps)     write)
```

| Stage | Work |
| --- | --- |
| Fetch | I-cache lookup, branch prediction, compressed-instruction expansion, a three-slot fetch FIFO |
| FD | Decode; with Sv39, instruction translation checks |
| RX | Synchronous register-file read, privilege and PMP fetch checks |
| EX | ALU, address generation, branch resolution, data permission checks |
| CM | Common completion stage: load data, multi-cycle results, CSR effects, traps, prediction repair, Sv39 data translation |
| WB | Register write and retirement |

Key properties:

- **One instruction per clock** for dependent ALU chains: the ALU result is
  captured in CM and forwarded from that register, so there is no ALU-to-ALU
  combinational loop.
- Loads, and with `EARLY_ALU=0` shifts and comparisons, stall a dependent
  consumer once. `EARLY_BITMANIP` forwards simple Zbb/Zbs results (ANDN, ORN,
  XNOR, SEXT/ZEXT, ORC.B, REV8, BCLR/BINV/BSET) from EX.
- Data-dependent flushes are registered: a branch decision sets a pending
  redirect, and the next cycle squashes younger stages. An older pending
  redirect blocks younger ones.
- Each CSR instruction runs with an empty EX predecessor, which keeps counter
  carries and CSR ordering off the ALU path. Writes to interrupt-control,
  privilege, PMP and translation CSRs serialize younger instructions.
- Memory waits follow response tokens rather than fixed latencies.
- Architectural PC, branch targets, `xepc` and `xtval` are 64 bits. Physical
  addresses are 32 bits.

## Front end

| Block | Configuration in the working profile |
| --- | --- |
| Instruction cache | 4 KiB, direct mapped, 64-bit lines in even/odd banks (`InstructionCache.veryl`) |
| Branch predictor | Compact: 64-entry direct-mapped BTB, 2-bit direction counter per entry, 8-entry return-address stack (`CompactBranchPredictor.veryl`) |
| Compressed instructions | Zca expanded to canonical 32-bit instructions (`CompressedDecoder.veryl`) |

The I-cache reads the current and next line together, so sequential code,
including a 32-bit instruction that crosses a line, sustains one instruction per
clock. It keeps one tagged refill in flight, prefetches up to PC+16, stores
compressed-length predecode bits with each line, and scrubs every valid bit on
reset and `FENCE.I`. Read/write collisions are masked rather than relying on the
FPGA's read-during-write behavior.

With Sv39 the PC stays virtual. The I-cache compares tags against both the PC
and the registered physical frame from the ITLB, and selects late, so translation
adds no multiplexer in front of the tag compare. See
[privileged.md](privileged.md#sv39).

The compact predictor reads the BTB synchronously in two stages and accepts a
new request every clock. Eight prediction tokens carry prediction-time metadata
until retirement, where training happens. The RAS keeps speculative and
committed copies, follows the x1/x5 call/return/coroutine hints, and handles
compressed `PC+2` links. Mispredictions are repaired in CM: EX registers the
equality comparisons (in 16-bit chunks) and CM selects the result, which keeps
the JALR adder off the recovery path. `EARLY_TAKEN_FETCH` presents a held taken
target to the I-cache while the back end is waiting.

The recovery scheme restores the committed history/RAS frontier. That is only
valid for a scalar in-order core; a wider or out-of-order core would need a
per-branch checkpoint.

Other predictor configurations remain available for comparison: a TAGE predictor
(128-set two-way BTB, 2048-entry bimodal plus four 512-entry tagged tables with
4/12/36/108-bit histories, 16-entry RAS), a 2048-entry bimodal predictor, a
static not-taken predictor, and compact variants with a 32-entry BTB or without
the RAS or direction prediction. The runners select them with
`--compact-predictor`, `--compact-predictor-32`, `--compact-predictor-no-ras`,
`--compact-predictor-no-direction`, `--bimodal` and `--static-predictor`; no flag
selects TAGE.

## Execution units

| Unit | Implementation |
| --- | --- |
| ALU (`ExecuteUnit.veryl`) | 64-bit add as four 16-bit carry-select slices; Zba, Zbb and Zbs |
| Multiplier (`DspMultiply64.veryl`) | 64×64 product from sixteen 16×16 DSP partial products, seven external register stages, no DSP-internal registers |
| Divider (`MulDivUnit.veryl`) | Restoring division with early termination; worst case 67 cycles (64-bit) and 35 cycles (W forms) |
| Atomics (`AtomicLsu.veryl`, `AtomicAlu.veryl`) | Owns CM for read, registered arithmetic and write; see [privileged.md](privileged.md#a-extension) |

An M instruction holds younger instructions until it completes; an older
redirect or exception cancels it, and its result is never forwarded early.

## Load/store unit and memory

The board memory (`HardwareMemory.veryl`) is a 256 KiB true-dual-port BRAM
running at half the CPU clock with zero relative phase: port A serves 64-bit
instruction lines and debug writes, port B serves data. With `FAST_LSU=6` the
held load request is accepted on the BRAM read edge and data and response token
arrive together in a CPU-side register; stores write from a registered bundle
once and acknowledge on the same edge. Loads are width- and sign-formatted
(`LoadFormat.veryl`) before that register, so returned data can be forwarded
without another selection.

Misaligned loads and stores to RAM are handled in hardware unless they cross a
page under Sv39 (which raises address-misaligned). Every byte is permission
checked before a request issues, so a faulting store writes nothing.

## Performance counters

`mcycle` and `minstret` are writable; `cycle` and `instret` are read-only
aliases. Custom CSRs `0x7C0` (selector) and `0x7C1` (64-bit value) expose nine
implementation events:

| Selector | Event |
| ---: | --- |
| 0 | Dependency bubble (includes M-unit waits) |
| 1 | Data-memory wait |
| 2 | Redirect |
| 3 | Empty front end |
| 4 | Retired conditional branch |
| 5 | Wrong conditional direction |
| 6 | Retired control transfer with a BTB miss |
| 7 | Return that fell back to the BTB because the RAS was empty |
| 8 | Next-PC prediction repair |

Events 0–3 are mutually exclusive per cycle (priority: memory, redirect,
dependency, front end); 4–8 are independent. The hardware runner reads all
eleven counters after each test and prints them as `METRIC` lines.

## Build-time parameters

The board top selects features through `HardwareTestTop` parameters:

| Parameter | Working profile | Meaning |
| --- | --- | --- |
| `ENABLE_M`, `M_USE_DSP`, `EARLY_DIVIDE` | 1, 1, 1 | M extension, DSP multiplier, early-terminating divider |
| `ENABLE_ZBA`, `ENABLE_ZBB`, `ENABLE_ZBS` | 1, 1, 1 | B extension subsets (`misa.B` when all three are set) |
| `ENABLE_A` | 1 | Zaamo and Zalrsc |
| `ENABLE_U`, `ENABLE_S`, `ENABLE_SV39` | 1, 1, 1 | Privilege levels and translation |
| `ENABLE_INTERRUPTS`, `ENABLE_CONSOLE`, `STRICT_MEMORY` | 1, 1, 1 | Machine interrupts, JTAG console, strict physical access checks |
| `PREDICTOR_MODE`, `ICACHE_INDEX_BITS` | 4, 8 | Compact predictor, 4 KiB I-cache |
| `FAST_LSU` | 6 | LSU response timing (0–6 kept for comparison) |
| `EARLY_BITMANIP`, `EARLY_TAKEN_FETCH` | 1, 1 | Early forwarding and taken-target preview |
| `PMP_DATA_STAGES`, `EARLY_ALU`, `DIRECT_LOAD_RETURN`, `MEMORY_HZ` | from `platform-user.json` | Clock-profile-dependent choices |
