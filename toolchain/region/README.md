# Official nextpnr region experiment

This directory contains a bounded, reproducible A/B experiment for the
`HardwareTestTop` map.  It uses an official `nextpnr-0.11.1` ECP5 binary and
the board's existing `CABGA381`, speed-grade 8, LPF, PLL, and 125 MHz target.

The input map must be supplied explicitly for a final run so that the map
revision cannot be confused with an ignored older artifact:

```sh
export RICA_NEXTPNR_BIN=/tmp/nextpnr-0.11.1-build/nextpnr-ecp5
export RICA_NEXTPNR_JSON=/absolute/path/to/final-riscv-hardware-test-125.nextpnr.json
export RICA_NEXTPNR_LPF="$PWD/riscv/lfe5um5g-85f-evn.lpf"
export PRJTRELLIS_DB=/tmp/prjtrellis-1.4-src/database
# Point this at the directory containing pytrellis.so produced by
# build_nextpnr_0.11.1.sh (for example /tmp/prjtrellis-1.4-build-shared).
export PYTHONPATH=/absolute/path/to/prjtrellis-build
sha256sum "$RICA_NEXTPNR_JSON"
toolchain/region/run_nextpnr_region_ab.sh
```

The runner performs these cases with one fixed placer/router configuration:

* no region, seeds 1, 2, and 5;
* no-op identity pre-place hook, seed 5, with placed-cell and routed-net equality checks;
* `region_memory_overlap`, `region_memory_left`, and `region_memory_right`,
  each at seed 5.

Set `RICA_REGION_CANDIDATE_SEEDS="1 2 5"` for the selected candidate(s) when
screening a winner across all three seeds; the no-region baselines are always
run at all three seeds.

Every routed JSON, report, log, and SHA-256 file is written below
`RICA_REGION_OUT` (default `/tmp/rica-region-ab`).  `provenance.txt` records
the nextpnr version and input-map/LPF hashes; `results.tsv` is the case
manifest.  The report fields to compare are the CPU clock's achieved Fmax and
period, both CPU/memory crossing delays, critical-path endpoints, and routing
delay.  A candidate is useful only if it improves the same-seed baseline and
does not introduce a failed route or a worse crossing.

The three candidate configurations constrain only the named `DP16KD` cells:

* `bram_hardware_memory.*` (128 cells) is the bulk unified memory;
* `bram_core.branch_predictor.*` (14 cells) and `bram_core.registers.*`
  (8 cells) are the CPU-side memory macros.

No LUT, FF, CCU2, or PFUMX cell is constrained.  This is intentional: the
nextpnr packer retains ownership of LUT/FF/CCU packing, and the regions are
only a placement experiment for the macro clusters.  Runtime validation in
`nextpnr_region_corridor.py` rejects an unmatched glob, an out-of-bounds
rectangle, a split assignment, or insufficient legal capacity.

For the UM5G-85F database, the candidate rectangles contain respectively
156/120, 164/120, and 160/123 legal DP16KD sites (hardware/predictor), against
the current 128/22 cell counts.  These capacities must still be reported by
the hook for the final map; a changed memory inference is intended to fail
the experiment instead of silently over-constraining placement.
