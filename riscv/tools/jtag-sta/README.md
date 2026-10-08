# JTAG STA release gate

This is the supplemental checker used by `../bitgen_jtag_checked.py`. It was
retained from the repaired JTAG investigation after its obsolete MRE/ECO tools
were removed. It shares the root workspace's Texo pin, Cargo.lock, and build
directory. Both compressed `.txcp` checkpoints and legacy JSON are accepted.
Run it through the bitgen wrapper; see [docs/fpga-flow.md](../../../docs/fpga-flow.md#checked-bitgen).

For a measured checkpoint, the checker takes the timing library as an argument
(the wrapper passes `riscv/assets/measured-sta-library-v2.json.zst` unless
`--measured-timing-library` is given), requires the SHA-256 recorded in
`target.measured_timing`, and selects LUT input delays from the routed input-permutation PIPs. Missing
measurement provenance is an error; the architecture's original cell table is
not a fallback for a measured checkpoint. The integrated checker compares all
515 setup and 515 hold endpoints, including their edges and declared margins.

The wrapper takes `--architecture` and `--bitgen-runtime` (an installed Texo
target-pack directory). It creates a local runtime view with the audited
architecture and the pack's existing codec/IO files, preserving the pack's
shared-library layout. The global target cache is not modified.
