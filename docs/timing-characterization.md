# Timing characterization

Rica signs off timing with an STA library measured on the board itself instead
of the vendor-derived delay tables. The measurement and fitting scripts are in
`riscv/characterization/`. The fitted placement model and STA library, and the
raw measurements behind them, are attached to a GitHub release rather than
stored in the repository (see [fpga-flow.md](fpga-flow.md)).

All measurements were taken on one LFE5UM5G-85F-EVN (CABGA381) with the board's
nominal 12 MHz clock, fixed board supply voltage and no external instruments.
Reference-clock accuracy, voltage and temperature were not independently
measured; a 20% temperature allowance and the 194 ps guard cover them instead.

## Methods

**Ring oscillators** (`generate.py`, `build.py`, `sweep.py`, `readout.py`).
Rings of inverting LUTs with different input pins (A/B/C/D), directions,
lengths (33/65/97 stages) and tile spacings (1/4/12). One ring oscillates at a
time; a registered enable gates it, a prescaler and 32-bit counter count it over
2²⁰ reference cycles, and JTAG only reads stored results. The first pass after
power-up is discarded, each ring is measured at least four times, and a range
above 1% rejects the model. The average propagation delay per ring traversal is
`D_ps = 10¹² × 2²⁰ / (12,000,000 × 16 × C × 2)` for count `C`.

The resulting placement cost model (fitted per input pin by non-negative least
squares over LUT count and piecewise x/y distance) was trained on 72 rings and
validated on 24 others with a worst error of 4.25%. Texo uses it to rank
placement candidates; it is not a timing table.

**PLL phase sweeps** (`phase_sweep.py`, `analyze_phase.py`). The same routed
circuit is captured at PLL phase steps of about 200.32 ps (1/8 of the 624 MHz
VCO period). For each phase the circuit counts captures that saw the new value
and captures that saw the old value, separately for rising and falling data, so
that mixed values are distinguished from a stable old value. Only paths that show
both an always-old early phase and an always-new late phase are used. A boundary
combines clock-to-Q, routing, logic, setup/hold and relative clock skew; it is not
split into independent arcs.

**Full STA replacement** (`sta_sweep.py`, `build_sta_refined_library.py`,
`fit_sta_*.py`). Phase-sweep capture circuits, ring oscillators and DCCA
feedback loops cover LUTs, carry chains, PFUMX/L6MUX, FF data/CE/LSR, both DP16KD
ports (address, data, CE, WE, CS, OCE, reset release, registered and
unregistered outputs), every MULT18X18D input bit and sign control, and the
global clock buffers.

| Item | Value |
| --- | --- |
| Path samples | 474 (261 training, 213 validation on independent placements and stimuli) |
| Replaced entries | All 1,552: 373 routing classes, 1,097 cell arcs, 82 setup/hold entries |
| Removed arcs | 342 DSP arcs that do not exist (input bit *i* cannot affect product bits below *i*) |
| Temperature allowance | 20% on data-path minimum and maximum delays |
| Setup/hold guard | 194 ps each, chosen by leave-one-out cross-validation on the training set and checked on the validation set; carry minimum delays get an extra 26 ps |

The library takes only pin and PIP-class names from the original tables; no
original delay value is used for fitting or as a fallback. Partial replacement,
fallback to the old table for unmeasured entries and modified measurement files
are rejected. Because LUT inputs have different measured delays, STA uses the
arc of the physical LUT input that the routed PIPs actually select.

Maximum delays use the boundary after the last observed error (some paths
glitch again after first settling); minimum delays use the end of the initial
run of old values. Every adopted boundary was observed repeatedly in forward
and reverse sweeps.

## Reproducing

```sh
python3 riscv/characterization/test_measurement.py   # unit tests of the measurement code
```

The model and library list every measurement file they were fitted from in
`input_sha256`, relative to `riscv/assets/`. To check them, download the
evidence archive (about 1 GB compressed) and let Texo verify each hash:

```sh
riscv/tools/fetch-assets --evidence
riscv/tools/texo-measured pnr ... --verify-measured-evidence
```

Local user and host names in the archived logs and records were replaced with
placeholders of the same length before publication; the recorded hashes are
those of the published files. The fitting and sweep scripts still read and
write experiment directories under `toolchain/measurements/` (ignored by git).
`sweep.py` and `phase_sweep.py` reprogram the FPGA and refuse to overwrite an
existing experiment directory.

## Limits

- One board, one voltage, uncontrolled ambient temperature.
- Boundaries describe complete measured paths; they are fitted into a cell plus
  routing model that reproduces all 474 samples, which does not identify each
  physical pin delay on its own.
- JTAGG external boundaries remain uncharacterized and are covered by the four
  reviewed timing exceptions.
