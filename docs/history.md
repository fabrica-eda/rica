# History

Milestones of the RV64 core, all on the LFE5UM5G-85F-EVN. "Hardware" means the
exact bitstream passed the listed runs on the board after routed STA and checked
bitgen. Until 2026-09-22, timing sign-off used Texo's ECP5 tables plus a 200 ps
setup/hold reserve; from 2026-09-23 it uses the measured library with 194 ps
guards ([timing-characterization.md](timing-characterization.md)).

## M-mode core at 124.8 MHz

The first design rule was a fixed 124.8 MHz CPU / 62.4 MHz memory clock: a
change that missed routed timing was rejected even if it saved cycles.

| Date | Milestone |
| --- | --- |
| 2026-09-02 | A TAGE-based front end (two-way BTB, bimodal plus four tagged tables) did not close at 124.8 MHz (nextpnr reached 89.6 MHz). Kept as a comparison configuration until it was removed on 2026-10-09 |
| 2026-09-05 | Static predictor with a 4 KiB synchronous L1 instruction cache closes at 124.8 / 62.4 MHz |
| 2026-09-06 | Compact predictor (64-entry BTB, 2-bit direction, 8-entry RAS): 100/100 ELFs on hardware, 1.22× geometric-mean speedup over the static predictor on four diagnostics |
| 2026-09-06 | JTAG receiver moved to falling-edge sampling, fixing a placement-dependent load failure; 8-byte debug writes |
| 2026-09-06 | Earlier store acknowledgment and PC+16 prefetch: load/store throughput +10.4%, nested calls +5.6% |
| 2026-09-07 | Loads one memory edge shorter; RV64M; 165 hardware runs |
| 2026-09-07 | DSP-based 64×64 multiplier |
| 2026-09-09 | Zba, Zbb and Zbs (full B) on hardware; early forwarding of simple bit operations; faster LSU (`FAST_LSU=6`); taken-target preview. 310 hardware runs |
| 2026-09-10 | Early-terminating divider (C division workloads 1.5–1.7× faster); machine interrupts and timer; strict 64-bit physical access checks; first Rust kernels. 332 hardware runs |
| 2026-09-11 | JTAG console with interrupt-driven receive; 327 hardware runs |
| 2026-09-22 | A extension (Zaamo, Zalrsc): 267 hardware runs including an atomic multitasking kernel |
| 2026-09-22 | On-board delay measurements: ring-oscillator placement model and PLL phase-sweep capture budgets |
| 2026-09-23 | Full STA library replaced by on-board measurements: setup +2 ps, hold +1 ps after 194 ps guards; 267 hardware runs |

Performance of the 2026-09-10 image at 124.8 MHz (median of three hardware runs):

| Diagnostic | Cycles | IPC | MIPS at 124.8 MHz |
| --- | ---: | ---: | ---: |
| `alu_chain` | 5,993 | 0.770 | 96.1 |
| `load_store` | 4,424 | 0.292 | 36.4 |
| `m_divide` | 21,445 | 0.120 | 15.0 |
| C division, small | 15,388 | 0.152 | 18.9 |
| C division, wide | 17,333 | 0.135 | 16.8 |
| C division, words | 17,835 | 0.131 | 16.3 |

These include startup and self-checks; they are diagnostics, not an
application benchmark.

## Privilege and virtual memory

U-mode with PMP could not close at 124.8 MHz: the PMP comparators landed on the
fetch and data issue paths. The platform was redesigned around a configurable
clock profile (`riscv/platform-user.json`) and compared by execution time for
the same work rather than by frequency.

| Date | Milestone |
| --- | --- |
| 2026-10-02 | Redesigned U/PMP platform closes STA at 78 MHz |
| 2026-10-04 | 84 / 84 MHz U/PMP image: setup +26 ps, hold +1 ps; 268 ELFs pass on hardware, the first run with tapir |
| 2026-10-05 | One-entry PMP permission caches; closes at 104/52 and 108/54 MHz |
| 2026-10-05 | 112 / 56 MHz after 16-bit carry-select addition and deferred data-fault `mtval` selection: setup +10 ps, hold +1 ps; 269 ELFs |
| 2026-10-05 | S-mode with trap delegation: 270 ELFs. Closing it needed Texo's re-placement from routed timing |
| 2026-10-06 | Sv39 (data and instruction translation, S-mode kernel with isolated U tasks): setup 0 ps, hold +1 ps at 112 / 56 MHz; 274 ELFs |
| 2026-10-07 | Toolchain update (Struo `bee70eb`, Texo with probed re-placement); PMA/PMP checker inputs registered in the Sv39 path. Setup 0 ps, hold +1 ps; 274 ELFs |
| 2026-10-09 | Texo 0.3.0; TAGE and bimodal predictors removed (80 dead registers left the compact image). Setup 0 ps, hold +1 ps with a 7,200 s setup budget; 274 ELFs, cycle counts within two cycles of 2026-10-07 |
| 2026-10-09 | Sv39 translation, PMP checking and the fetch queue moved out of `Core`, whose processes are split per stage; Struo 0.2.1 so that the split synthesizes to the same netlist as before it. Setup 0 ps, hold +1 ps; 274 ELFs, cycle counts within one cycle of the earlier 2026-10-09 image |

Selected cycle counts on the 112 / 56 MHz Sv39 image (hardware, matching the
simultaneous-clock model within one cycle):

| ELF | 2026-10-06 | 2026-10-07 |
| --- | ---: | ---: |
| `sv39-data.elf` | 29,662 | 29,745 |
| `sv39-fetch.elf` | 33,593 | 33,696 |
| `kernel-supervisor.elf` | 477,807 | 512,104 |
| `kernel-user.elf` | 231,190 | 231,189 |

The 2026-10-07 increase comes from the extra settle cycle in the walker and in
data translation.

## Next

- A TLB entry that holds a whole superpage (the S-mode kernel's main cost).
- Recovering clock frequency toward 124.8 MHz with U/S/Sv39 enabled.
- A real operating system on top of the test kernels; F/D, external interrupt
  controllers and other extensions are added when that work needs them.
