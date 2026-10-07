# Toolchain issues

Tool defects and sign-off limitations found while building Rica. An entry counts
as a tool defect only when an equivalent RTL/netlist comparison, an independent
flow or a source-level diagnosis isolates the tool. RTL and board lessons are in
[design-notes.md](design-notes.md).

Upstream projects: [Struo](https://github.com/fabrica-eda/struo),
[Texo](https://github.com/fabrica-eda/texo), [Celox](https://github.com/celox-sim/celox).

## Struo (synthesis and ECP5 mapping)

| ID | Issue | Status |
| --- | --- | --- |
| STR-001 | Wide-cover selection could multiply LUT/mux area discontinuously | Fixed ([#79](https://github.com/fabrica-eda/struo/pull/79)) |
| STR-002 | Several partial writes to one packed vector register expanded recursively during range lowering and never finished | Open; Rica stores the RAS valid bits as an unpacked array |
| STR-003 | Module constants and dynamic packed selects were not lowered | Fixed |
| STR-004 | Top-level `always_comb` statements could violate blocking-assignment order | Fixed ([#78](https://github.com/fabrica-eda/struo/pull/78)) |
| STR-005 | Many-arm `case` statements became linear priority-mux chains | Fixed ([#81](https://github.com/fabrica-eda/struo/pull/81)) |
| STR-006 | Redundant true-dual-port BRAM clock-enable logic consumed timing margin | Fixed ([#80](https://github.com/fabrica-eda/struo/pull/80)) |
| STR-007 | Post-map simulation rejected TDP BRAM or built a linear 512-way read mux | Fixed ([#82](https://github.com/fabrica-eda/struo/pull/82)); full post-map simulation of the CPU is still too slow to be a default gate |
| STR-008 | Associative AND/OR chains kept their source association and depth | Fixed upstream (first proposed in [#83](https://github.com/fabrica-eda/struo/pull/83)) |
| STR-009 | Clock-enable fanout limits duplicate the driving LUT and can worsen its input fanout | Open design limitation; Rica applies no predictor-wide CE limit |
| STR-010 | Disjoint fields of a packed struct written by separate FF processes were reported as multiple drivers | Fixed ([#84](https://github.com/fabrica-eda/struo/pull/84)) |
| STR-011 | LUT inputs that do not affect the truth table stayed routed and timed | Fixed ([#86](https://github.com/fabrica-eda/struo/pull/86)); a remaining case with constant-tied pins fixed in [#156](https://github.com/fabrica-eda/struo/pull/156) |
| STR-012 | Nested self-hold registers did not infer clock enables | Fixed ([#93](https://github.com/fabrica-eda/struo/pull/93)) |
| STR-013 | A register whose enable folds to constant 1 was driven by an input-less constant LUT on CE, which Texo reports as an endpoint without a synchronous launch | Fixed ([#158](https://github.com/fabrica-eda/struo/pull/158)) |

## Celox (simulation)

| ID | Issue | Status |
| --- | --- | --- |
| CEL-001 | Native x86 SSA reload materialization could exhaust the register allocator | Fixed upstream |
| CEL-002 | Rejecting a >128-bit concat fold could discard operands | Fixed upstream ([#736](https://github.com/celox-sim/celox/pull/736)) |
| CEL-003 | Shared-clock FF publication order, a WASM store spill and padded native array access in source memories | Fixed ([#755](https://github.com/celox-sim/celox/pull/755)) |
| CEL-004 | Value-generic module specialization produced wrong behavior when two instances differ only in a parameter | Fixed ([#884](https://github.com/celox-sim/celox/pull/884)); Rica uses an ordinary parameter |

## Texo (place and route, STA, bitgen)

| ID | Issue | Status |
| --- | --- | --- |
| TEX-001 | ECP5 STA corner, clock-edge and margin policy did not correlate with nextpnr | Fixed |
| TEX-002 | Hold ECO kept an invalid route after rebinding a pin | Fixed |
| TEX-003 | Successful place-and-route could emit a functionally wrong configuration | Fixed |
| TEX-004 | Critical sinks could disappear from high-fanout placement edges | Fixed |
| TEX-005 | Bitgen emitted a nonexistent `JTAG.MODE` setting | Fixed |
| TEX-006 | Positive slack on checked paths allowed bitgen with unconstrained JTAG endpoints | Fixed ([#61](https://github.com/fabrica-eda/texo/pull/61), [#62](https://github.com/fabrica-eda/texo/pull/62)); closure now requires full endpoint coverage plus reviewed exceptions |
| TEX-007 | With measured timing the placement-delay predictor is disabled, so initial placement had no timing weights and closure depended heavily on small RTL changes | Fixed by re-placement from routed timing ([#102](https://github.com/fabrica-eda/texo/pull/102)), candidate ranking ([#104](https://github.com/fabrica-eda/texo/pull/104)) and probing ([#106](https://github.com/fabrica-eda/texo/pull/106)) |

Router and search fixes found along the way include 64-bit search costs (#79),
immutable-route obstacles (#80), anchored placement keeping BEL assignments
(#81), partial advisory route trees (#82), retrying setup routes after
occupancy changes (#83), measured-delay initial routing priorities (#84) and the
routability-driven initial placement used by the plain flow.

## nextpnr

**NPNR-001.** nextpnr-ecp5 marks every `EHXPLLL` port as timing-ignored, so it
cannot relate the divided CPU and memory clocks. The CPU↔memory crossings are
reported but not checked against the one-CPU-period budget that the zero-phase
2:1 clocks require. The `nextpnr-riscv` helper adds a limited setup guard for the
exact 124.8/62.4 MHz PLL configuration; it does not check hold, skew or jitter.
Texo's STA handles the related clocks directly.

## Adding an entry

Record the affected revision, a retained reproducer or numeric comparison, the
workaround in Rica, the upstream PR, and the regression that guards it. Mark an
item fixed only after the pinned dependency contains the fix and Rica's
regressions pass with it.
