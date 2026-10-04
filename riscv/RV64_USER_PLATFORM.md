# U-mode and PMP platform

The 84/84 MHz implementation passes measured STA and functional simulation as
of 2026-10-03. Worst setup is +26 ps and worst hold is +1 ps after hold repair,
with the same measured library, 20% thermal margin, 194 ps guards and four
reviewed exceptions. All 22,601 modeled setup/hold checks outside those four
boundaries pass; route ownership is conflict-free. The supplemental JTCK audit
matches all 515 setup and 515 hold checks. On 2026-10-04 the bitstream passed
all 268 validation ELFs on the FPGA, including the U kernel; see
[hardware execution](#hardware-execution-2026-10-04).

The source, exact 588 MHz VCO PLL, timer test ELFs, checkpoint and validation
records are saved in `toolchain/measurements/2026-10-03-rv64-u-84`.
The bitstream is `riscv/build/riscv-hardware-compact-user-84-20261003.bit`, SHA-256
`d4de74dd8d191bbd3a8b066c4286c803fa87a394e6541ec5ff3216c6ba5489c1`.
It passes 536 native runs, the simultaneous U kernel and 31 simultaneous memory
tests. The clock is 7.7% higher than the preserved 78 MHz reference; 258 cases
with identical ELFs and retired work take a median 7.1% and weighted 6.9% less
simulated time. Recovery toward 124.8 MHz remains incomplete: this 84 MHz
implementation still takes a median 26.3% more time than the simulated 124.8 MHz
U candidate, which itself failed STA. These are not hardware measurements.

The 84 MHz implementation uses the earlier parallel PMP offset/endpoint RTL.
Its clock change altered timer-readback LUT contents; restoring the corresponding
59 LUT placements, including six complete wide-mux groups, allowed all 45,708
ordinary signal trees to be reimported with current endpoint checks. New LUT
contents and fresh STA were retained. Initial signal routing then took 0.56 s;
the complete PNR took 395.29 s, including synthesis and hold repair. This is one
recorded run, not a general router performance benchmark.

### Hardware execution (2026-10-04)

The 84 MHz bitstream above was loaded into SRAM on the LFE5UM5G-85F-EVN
(IDCODE `0x81113043`) with [tapir](https://github.com/fabrica-eda/tapir),
using FT2232H interface A forwarded into WSL by usbipd-win. `tapir xvc` served
the existing runner at a 167 ns JTCK period. All 268 ELFs from the 84 MHz native
validation were first checked against their recorded SHA-256; **268/268 pass on
hardware** in 340 s. This is the first FPGA execution of a U/PMP image.

- The U kernel reports mcycle 209,579 and minstret 66,924, identical to the
  simultaneous-clock model.
- All 32 ELFs with simultaneous-clock records (the U kernel and the 31
  memory/atomic/PMP cases) match both mcycle and minstret exactly.
- 258 ELFs match native retired-instruction counts: the 136 ACT4 ELFs equal the
  native commit count, and the 122 project ELFs equal it plus one, the same
  offset as the U kernel's simultaneous record. The other ten depend on timer
  interrupts (the kernels, `timer_context_switch`, `atomic-interrupts`) or write
  the counters themselves (`system`, `performance`); their self-checks pass.
- Native-model cycle counts are not compared: that model does not reproduce
  same-rate clock timing. The other 236 ELFs therefore have no cycle reference.
- The interactive console kernel was not part of the 84 MHz validation set and
  was not run.

A preliminary U-kernel run that used openFPGALoader 0.12.0 for programming and
XVC produced the same metrics; the recorded suite uses tapir only. Logs, the
ELF list and the comparison script are in
`toolchain/measurements/2026-10-03-rv64-u-84/hardware-2026-10-04/`. To
reproduce from the repository root:

```sh
tapir program-sram riscv/build/riscv-hardware-compact-user-84-20261003.bit
tapir xvc --listen 127.0.0.1:3721
# In another terminal:
xargs -d '\n' -a toolchain/measurements/2026-10-03-rv64-u-84/hardware-2026-10-04/elfs.txt \
  target/release/hardware-riscv-tests --xvc 127.0.0.1:3721 --no-program
```

The current working profile prioritizes the CPU at 104 MHz with 52 MHz memory
and is not yet timing qualified. PMP bounds retain their nonempty predicate
in the existing two-stage update; each access no longer repeats that 33-bit
comparison. The RAS uses one-hot pointers and caches both stack tops on the same
edge as their contents, including the current retirement during restore. CSR
write gating uses raw pending work behind the unchanged final discard check.
These changes preserve all 268 ELF cycle/commit records in both native clock
orders at 104/52 MHz. The 536 native runs, simultaneous U kernel, 31 simultaneous
memory cases and 61 predictor-recovery tests pass. Independent native/mapped RAS
checks cover 65,536 cycles, including six wraps of one stack while preserving
the other stack's snapshot. The working source replaces bulk RAS recovery copies with eight
sharing flags. Recovery selects the updated committed entries. Before retirement
later overwrites a shared entry, its old value is saved in the speculative
register, preserving the speculative snapshot. In the mapped base graph,
retire-valid reaches zero speculative data pins instead of 256; it now reaches
their clock-enable pins. This is a connectivity check, not a measured timing gain.
Compressed instruction legality and opcode bits now decode directly from the
instruction families, avoiding the full instruction-selection tree for these
outputs. All 65,536 compressed inputs match the original output/legality oracle
in both native and mapped RTL, and the complete CPU validation above remains
passing with identical cycle counts. System-event guards now use raw pending
work behind the unchanged redirect-discard check. The compact predictor receives
readiness directly and qualifies it with response-valid internally; the core's
accepted event retains the same valid-and-ready condition. All 41 working source
files match the source tested by the 536 native runs, simultaneous U kernel,
31 memory cases and 61 recovery tests, with unchanged cycle/commit records.

The physical-access boundary check now uses width-specific byte-lane comparisons
instead of decoding the byte extent and adding it through four carry bits. All
32 lane/width predicates match the old addition. An independent per-byte region
oracle passes 206,848 native/mapped checks, including timer on/off, instruction
and data access, region boundaries and high-address aliases. Fresh 536 native
runs, the simultaneous U kernel, 31 memory cases and 61 recovery tests pass;
all 268 ELF cycle/commit records remain identical in both native edge orders.
The mapped base has 42,177 cells and 45,676 nets. A port-specific diagnostic that
excludes truth-table-independent LUT inputs reduces the fetch queue valid path
from 13 to 11 LUT/mux steps; the longest remaining data cone is the cache's
`proposed_request_q` at 13 steps. These are logic depths, not routed delays.

That inspection also found 16 routed inputs on 12 constant-output LUTs. Struo's
cleanup checked the unrestricted INIT, missing inputs made irrelevant by other
pins tied to constants. [Struo #156](https://github.com/fabrica-eda/struo/pull/156)
restricts the truth table before checking input independence, while retaining
the original INIT. The previous implementation fails the new majority-LUT
regression; the fix passes every INIT with two constant-binding patterns, all
88 public ECP5 tests, strict Clippy and formatting. The compatible private tool
backport passes 91 ECP5 tests and 62 CLI tests, with unchanged common dependency
versions. Public CI passes. On the new byte-lane RTL, all 42,177 mapped cells
retain their function and 15 remaining dead inputs are removed from 11 LUTs;
no truth-table-independent dynamic LUT inputs remain in that graph.
Neither running branch candidate has been changed
to use this new mapper or the byte-lane RTL while routing is in progress.

The cache successor comparison now subtracts one row from the registered even
bank address and eight bytes from the held request address, comparing their
predecessors against the current PC. This removes a PC-mux/increment/comparison
chain without changing state or request cycles. A QF_BV proof establishes all
four changed predicates for every input state at index widths 7, 8 and 9,
including wraparound. Twelve native/mapped cache tests pass. Fresh 536 native
CPU runs, the simultaneous U kernel and 31 memory cases pass; all 268 ELF cycle
records match the byte-lane candidate. The Core-only 61-test evidence is reused
after proving its actual 28 input source files are byte-identical; that harness
does not read InstructionCache. Mapping at a 104 MHz synthesis goal produces
42,327 cells and 45,891 nets, with unchanged 10,768 FFs. Maximum data logic depth
falls from 13 to 12. Fresh placement, routing and STA are still required for
these working-source changes.

The corresponding physical candidate is
`.agent-artifacts/cpu-priority-20261003/cache-predecessor-branches-104-52`.
It uses the frozen constant-support mapper, a 104 MHz synthesis goal, 66
same-cycle FF copies and 60 ordinary LUT copies. All 42,416 pre-copy primitives
preserve their metadata and connections after resolving the identical LUT
aliases; the final graph has 42,476 cells and 46,040 nets. Its source-bound
preflight passes. Placement/routing is running from the completed opcode/COW
checkpoint, retaining the three prescribed clock-buffer sites. That reference
has legal routing but failing timing; no timing evidence is inherited. The
104/52 MHz profile remains unqualified until fresh setup and hold both pass.

The opcode/COW candidate completed legal routing but failed measured STA at
setup −8,489 ps and hold −15 ps. A subsequent setup-search trial accepted no
improvement and finished at −8,489/−452 ps; it was not adopted. A subsequent
preparation audit found that its reuse script had omitted the three
packer-inserted clock buffers from placement hints, changing clock insertion
delays. The script now preserves and verifies these BELs explicitly. This
placement error invalidates the hold comparison; it did not change clocks or
the measured timing model. Both current branch candidates retain all three
prescribed buffer locations and will check them again in the final checkpoint.
A running physical candidate splits four high-fanout predictor controls using 60 identical LUT
copies, with no new registers or stages. Independent graph comparison preserves
all 42,278 original primitive functions and verifies every copy; actual copied
outputs drive at most 32 data pins. Mapping equivalence passes, but routed
timing is still pending. The tool changes are in
[Struo #155](https://github.com/fabrica-eda/struo/pull/155) and
[Texo #99](https://github.com/fabrica-eda/texo/pull/99).
Functional validation does not establish timing closure. A distributed-RAM
experiment was not adopted because the measured STA table does not cover its
write paths; the working design uses ordinary registers.

The preceding 104/104 MHz development established the following pipeline changes.
It uses two data-permission stages, registered shift/compare completion, balanced
console FIFO reads, and parallel PMP end-byte address generation. The two-stage
data path adds the byte extent to the signed immediate before the forwarded
base arrives, so endpoint generation runs alongside effective-address generation.
The upper-address predicate also decodes the adjacent 4 KiB boundaries in
parallel, avoiding a reduction after the full 64-bit effective-address sum.
In the two-stage variant, address inputs refresh while the PMP stage is idle;
the unchanged stage transitions still control when permission checking may
consume them. This removes backend-wait and redirect logic from their clock
enables. The three-stage variant keeps its original capture conditions.
Instruction fetch computes its halfword and word endpoints before selecting
the decoded instruction length. These endpoint changes add no pipeline wait cycles.
Native/mapped tests cover 403,288 signed-offset, boundary and wrapping cases,
plus 24,764 instruction-fetch endpoint cases. Capture equivalence checks compare
65,536 cycles across the two-stage and three-stage variants; all 536 native
cycle/commit records remain identical to the previous 104 MHz RTL. The
simultaneous U kernel and 31 simultaneous memory/atomic/PMP cases also pass.
Physical timing remains failing: the best saved 104/104 MHz candidate has setup
−2,241 ps and hold −15 ps. Grouping clock-enable branches by actual register
placement and shared CE wires reduces setup violations from 1,221 to 834 and
total negative slack from −725,306 ps to −400,121 ps, but worst setup slack
regresses from −3,144 ps to −3,314 ps. Subsequent routing repair reaches
−3,068 ps, still failing. Functional equivalence does not establish a timing gain.
Adding complete, same-cycle copies of 32 PC registers and one CSR-index register
near the observed slow PMP comparison/decode consumers reaches −2,241 ps.
The next critical path starts at the return-stack pointer. These comparisons
use the same measured library, 194 ps guards and four timing exceptions.
The placement-based clock-enable grouping support is proposed in
[Struo #118](https://github.com/fabrica-eda/struo/pull/118) and
[Texo #96](https://github.com/fabrica-eda/texo/pull/96).
The qualified 84 MHz archive contains its own matching RTL and profile; the
104 MHz development sources remain separate. The exact 84 MHz PLL template uses
a 588 MHz VCO and zero CPHASE/FPHASE on both fabric outputs. An earlier
nonzero-phase template was rejected before placement; the timing model was not
relaxed to accept it. The saved 78 MHz source, checkpoint and bitstream are
unchanged.

The optional `ENABLE_U=1` platform adds U-mode to the existing M-mode hart.
The board target is `HardwareCompactUserTestTop`; the model uses `--u-mode`
with `--strict-memory`. S-mode, delegation and address translation remain later stages.

## Architectural choices

- RV64 U-mode; `misa.U=1`, fixed UXL=64, MPP accepts U or M. Unsupported
  MPP values map to U. Trap entry saves the originating privilege; MRET restores
  it and sets MPP to U. MRET to U clears MPRV.
- Machine CSRs and MRET/WFI are illegal in U. U ECALL has cause 8.
- Machine interrupts remain globally enabled while U executes, regardless of
  MSTATUS.MIE. MIE still masks individual interrupt sources.
- MPRV affects loads/stores/atomics, not instruction fetch. `mcounteren` controls
  the implemented cycle/instret aliases; its other bits are WARL zero.
  `menvcfg` is read-only zero with legal ignored writes.
- Sixteen architectural PMP entries: entries 0–3 programmable, entries 4–15
  permanently OFF/zero. RV64 `pmpcfg0` and `pmpcfg2`, `pmpaddr0`–`pmpaddr15`;
  odd configuration CSRs are illegal. Physical address width is 32 bits,
  address register width 30 bits, grain four bytes.
- OFF, TOR, NA4, NAPOT, R/W/X, lowest-index priority, full-access containment,
  and reset-only locks. Locked TOR also locks the preceding address register.
  Reserved R=0/W=1 writes clear W. Unmatched U accesses fail; unmatched M
  accesses pass PMP and still require valid platform PMA permissions.
- Permission changes serialize execution. Instruction permissions are checked
  even for cache hits, including a 32-bit instruction crossing a PMP boundary.
  Disallowed data requests never issue a memory token; this includes AMO/LR/SC.
- PMP range decoding is registered in two stages, with configuration bits
  carried alongside the bounds. The existing CSR serialization flush keeps
  younger instructions from consuming stale permissions. Instruction permission
  and U-only illegal-instruction predicates are captured at RX→EX, before
  EX frontend control. The current profile uses two held cycles for EX data permissions:
  capture the effective and end-byte addresses together, then register permission.
  The earlier three-cycle experiment captured the end-byte address separately.
  Only the checked owner
  can issue a memory token. CM arithmetic/CSR results and atomic completion
  remain held while this additional wait is active.
- The working source caches fetch and data permissions for one uniform 4 KiB
  page each ([permission cache](RV64_PMP_PERMISSION_CACHE.md)). RX→EX then
  captures a registered fetch result; a fetch miss holds RX for one cycle while
  dedicated endpoint registers are checked. A data hit ends the permission
  wait after one held cycle. Pages split by a PMP boundary keep the exact checks
  above. The 84 MHz qualified image predates this change; the cached RTL closes
  timing at 104/52 and 108/54 MHz and passed 269 ELFs on the FPGA at both.
- End-byte addresses are captured before range comparison: FD→RX captures the
  instruction and first-parcel endpoints with their PC; the first held data
  cycle captures the endpoint alongside the effective address. The carry beyond
  physical bit 31 is retained. RX→EX also captures read/write access kinds and a valid-owner PMP pending
  flag. Permission capture clears that flag; bubbles and flushes clear it with
  EX validity. Stage decoding stays off the global wait path.

## OS exercise

`python3 riscv/kernel/build-user.py` builds `riscv/build/user-kernel/kernel-user.elf`.
Two U tasks share a read/execute code page and each own a separate read/write
stack/data page. Timer interrupts and ECALL yields switch contexts; the M-mode
handler reprograms PMP before returning. Trap frames are saved on a kernel
stack selected through MSCRATCH, never through an untrusted user SP.
The kernel checks illegal CSR/MRET/WFI/counter access, kernel/other-task/MMIO
load/store protection, forbidden atomics, NX, cross-boundary instruction and
store faults, guard values, per-task ticks/yields, and completion.

## Validation status (2026-09-25)

- Native and ECP5-mapped PMP checker: 5,000 cases each, compared with a
  bytewise reference calculation; includes first instruction parcel, entry
  priority, all addressing modes and the top of the physical address space.
- Native/mapped registered-range tests change all four entries every cycle
  for 1,000 cycles and check that bounds and configuration remain coherent.
- Native/mapped CSR tests cover U entry/exit, MPRV clearing, interrupt privilege,
  counter controls, PMP locks and the read-only entries.
- CPU integration `pmp-machine.elf` covers MPRV permissions, M-mode locked
  entries and partial-match failures, including a store immediately after locking
  the region. Both clock evaluation orders pass.
- Existing 266 ISA/OS ELF tests pass both clock orders (532 runs). `system.elf`
  was rebuilt with expectations conditional on `misa.U`, because UXL and MPP
  after MRET legitimately differ. The same diagnostic passes the M-only core.
- The new isolated U-task kernel passes both clock orders and the simultaneous
  clock model. The latter retires 66,053 reported commits in 230,503 cycles.
- After registering PMP ranges, RX permission checks, and two data-permission
  wait cycles, the full 268 ELF set passes both orders (536 runs). The U kernel
  reports 66,053 commits in 219,154/219,153 cycles. This includes the natural
  nested ALU assignment form, with no outer-enable RTL workaround.
- Struo PR [#93](https://github.com/fabrica-eda/struo/pull/93) is merged and fixes nested
  self-hold inference. The original ALU-form regression infers all 64 enables,
  accepts fanout eight, and matches native/mapped simulation for 512 cycles.
  Full-core mapping with the final compiler revision `0ea48de` also passes
  equivalence and produces eight CE branches of eight ALU bits each.
  Its compiler suite passes 204 tests; the existing AXI4 routing check passes
  all ten prescribed seeds at 300 MHz locally.
- Physical timing and FPGA execution of the U image are still pending. The
  qualified M/A image remains the default. No U image has been programmed yet.
  (Superseded: the 84 MHz U image ran on hardware on 2026-10-04; see
  [hardware execution](#hardware-execution-2026-10-04).)
- The current PMP checker shares range comparisons and omits instruction-parcel
  logic in the data instance. Both configurations are exercised together against
  the bytewise oracle (5,000 cases on each backend); the full 536 CPU runs pass.
  This mixed-instance test exposed Celox's value-generic specialization bug,
  reproduced on 0.6.0 and fixed in merged PR
  [#884](https://github.com/celox-sim/celox/pull/884).
  The RTL currently uses a hardware parameter, which passes both backends and
  produces the same physical graph as the value-generic form.

- Isolated Celox 0.6 integration with the specialization fix passes the same
  536 CPU runs with identical cycle/commit counts, plus PMP/CSR and simultaneous
  clock tests. Production dependency pins remain unchanged; this compatibility
  result is recorded in `celox-060-integration-validation.json`.
- The best saved RAS-enabled candidate (`user-95.texo.json`, source19)
  routes legally but fails timing: setup slack −2,796 ps and hold slack −36 ps
  at 124.8/62.4 MHz with the measured library and 194 ps uncertainty. Mapping
  equivalence passes. This is diagnostic evidence, not a qualified image.

The three-stage candidate (`user-31-source`) passes all 536 CPU runs.
Its U kernel reports 66,053 commits in 230,420/230,419 cycles; simultaneous
clocks pass at 239,565 cycles and 66,498 commits. Timing-sensitive interrupt
and polling workloads can retire different instruction counts with the extra
wait cycle; their self-checks pass. The two-stage source30 passes the same suite
with every cycle/commit count unchanged from source19. CSR decode additionally
passes 196,608 address/access/configuration cases, and the relaxed speculative
BTB read enable passes ten predictor/RAS regressions. See
`software-validation30.json` and `software-validation31.json` in the measurement
folder. Neither candidate is physically qualified yet.

`build-user.py` also rebuilds `system.elf` and `pmp-machine.elf`; its manifest
records compiler arguments and source/ELF hashes. Detailed software evidence is
under `toolchain/measurements/2026-09-24-rv64-u/software-validation31.json`.

## Specification

[RISC-V privileged ISA 1.13, Machine-Level ISA](https://docs.riscv.org/reference/isa/v20260120/priv/machine.html),
especially mstatus, counter-enable registers, and Physical Memory Protection
§2.1.7. No Smepmp or supervisor functionality is claimed.

The current RTL (`user-39-source`) computes the effective address upper-half
nonzero predicate alongside the end-byte address, before permission arbitration.
All 536 CPU runs retain exactly the source31 cycle and commit counts, and the
simultaneous U kernel result is unchanged. The mapping also limits the atomic
ALU sum register CE branches to eight loads. Evidence is recorded in
`software-validation39.json`; timing closure remains pending. Instruction fetch
also captures its upper-address predicate with FD→RX. Permission arbitration
uses parallel lowest-entry grants, checked against the bytewise PMP oracle.

The source31 physical refinement `user-34.texo.json` has setup slack −4,440 ps
and hold slack −20 ps. A separate candidate (`user-35`) uses its measured
per-sink delay/slack and placement as synthesis feedback for equivalent local
replication (64 replicas, 359 rewires). Mapping equivalence and the feedback
unit tests pass; this is not a timing qualification. The feedback patch and
source hashes are archived with the measurement evidence.

Physical search now retries a failed timing-weighted local route with ordinary
congestion routing from advisory trees; full measured STA still decides whether
to accept it. With this change, source39 improved from −5,433 ps to −3,909 ps
setup slack (`user-40`), with hold slack −20 ps. A subsequent tool candidate
tries one candidate per critical cell and search radius before the next candidate
for the same pair; its 89 flow tests pass. Both changes are archived as patches
with binary/source hashes. No timing margin or clock constraint was reduced.

`HardwareCompactUserNoRasTestTop` is an optional timing comparison with the
return-address predictor disabled; the original target retains it. All 536
native CPU runs and the simultaneous U OS test pass. The U OS cycle/commit
counts are unchanged, but `call_nested.elf` grows from 16,262 to 19,842 cycles
(about 22%). Mapping contains 773 fewer cells and 1,150 fewer nets. Its best saved
physical result (`user-126`, including ten equivalent LUT replicas) is setup
−1,769 ps and hold −20 ps, still failing at the same frequency and margin;
this is not a qualified replacement for the RAS target.
Evidence: `software-validation45.json`.

The two-stage NoRAS comparison (`user-50-source`) passes 536 CPU runs and
the simultaneous U OS test using the isolated, specialization-fixed Celox 0.6
binary. The U OS retains source19 cycle/commit counts. This does not change
production dependencies or the default RAS configuration. Evidence:
`software-validation50.json`.

Local PnR search changes are submitted as
[Texo PR #78](https://github.com/fabrica-eda/texo/pull/78). The upstream port
passes 479 workspace tests (five existing ignored), strict all-target Clippy,
and formatting. Physical comparisons use the separately recorded backport
binary; passing tool tests does not establish hardware timing closure.

Latest saved comparisons use the unchanged measured STA library, CPU/memory
clocks of 124.8/62.4 MHz, and 194 ps setup/hold uncertainty. All four fail timing;
independent checks of saved wire/PIP ownership find no overlaps.

| PMP data stages | Return predictor | Checkpoint | Setup slack | Hold slack |
| --- | --- | --- | ---: | ---: |
| 2 | Enabled | user-95 | −2,796 ps | −36 ps |
| 3 | Enabled | user-83 | −3,783 ps | −20 ps |
| 2 | Disabled | user-84 | −2,250 ps | −20 ps |
| 3 | Disabled | user-126 | −1,769 ps | −20 ps |

Two additional router fixes are submitted separately:
[64-bit search costs, PR #79](https://github.com/fabrica-eda/texo/pull/79), and
[immutable-route obstacles, PR #80](https://github.com/fabrica-eda/texo/pull/80).
Experimental negotiated ECO extensions remain local. Their focused unit suites
pass, but the modified compiler snapshot has a pre-existing Core250 structural
fixture mismatch (6,278 synthesized nodes versus the pinned expectation 6,228),
reproduced before the latest routing extension. No full-suite pass is claimed
for that modified snapshot.

Explicit PMP capture-predicate candidates preserve the existing stage protocol
while presenting one direct enable per payload register. Source111 (RAS, three
stages) and source112 (NoRAS comparison) each pass all 536 ordered CPU runs with
identical cycle/commit records to source39/source45, plus identical simultaneous
clock metrics. Mapping removes D self-feedback from all 99 PMP capture registers.
For address bit25, the mapped CE cone's registered boundary inputs shrink from
12 to four, eliminating memory-response dependencies. These are structural and
functional results; physical qualification remains pending. Evidence:
`software-validation111.json`, `software-validation112.json`, and per-graph
`user-*-capture-feedback.json`.

The Core-only free-running RF-read candidate source123 passes all 536 ordered
CPU executions and the simultaneous-clock test with records identical to
source113 (two-stage PMP, RAS enabled). RX retains its first stalled response
and folds in WB updates; the generic RegisterFile hold contract is unchanged.
Mapped RF read-data/bypass enables no longer depend on backend completion.
This is not timing qualification; source123 remains an isolated build snapshot.
Evidence: `software-validation123.json`.
The corresponding three-stage RAS candidate source124 also matches all 536
source111 records and simultaneous-clock metrics; physical evaluation is pending.
Evidence: `software-validation124.json`.

Source131 keeps source124 timing/cycle behavior while decoding RAS slot writes
independently of the late transaction-valid input. All 536 ordered CPU records
and simultaneous-clock metrics match source124. The three existing RAS RTL
tests pass, including restoration with concurrent retirement and a 4,096-cycle
architectural stack comparison. The mapped redirect-to-RAS-enable cone shrinks
from six logic levels to three; this is not routed timing qualification. Evidence:
`software-validation131.json`, `user-131-ras-rtl-test.json`, and
`user-131-ras-control-depth.json`.

Source134 replaces the selected-bank counter overflow carry with a local
per-bank all-ones check. All 536 ordered CPU records and simultaneous-clock
metrics match source131. An independent 8,192-cycle four-counter RTL model
also checks low-word and full-width wraparound, freeze, and competing writes.
Source135 specializes a valid CM CSR write's wait condition to the independent
PMP data wait; a CM CSR cannot simultaneously own a memory/muldiv/atomic
operation. Its 536 records and timed metrics also match, and mapping removes
the memory-operation-to-event-counter-enable dependency. Evidence:
`software-validation134.json`, `user-134-counter-rtl-test.json`,
`software-validation135.json`, and `user-135-csr-control-dependency.json`.

Source143 makes the existing RX PMP endpoint capture predicate explicit,
retaining the original ordered stall/flush branches and asynchronous resets.
All 536 ordered executions and simultaneous-clock metrics match source135.
The mapped 67-bit endpoint bank has no D self-feedback; 59 bits have proposed
compatible carry-to-register pairs. This is a functional and structural result,
not timing qualification. Source143 remains isolated from the working RTL and
has not been programmed. Evidence: `software-validation143.json`,
`user-143-rx-pmp-feedback-check.json`, and `user-143-capture-carry-pairs.json`.

An opt-in mapped-LUT composition experiment has 83 passing ECP5 mapper tests,
including exhaustive truth-table cases and small add/subtract boundaries.
Independent graph checks cover the changed LUT functions and absorbed mux
aliases. The initial source135 route with this experiment (candidate139) still
fails setup at -7,385 ps and hold at -20 ps; fewer LUTs alone is not evidence of
improved physical timing. Candidate145 uses the same source143 RTL without
this mapper experiment to isolate the capture change. Sign-off input hashes
for candidates131 through144 match the existing full measured model, clock
periods, and 194 ps setup/hold uncertainties. No new hardware qualification is
claimed by these experiments.


Source146 extends explicit capture to all 326 selected RX payload bits, excluding
valid, stalled, and execution-hazard state. Its 536 ordered executions and timed
metrics match source143. Candidate150 (localized capture enables and a direct
PMP permission LUT/register pair) remains failing at setup -4,665 ps / hold
-20 ps. Independent serialized routing ownership checks pass for149–151.
The candidate148 broad PMP placement experiment was cancelled before STA;
no routability or timing result is claimed.

Source152 replaces the CSR read mux with shared one-hot selection and a balanced
masked OR tree. It adds no pipeline stages or state. All 536 ordered CPU records
and simultaneous-clock metrics match source146. An independent old/new RTL
comparison passes 262,144 read cases across eight configurations, with random
writes, traps, returns, interrupts, counter overflow and freeze. The selector's
2,048 four-state index/configuration combinations also agree with the original
procedural branches. Routing exhausted 256 iterations with one wire conflict, so no STA
result was produced; this source is not adopted or programmed. Evidence: `software-validation152.json`,
`user-152-csr-differential.log`, and `user-152-pnr-input.json`.

The refined LUT-composition candidate144 fails at -4,404/-20 ps versus
-4,305/-20 ps without composition (candidate138). It has not shown a timing
benefit and is disabled in subsequent candidates. Additional mapper unit tests
pass (86 total), but those tests do not constitute routed CPU qualification.


Source155 moves CSR read decoding before the existing EX capture boundary;
the decoded selection and the original index share capture/reset/hold behavior.
This removes the high fanout left on index bits 3 and 4 by the combinational
source152 rewrite (555 loads each). Source155 has one load per index bit and
at most 64 loads per decoded selection bit. Its 536 executions and timed
metrics match source152 exactly; the independent original/predecoded CSR RTL
comparison also passes all 262,144 read cases. Generic CsrFile use retains its
binary-index interface behavior through a default-disabled predecode parameter.
Its routed result fails at setup -6,063 ps / hold -20 ps; serialized route
ownership passes. It is not qualified.

Source157 adds two inverse JALR prediction operands to the existing EX boundary.
EX selects an equality result using the low sum bit instead of waiting for the
forwarded operand's target-adder carry. Actual jump targets and CM recovery
edges are unchanged. All 536 ordered executions and timed metrics match155.
Independent native RTL and Rust reference checks pass 1,000,000 comparisons
including held EX state and 32/64-bit wraparound; 1,503,456 two-state algebra
cases also pass. The proof concerns the reduced prediction match, not equality
of each internal slice flag or arbitrary four-state behavior. Evidence:
`software-validation157.json`, `user-157-jalr-rtl-test.log`, and
`user-157-jalr-algebra.json`. No FPGA programming or timing sign-off is implied.


Source157 routing was cancelled after more than 65 minutes with approximately
200 unresolved nets. This bounds the experiment, not a proof of unroutability.
Sources158 and160–166 also remain timing failures; routing ownership audits
pass for158–162 and164–166. None replaces the qualified image.

Sources167/168 combine the signed memory immediate and access-byte displacement
before the existing EX capture, allowing the effective address and PMP last-byte
address to be added in parallel. A bit-32 correction preserves effective-address
wraparound. An independent actual-RTL comparison against the original serial
adder and Rust arithmetic passes 688,128 cases, covering every signed 12-bit
immediate, all access sizes, and directed wrap boundaries. Source167 matches
source158's 536 ordered CPU records and timed metrics. Source168 applies only
this change to the best two-stage RAS RTL (source19); its records and metrics
match source113, previously checked against source19.

Source169 additionally captures the upper-address zero predicate alongside the
original fetch/data PMP owner. PmpPermissionEnd observes upper address bits only
through that predicate; low addresses, bounds, privilege and access conditions
are unchanged. Independent original/compressed-input RTL comparisons pass
2,097,152 cases, and source169's 536 CPU records and timed metrics exactly match
source168. The predicate substitution also preserves four-state equality
semantics; this is a local proof, not whole-CPU formal equivalence.

These candidates retain 124.8/62.4 MHz, the measured models, 194 ps setup/hold
uncertainties and the existing four exceptions. Sources167/168/169 fail routed setup/hold at -7,423/-28, -6,545/-36,
and -7,287/-36 ps respectively. All three pass routing ownership audits;
none is timing-qualified.

Texo PR [#81](https://github.com/fabrica-eda/texo/pull/81) fixes anchored analytical
placement unnecessarily changing BEL assignments when tile targets did not
move. The regression fails before the fix and passes afterward; the upstream
suite passes 231 tests (two ignored), formatting and strict Clippy, with CI green.
Re-evaluation of the best saved CPU placement (candidates159/161) still gives
-2,796/-36 ps. No FPGA timing improvement is attributed to this fix.


Texo PR [#82](https://github.com/fabrica-eda/texo/pull/82) permits validated
partial advisory route trees and optional initial per-sink routing priorities.
The router completes missing sinks, and fresh measured STA remains mandatory.
The upstream suite passes 91 flow tests, strict Clippy and formatting, with CI
green. Candidate175 isolates partial-tree reuse against170: mapped connectivity
and final placement are identical, but setup improves from -8,777 to -6,915 ps;
hold stays -36 ps. Both fail. Broad transferred priority weights stalled routing
in173/174; those runs were cancelled after approximately20 minutes without a
checkpoint. They are not proofs of unroutability.

Source177 simplifies the two-stage PMP completion predicate using the reachable
states0/1/2. The 192 enumerated state/control cases and all536 CPU executions,
including timed metrics, match172. Routed setup/hold is -8,949/-36 ps and routing
ownership passes. Source179 replaces binary RAS pointers plus read decode with
directly rotated one-hot pointers, preserving restore with simultaneous
retirement. Ten predictor/RAS tests and536 CPU executions pass, with timed
metrics identical to177. Routing179 exhausted256 iterations with four
overused wires; no legal checkpoint or STA result was produced.

Source180 narrows the compressed PMP address register from64 to33 bits and
zero-extends it at the checker. The removed upper31 bits reset to zero, their
only capture writes zero, and holding retains zero. The previous synthesis
retained these redundant registers and their reduction logic. This explicit
width change removes75 mapped cells;536 ordered CPU records and timed metrics
match179 exactly. Routing180 exhausted256 iterations with one overused
wire; no legal checkpoint or STA result was produced. These functional passes
are not permission to replace the qualified bitstream.

Source181 registers four range-nonempty predicates alongside their corresponding
PMP bounds and configuration. Generic PmpPermissionEnd still computes the
original comparisons; Core uses the captured predicates. Reset, TOR empty and
reversed bounds, and NA4/NAPOT upper bounds at2^32 are covered by131,072 clocked
old/new RTL comparisons plus2,097,152 permission comparisons. All536 CPU
executions and timed metrics match180. The mapped netlist removes188 LUTs and
adds four FFs. Timing is pending. Candidate178's preserved-route experiment
fails at -11,677/-36 ps despite legal ownership; it is not adopted.

Texo PR [#83](https://github.com/fabrica-eda/texo/pull/83) reopens setup route
trials after a strict committed improvement changes routing occupancy or timing.
A router-backed two-net regression fails before the fix: A is initially blocked
by B; B moves away, but the old scheduler never retries A. The fixed scheduler
retains deduplication within unchanged epochs and the existing strict objective
and time budget. Upstream88 tests, strict Clippy, formatting and CI pass.

The local measured-geometry trial now retains stationary branches sharing a
prefix with a moved endpoint. Candidate185's10-cell proposal routes in7
iterations versus43 in183; its setup/hold is -3,644/-36 ps versus -5,365/-36 ps.
Both proposals are rejected because the saved incumbent is better. This is
search improvement, not CPU timing closure.

Source186 rotates RAS storage so the top is directly available at slot0. Push,
pop, overflow, empty handling and restore with current retirement preserve the
valid stack contents. Ten predictor/RAS tests and536 CPU records pass, including
identical timed metrics to181. Its graph has42266 cells and46015 nets; physical
qualification is pending. The predictor ignores the top value while empty.

Texo PR [#84](https://github.com/fabrica-eda/texo/pull/84) fixes the initial
timing-route fallback when measured timing disables the placement-delay
predictor. Empty priorities selected hop-first routing, with installed delays
used only to break ties. Uniform minimum timing-enabled priorities now make
those installed delays the primary cost until actual STA supplies criticalities.
No estimated slack, delay table or sign-off constraint is introduced or changed.
A synthetic router fixture prefers a2000 ps direct edge over a100 ps detour
before the fix and correctly uses the faster path afterward; these fixture
values are not FPGA measurements. Upstream88 tests, strict Clippy, formatting
and CI pass. CPU re-evaluation has not produced a qualified checkpoint.

Source187 keeps the RAS arrays and pointers but caches the committed and
speculative tops at their existing state boundary. Push bypasses the new link,
pop selects the previous second entry, and restore includes current retirement.
Ten predictor/RAS tests and536 CPU records pass with timed metrics identical
to181. Its graph has42302 cells and46054 nets. Empty top values are unobserved
by the predictor. Physical qualification is pending.

The execution environment restarted before candidates181/184–190 completed.
Their running processes and temporary logs were lost; saved input manifests,
source snapshots and completed checkpoints remain. Candidate183 logged -2,796/-36 ps, unchanged from95, but its saved JSON
is truncated and unusable. It is excluded from checkpoint comparisons. Runs181 and184 were restarted from verified
input hashes, with logs under the measurement directory. Candidate191 compares
measured-delay route ECO search against184’s hop-priority search from the same
95 checkpoint. All use unchanged clocks, measured tables,194 ps setup/hold
uncertainty and four exceptions. No new bitstream has been programmed.

Candidate192 repeats191 with256 route iterations instead of32. Candidate193
uses an opt-in setup-placement policy that retains stationary sibling branches
even when they share a prefix with a moved sink. Conflicting owners are still
released, every proposed route is checked, and fresh whole-design STA remains
the acceptance gate.100 flow tests and formatting pass; the router fixture
covers both shared and unshared immutable partial trees. The default policy is
unchanged. Physical results remain pending. Logs and source/binary hashes are
stored under the measurement directory, including the environment flag.

Checkpoint storage now uses `.txcp`: versioned CBOR in a checksummed Zstd frame.
The writer streams directly to an atomic, synchronized temporary file; it does
not emit expanded JSON. Legacy JSON remains readable. Resume, bitgen,
visualization, physical hints and Python route/JTCK audits accept binary input.
The Nix shell supplies cbor2 and zstandard. Small reports remain JSON.

Converting checkpoint95 reduced478,693,319 bytes to25,698,710 bytes, with full
content equality verified on reread. Its binary route-ownership audit passes.
Bitgen still rejects it for missing timing-closure evidence; conversion changes
neither STA nor qualification.44 Rust library,16 CLI and3 Python tests pass.
Candidate194 exercises the binary route hints and output with fresh STA. Its
first attempt found missing historical Nix tool paths in the measured library;
exact-hash Yosys/nextpnr store outputs were restored and GC-rooted. Input checks
now verify those transitive measured inputs before starting expensive work.

Candidate194 completed the binary-input/output PNR check at -2,796/-36 ps.
The complete placement, routes, packing and timing records equal95. Its binary
checkpoint is25,700,045 bytes and passes the independent ownership audit.
Candidate195 continues timing optimization from194 with stationary-branch
retention enabled; timing closure is still pending.

## Timing RTL experiments, 2026-10-01

The attempted additional 62.4 MHz memory-return register was withdrawn: its
extra memory edge adds two CPU clocks to the response path, which is not an
acceptable default tradeoff for this platform without a data cache. Memory
RTL, wrapper connections and memory tests were restored to their original
response latency. The experimental parameter and response-token register have
been removed. The rejected source and validation records remain archived for
comparison, and its unfinished PNR processes were stopped.

CSR address properties are also computed in fetch and registered with the
existing decode bundle (`fd_csr_known_q`, `fd_csr_writable_q`). The standalone
and prefetched paths share `DecoderWithCsrClass`; the full decoder is not
duplicated. Exhaustive CSR tests compare legality and index for 196,608 cases.
The original-latency baseline passes 536 native runs and simultaneous-clock
kernel simulation: 239,565 cycles, 66,498 commits and 71,743 memory-wait cycles.
The withdrawn memory-stage version needed 295,471 cycles for the same ELF,
about 23% more; the timer-driven workload also committed more instructions.

The replacement changes only the return-path selection logic. The low two
byte-rotation bits are decoded into four one-hot bits on the existing BRAM
read edge; a final 32-bit half swap handles the high rotation bit. This limits
fanout compared with a complete eight-way crossbar. `LoadFormat` combines disjoint data
and sign-extension terms instead of cascading complete 64-bit results. The
response-token edges and CPU return register are unchanged. Six memory tests,
34,944 native/mapped load-format cases, all 536 CPU runs and the simultaneous
kernel pass. All 536 native execution records and the kernel's timing/counter
metrics exactly match the original-latency baseline. Records and source hashes
are in `.agent-artifacts/rtl-timing-20261001/hybrid-return/`; the baseline is
under `no-extra-latency/`.

An isolated same-tool synthesis comparison, changing only `HardwareMemory`
and `LoadFormat`, reduces the maximum BRAM-to-return-FF combinational cell
count from eight to six (64 return endpoints checked). This is structural
evidence, not a delay estimate: it does not account for unequal primitive
delays, routing, setup or hold. The imported primitive graph contains 41,935
cells versus 42,015 in the baseline and 42,227 in the eight-way variant.
These are graph-cell counts, not final occupied-BEL counts. The graphs are
stored as `.txcp`.

Physical re-evaluation uses 124.8/62.4 MHz, the unchanged measured models,
194 ps setup/hold uncertainty and the four existing endpoint exceptions.
Initial unseeded routing trials were stopped after stagnation. Structurally
matched placement trials for the memory-stage-only version exhausted 128 route
iterations with 597 or 853 overused wires; neither produced valid timing
evidence. Releasing 718 placement anchors around the changed memory/PMP stages
also failed routing (1,366 overused wires). Matching ordinary CE-driver LUTs
by their exact destination FF/CE sets restored another 462 placement anchors;
that trial still failed with 963 overused wires. These are routing failures,
not measurements of the new RTL's setup slack.

The withdrawn trial also retained partial advisory trees: 962 trees reused 15,529
common physical sinks even when the complete sink set has changed. All reused
data routes remain movable, and qualified clock-buffer locations are retained.
Functional validation does not qualify timing or a new FPGA image.
Inputs and logs are recorded in `.agent-artifacts/rtl-timing-20261001/`;
physical qualification is pending. None of the rejected memory-stage timing
trials qualifies the current RTL. The eight-way variant also failed initial
routing with 2,060 overused wires; its slow hop-first trial was stopped when
the smaller hybrid passed equivalent functional checks. The hybrid PNR uses fresh synthesis,
whole-group placement hints, 61 additional direct load-return LUT/FF pairs and
mutable complete/partial route hints. Clock, model and uncertainty settings
remain unchanged. This seeded hybrid trial also exhausted 128 iterations,
with 1,363 overused wires and no checkpoint. A fresh analytical placement is
also unsuccessful (9,590 overused wires after 128 iterations). An isolated
order-only routing experiment ends with 1,449 overused wires; it is not adopted.
These failed trials established no setup/hold result; the later legal runs are
recorded below.

Texo [#88](https://github.com/fabrica-eda/texo/pull/88) addresses a reproduced
router defect: a congested path inside the fixed timing-search corridor prevents
the outside-corridor fallback from being searched. A two-net regression has a
legal external detour, but the old router exhausts all 128 iterations. Widening
only stalled negotiated searches makes the regression pass. Related validation
passes 388 tests, strict Clippy and formatting checks. The full hybrid RTL trial
uses an isolated build with Rica's exact dependency versions and Veryl patch;
its inputs are in `.agent-artifacts/rtl-timing-20261001/router-corridor/`.
This expands a geometric search window, not STA uncertainty or model margins.
CI passes. The corridor-only Rica trial was stopped after reaching iteration 49
with 1,324 nets still requiring rerouting; widening to 32 tiles greatly increased
runtime without resolving congestion. It produced no legal checkpoint or STA.

Texo [#89](https://github.com/fabrica-eda/texo/pull/89), stacked on #88, fixes a
second reproduced negotiation failure. On a one-tile graph, two conflicting
nets require two neighboring nets to move to private alternatives. Additive
history penalties never outweigh temporarily displacing both neighbors within
128 iterations. Weighting present overuse by accumulated history resolves the
regression. Resource costs remain 64-bit throughout the caches and searches.
All 390 related tests, strict Clippy and formatting checks pass. The combined
Rica evaluation is in `.agent-artifacts/rtl-timing-20261001/router-history/`,
using the same source, dependencies, hints, measured models and constraints.
Both PRs remain drafts while large-design qualification is pending.

The combined #88/#89 trial was stopped after iteration 34 (3,662 nets
requiring rerouting). Removing only the advisory route hints, with the same
placement and LUT/FF pairs, was also stopped after iteration 32 (2,284 nets).
Neither produced a legal checkpoint or a timing result; these are cancelled
experiments, not completed 128-iteration failures. An audit of 39,489 complete
advisory trees found no unused terminal branches. Another 628 complete trees
were outside the available logic-pin inventory; partial hints already prune
unretained branches.

Texo [#90](https://github.com/fabrica-eda/texo/pull/90), stacked on #89, addresses
bulk rip-up hiding other dirty nets' occupancy. Its regression has a mandatory
chain and a net that can cross that chain at 129 locations or use a legal
private path. The old router exhausts 128 iterations by moving the conflict
between crossing points. Retaining other nets until their own replacement
search resolves it within ten iterations. Released cost caches and resource
owners are refreshed along with occupancy. All 391 related tests, strict
Clippy and formatting checks pass. The same-source, same-hint Rica evaluation
uses commit `1b0b9711e62a024c340081becfcc650d40c8aa15` in an isolated build under
`.agent-artifacts/rtl-timing-20261001/router-occupancy/`. At iteration 23, nets
requiring rerouting fall from 3,069 with #88/#89 to 265 with #90. The run
finishes at the existing 128-iteration limit with eight overused wires and no
overused PIPs; seven involve the reset distribution net. It has no legal
checkpoint or STA result. An isolated `router-extended/` experiment raises only
the routing iteration cap from 128 to 512; its 237 PNR tests pass, with two
existing ignored tests. That budget change is not part of #90. Rica's default
dependency pin is not changed by these experiments.

The 512-iteration trial completes initial routing after 178 iterations
(323.52 seconds of routing). Its initial STA is still failing: setup
WNS -4,187 ps, TNS -1,518,329 ps across 1,829 checks; hold WHS -20 ps,
THS -459 ps across 42 checks. This is a result for the current hybrid RTL,
not the withdrawn memory stage. Setup optimization with a 900-second budget
and a separate zero-budget diagnostic capture have both finished. No hardware
qualification follows from routing alone.

The diagnostic checkpoint is saved under `router-extended/capture/` (SHA256
`418a0b4fea647881ab48e3547eaa54dc701ae3686dedffa2baf5b9253f88f165`).
Independent ownership checks pass for 45,472 nets, 594,778 wires and 549,306
PIPs, with no duplicate route resources. Fixed clocks/model/uncertainty/exception
policy checks pass; timing does not. The 900-second setup optimization also
finishes at -4,187/-20 ps. The worst check is a speculative return-stack CE;
its equal-slack fan-in cone starts at atomic-LSU `done`, with 2,851 ps on the
final CE net. The memory-return worst setup check is still -2,554 ps.

The next RTL revision predecodes the RAS write destination independently of
the late predict/retire-valid signals, qualifying the write at the final gate.
Calls write SP, while a pop/push writes SP-1 when nonempty; restore still takes
priority and includes the current retirement. It adds no register stage.
Ten predictor/RAS tests, 536 native runs and the simultaneous-clock kernel
pass. All native execution records and kernel cycle/counter metrics exactly
equal the hybrid baseline. With the same mapping tool and options, the maximum
combinational cell count from atomic `done` to each of the 256 speculative RAS
CE endpoints drops from seven to five; the graph grows from 41,935 to 41,964
cells. This is structural evidence only. The new trial and source hashes are
under `.agent-artifacts/rtl-timing-20261001/ras-valid-last/`. The previous
checkpoint does not qualify this new RTL.

The RAS revision's first routed STA is setup WNS -5,743 ps, TNS -1,047,372 ps
across 1,564 checks, and hold WHS -15 ps, THS -499 ps across 46 checks. Its
aggregate setup deficit improves, but its worst setup is worse than the hybrid
baseline. The completed 900-second optimization improves WNS to -3,831 ps,
with TNS -1,046,150 ps across 1,565 failing checks; hold remains -15 ps across
46 failing checks. Thus the final WNS improves on the hybrid by 356 ps, but
timing is still far from closed. Independent ownership checks pass for 45,504
nets, 596,793 wires and 551,289 PIPs. Fixed clock/model/uncertainty/exception
policy checks also pass. The optimized checkpoint SHA256 is
`548914ced189bdd7d3e99ad96657db9a351758b2bbce0682b2cc44e7971151dd`.

The final worst endpoint is `ex_frontend_block_q`, followed by
`ex_pmp_fetch_fault_q` at -3,747 ps and `ex_pmp_second_parcel_q` at -3,388 ps.
The memory return still fails by 1,898 ps. A read-only assessment under
`.agent-artifacts/rtl-timing-20261001/frequency-assessment/` reconstructs the
exact saved implementation and evaluates related CPU/memory clock waveforms
at a constant 2:1 ratio. This assessment does not change the platform clocks,
the RTL, memory response latency, measured delays or timing margins.

## Timing feasibility assessment, 2026-10-02

The diagnostic replay reproduces every original placement/pin binding, packing
entry, routed wire/PIP, primitive metadata record, and STA result. It uses the
same Texo revision plus read-only diagnostics; the original 124.8/62.4 MHz
waveform also reproduces every timing-report field. The sweep retains the
actual generated-clock graph: CPU from the 12 MHz reference, memory from CPU
divided by two, and unchanged 6 MHz JTCK. All measured cell and net delays,
194 ps setup/hold guards, 22,508 checked endpoints and four unchecked endpoint
exceptions remain unchanged. This is full STA with changed waveforms, not a
single-WNS-to-Fmax calculation.

| CPU / memory MHz | Setup WNS (ps) | Setup failures | Hold WHS (ps) |
| --- | ---: | ---: | ---: |
| 124.8 / 62.4 | -3,831 | 1,565 | -15 |
| 104 / 52 | -2,229 | 88 | -15 |
| 96 / 48 | -1,427 | 26 | -15 |
| 84 / 42 | +61 | 0 | -15 |
| 80 / 40 | +656 | 0 | -15 |
| 78 / 39 | +977 | 0 | -15 |

Exact maximum-arrival reconstruction identifies the worst path as
`rx_pc_q[19].Q -> ex_frontend_block_q.DI`: clock-to-Q is 195 ps,
combinational cell arcs sum to 2,848 ps and routed net delays sum to 8,367 ps.
The 5,391 ps PC-to-PMP-comparator net spans R55C48 to R50C24 and has 15 sinks.
A separate CE path has an 8,269 ps net from R68C62 to R48C29; its source drives
218 sinks. Every traced edge and complete delay sum is checked against the
STA engine's actual launch-domain arrivals. The trace does not establish an
intrinsic RTL limit below 124.8 MHz: these paths are dominated by placement
and routing. Continuing to reduce Boolean depth alone is insufficient.

The sweep is diagnostic evidence only. It does not qualify a new platform.
In particular, its 46 hold failures are **not a failed hold-fix attempt**:
the original setup-negative implementation never enters Texo's post-setup hold
repair, and the waveform sweep deliberately does not change routing. The new
physical implementation must close setup and then actually run hold repair.
Detailed source/tool hashes, exact paths, frozen-replay verification and sweep
results are under `.agent-artifacts/rtl-timing-20261001/frequency-assessment/`.

## Platform redesign, 2026-10-02 — STA signed off; FPGA execution pending

The user permits a lower target frequency only with a coherent redesign for
that target. A PLL-only slowdown or merely adjusting the software timebase does
not meet that requirement. The former 78/39 MHz recommendation is superseded.

The working candidate uses **78 MHz CPU and 78 MHz memory**. It is configured
by `platform-user.json`; `tools/user_platform.py` generates and checks the PLL
and `src/UserPlatform.veryl`. Native simulation, simultaneous-clock simulation,
and the OS timebase follow the same profile. Legacy non-U board targets retain
their own configuration and are not implicitly requalified.

| Component | Redesign for this candidate | Verification requirement |
| --- | --- | --- |
| Memory clock | Same rate as CPU; no added response stage | Both serialized edge orders and simultaneous edges |
| Data PMP | Address/end capture combined, permission capture second | Faulting loads/stores/atomics cannot issue a memory token |
| Integer execution | Finish shifts and SLT/SLTU in EX, forward via existing CM result register | RV64 word sign extension, SLLI.UW, dependent consumers |
| Register file | Retain synchronous read required by the memory primitive | Existing forwarding and hazards |
| Retirement | Retain completion/retirement boundary for loads, multi-cycle units and precise traps | Commit comparison and U-mode trap tests |
| OS scheduler | Read actual timebase and calculate a 50 µs quantum once at boot | Timer preemption and isolation workload |
| Physical implementation | Fresh synthesis at the target, placement, routing and setup/hold repair | Measured model, 194 ps guards and exactly four existing exceptions |
| Bitstream gate | Compare actual PLL parameters/phase and STA clocks with profile | Reject changed PLL even when claimed periods match |

The first candidate (same-rate memory and two-stage PMP, before early ALU)
passes 536 native ELF runs and the simultaneous-clock U kernel. The latter uses
214,070 cycles at 78 MHz (2.744 ms), with 67,997 commits. This is slower in wall
time than the former 239,565-cycle/124.8 MHz run (1.920 ms), and the kernel ELF
also changed; these are not a controlled performance comparison. That result
motivated moving shift/compare completion into EX and removing the scheduler's
per-switch division. The early-ALU candidate passes the same 536 native runs and the simultaneous
U kernel (212,225 cycles, 67,819 commits, 2.721 ms). Its physical run is separate.
A third candidate removes the CPU load-return register at equal clock rates
and formats the held response in CM; this changes a real response latency. It passes 536 native runs, the U kernel
at simultaneous clocks (208,304 cycles, 67,819 commits, 2.671 ms), and 31 additional
simultaneous load/atomic/PMP cases. Rebuilt legacy M kernels also pass all 15
serialized/simultaneous checks, including console I/O. The direct-return physical result is recorded below; it fails timing.

A controlled serialized comparison contains 258 identical ELF hashes with equal
commit counts. The early-ALU candidate takes 1.360 times the baseline execution
window at the median (1.396 times when weighted by total execution cycles).
ALU/branch/call loops are about 1.60 times slower, while pointer chasing is
1.334 times slower. The lower-frequency candidate therefore does not preserve
the former platform's throughput. The old U kernel has a fixed timer quantum;
even with identical ELF bytes its interrupt/commit count changes with timer Hz,
so it is not used as a controlled throughput benchmark.

The intermediate experiments below are distinct from the final signed-off
registered-response implementation. Functional results and
physical inputs are separately hash-bound under
`.agent-artifacts/platform-redesign-20261002/`; its `early-alu/` subdirectory
contains the second candidate; `direct-return/` contains the third. Large physical graphs, routes and checkpoints
remain compressed `.txcp` artifacts.

Texo [#87](https://github.com/fabrica-eda/texo/pull/87) fixes the placement-model
loader's missing support for archived measurement inputs. Rica pins commit
`97b2b0490a2d200d60e3265f1fd46069c6a6dbb9`, which hashes decompressed original
bytes directly and does not stage large JSON files. This changes artifact
reading, not model coefficients or timing margins.

The direct-return candidate improves pointer chasing to 1.202 times the former
124.8 MHz execution time, array copy to 1.340 times, and array sum to 1.372 times.
Across the 258 matched cases its median is 1.387 times and its total weighted
ratio is 1.397, so removing the stage is not a universal throughput improvement.
The candidate remains a timing/performance tradeoff, not a throughput-preserving
replacement. Profile checks reject direct return with unequal CPU/memory clocks.

Mapped-netlist inspection found that the early-ALU candidates retained 64 old
`cm_shift_low_q` FFs even though their completion result was no longer selected.
The `compact-ex/` trial exposed a signed-branch regression in `soft_divide`
because `cm_compare_signed_q` is also needed for branch recovery. It was rejected
before PNR. The corrected `compact-ex-final/` candidate retains that shared bit
and explicitly holds only the obsolete shift payload/arithmetic/direction at
zero when `EARLY_ALU` is enabled. Shared
word/amount/compare metadata for the separate Zbb/Zbs unit remains live. The
previous `direct-return/` physical run was stopped before completion and has no
timing qualification; it is not reported as a failed timing attempt.

The corrected compact-EX candidate passes all 536 native cases and the
simultaneous U kernel. Every native cycle/commit record and the timed metrics
match the prior validated direct-return candidate exactly. Its mapped register
count is 10,549, down 66 from direct-return and 133 from the first 78 MHz
candidate. It also removes the obsolete `cm_shift_low_q[*]` fanout constraint.
Its completed physical run has setup −2,451 ps (204 violations), hold −20 ps
(90 violations), and valid route ownership. The worst saved setup endpoint is
`ex_operand_a_q[56]`; its equal-slack fan-in includes a RAM-to-load/bypass
path. This candidate is not timing-qualified.


The active `registered-ex/` candidate retains the CPU load-response register
while keeping same-rate 78 MHz memory, two-stage PMP, EX shift/compare completion,
and explicit removal of obsolete shift state. This follows measured physical
results: the earlier registered-response implementation reached −532 ps setup
with seven violations, whereas compact direct return reached −2,451 ps.
Functional validation and fresh implementation use frozen sources in the new
candidate directory; its final static signoff is recorded below.

The first redesign candidate completed actual hold repair: setup +123 ps and
one remaining hold violation at −5 ps, on EX-to-CM fallthrough PC bit 19. Its
ownership and fixed clock/model/guard/exception policy audits pass, but timing
does not. Repeating the search and refining arrival buckets alone did not repair
that path. Two separately reproduced hold-search defects are submitted as
[Texo #91](https://github.com/fabrica-eda/texo/pull/91), stacked on #90: coarse
arrival buckets can discard useful slower prefixes, and a cyclic walk can
suppress a valid simple path before late cycle rejection. The fix passes 389
CLI/flow/router tests (two ignored) and strict all-target Clippy. Its combined
physical comparison also finishes at +123/−5 ps. The defects are reproduced,
but they do not account for the remaining physical violation. CI passes. A
separate local-placement experiment now considers both register endpoints; the
existing repair only relocates capture FFs.


The active registered-response candidate passes 536 native runs and the
simultaneous U kernel (212,225 cycles, 67,819 commits). All native cycle/commit
records and normalized timed metrics equal the validated early-ALU candidate.
Its mapped FF count is 10,614, removing the 66 obsolete shift FFs while restoring
the 65 load response/token FFs relative to direct return. Its final PNR passes.


### Saved 78 MHz implementation (2026-10-02)

The saved profile has CPU/memory clocks of **78/78 MHz**, `PMP_DATA_STAGES=2`,
`EARLY_ALU=1`, and `DIRECT_LOAD_RETURN=0`. Native simulation (536 cases), the
simultaneous U kernel (212,225 cycles / 67,819 commits), and 31 additional
simultaneous load/atomic/PMP tests pass. The resulting hardware has 10,614 mapped
FFs. Native synthesis mapping reports equivalence; a full post-map functional
simulation of the complete CPU is not supplied.

The compressed checkpoint has **setup +33 ps / hold +1 ps**, with all 22,372
setup and 22,372 hold checks passing. The measured model, 20% temperature margin,
194 ps setup/hold guard, and exactly four existing exceptions are unchanged.
All 45,746 net trees have exclusive wire/PIP ownership. The independent JTCK
graph matches all 515 setup and 515 hold endpoints, including edge polarities.

The supplemental JTCK checker previously loaded the architecture's original
cell table. It now installs the checkpoint's hash-bound measured table and
uses routed physical LUT inputs. Changed hashes and missing measured provenance
are rejected. Bitgen also uses the exact audited architecture, combined with
an installed target pack's codec, IO database and shared libraries; the wrapper
requires `--bitgen-runtime` for that pack directory. It never silently selects a
different architecture for encoding.

The first redesign's stubborn −5 ps hold path exposed a missing placement
search: its launch FF belonged to a dedicated LUT/FF pair. Moving the whole pair
closed that earlier implementation at +123/+1 ps with the same policy. This is
[Texo PR #92](https://github.com/fabrica-eda/texo/pull/92), stacked on
[#91](https://github.com/fabrica-eda/texo/pull/91). Both CI runs pass; neither PR
was merged here. The final compact-EX implementation independently closes using
the #91 router fixes; its tool base and isolated 512-iteration cap are recorded.

For 258 identical-ELF/equal-commit comparisons, elapsed execution time has a
median ratio of **1.360** and a cycle-weighted ratio of **1.396** versus the
124.8 MHz baseline. ALU/branch/call loops are about 1.60 times slower; pointer
chasing is 1.334 times slower. Timing closure does not imply preserved throughput.

[Signoff and input hashes](../toolchain/measurements/2026-10-02-rv64-u-78/signoff.json)
record the frozen source, compressed checkpoint, validation results and tool
patch. `rebuild-tool.py` there recreates the exact PNR base plus patch in an
isolated worktree and builds it with Nix/MBX, without replacing the original
implementation binary. The selected bitstream is
`build/riscv-hardware-compact-user-78-20261002.bit`, SHA-256
`d2daa981e6621fba590178119797f0212fed849466b0a46e73714cf2bc0097de`.
No FPGA write or execution was performed for this image. JTAGG hard-macro arcs,
reset recovery/removal and bundled-data CDC remain the previously declared
qualification boundaries; no additional exception was introduced.
