# FPGA implementation flow

Rica images are produced by Struo (synthesis and ECP5 mapping) and Texo
(placement, routing, STA and bitgen). An image counts as qualified only after
all of the following pass on the exact bitstream:

1. Functional simulation (both clock orders and the simultaneous-clock model).
2. Routed setup and hold timing with the measured timing library and 194 ps
   setup/hold guards, with only the four reviewed endpoint exceptions.
3. The supplemental JTCK-domain timing check and checked bitgen.
4. The hardware test suite on the board ([hardware-testing.md](hardware-testing.md)).

## Running Texo

`riscv/tools/texo` builds and runs the Texo CLI locked in `Cargo.lock` through
the Nix shell (`nix develop`) and mbx. Texo's diagnostic metric logging is off
unless `RICA_VERBOSE=1` is set.

```sh
riscv/tools/texo --version
```

`riscv/tools/texo-measured` wraps it for sign-off. For `pnr` it adds the
measured placement model, the measured timing library and 194 ps setup/hold
uncertainty, after checking each file's SHA-256 against
[`riscv/qualified-measured-sta.json`](../riscv/qualified-measured-sta.json). The
measurement data itself (placement model and timing library) is a large archive
that is **not included in this repository**; without it `texo-measured` stops
with an error, and `riscv/tools/texo` uses Texo's built-in ECP5 timing tables
instead. Timing results from the built-in tables are not comparable to the
numbers in this documentation. How the library was measured is described in
[timing-characterization.md](timing-characterization.md).

## Place and route

The working profile, as used for the current image:

```sh
riscv/tools/texo-measured pnr riscv --top HardwareCompactUserTestTop \
  --device LFE5UM5G-85F --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf --pll-binding riscv/pll-user.json \
  --synthesis-goal-mhz 104 --jtagg-prefix jtag \
  --clock-constraints riscv/jtck-clock-constraints.json \
  --timing-exceptions riscv/timing-coverage-exceptions.json \
  --setup-optimization-budget-seconds 3000 \
  --output riscv/build/user-112.txcp
```

| Input | Purpose |
| --- | --- |
| `riscv/lfe5um5g-85f-evn.lpf` | EVN pin constraints |
| `riscv/pll-user.json` | PLL binding generated from `platform-user.json` |
| `riscv/jtck-clock-constraints.json` | 6 MHz JTCK constraint on the JTAGG output |
| `riscv/timing-coverage-exceptions.json` | The four reviewed endpoint exceptions (below) |
| `--architecture FILE` | Optional ECP5 architecture database; the recorded runs passed a local `.txdb` that includes DSP timing |

This is the **plain flow**: no seed placement, route hints, LUT/FF pairing, CE
fanout limits or modified router limits. Texo places analytically, routes,
re-places from routed timing (keeping several candidates and probing each with
a short setup search), and then runs a setup-optimization search of local moves
and route ECOs. Hold is repaired afterwards. Checkpoints are written as `.txcp`
(versioned CBOR in a checksummed Zstd frame).

The setup-optimization budget is wall-clock time. The same deterministic search
reaches the same result if it has time to finish, but on a loaded machine a
short budget can stop it early. The current image needed about 2,500 s; with an
1,800 s budget on a busy machine it stopped at −20 ps on the same trajectory.

### Timing exceptions

All four exceptions are JTAGG boundaries whose launch timing is not
characterized (`no_synchronous_launch`). They are checked functionally by RTL
tests and physical JTAG regressions rather than by numerical STA:

| Endpoint | Reason |
| --- | --- |
| `jtag_debug_transport.shift_high[63]` | JTDI launch from JTAGG |
| `jtag_debug_transport.shift_enable_q` | CE1/SHIFT launch from JTAGG |
| `jtag_debug_transport.update_q` | UPDATE launch from JTAGG |
| `jtag_debug_transport.request_toggle_meta_q` | First stage of the JTCK-to-CPU toggle synchronizer |

## Checked bitgen

`riscv/tools/bitgen_jtag_checked.py` is the only supported way to make a board
image. It checks the checkpoint's clocks against the platform profile and PLL,
re-runs the JTCK-domain STA with the independent checker in
`riscv/tools/jtag-sta` (515 setup and 515 hold endpoints must agree), writes an
audit report, and then runs Texo bitgen:

```sh
python3 riscv/tools/bitgen_jtag_checked.py riscv/build/user-112.txcp \
  --bit riscv/build/user-112.bit \
  --audit-report riscv/build/user-112-audit.json \
  --architecture ECP5_ARCHITECTURE.txdb \
  --bitgen-runtime ~/.cache/texo/targets/<installed ECP5 target pack> \
  --platform-profile riscv/platform-user.json
```

`--bitgen-runtime` is an installed Texo target-pack directory; Texo downloads
and verifies the pack on first use.

## Current result

| Item | Value |
| --- | --- |
| Profile | `HardwareCompactUserTestTop`, 112 / 56 MHz, Sv39 |
| Native STA | setup 0 ps, hold +1 ps after the 194 ps guards; 24,523 checked endpoints |
| JTCK | 515 setup and 515 hold checks agree |
| Run time | 2,544 s with a 3,000 s setup budget |
| Hardware | 274 ELFs pass |

## Notes

- **Logic depth matters more than cell count.** At 112 MHz the critical paths
  are in fetch translation, PMP/PMA checking and the ALU carry. Adding a single
  mux in front of the I-cache tag compare took the mapped depth from 12 to 18
  LUT levels; the registered ITLB lookup and dual tag compare brought it back to 12.
- **Starting point sensitivity.** Small RTL changes can move the post-routing
  violation count by an order of magnitude. Texo's routed re-placement and
  candidate probing exist to make closure less dependent on the initial
  placement; they are part of the default flow.
- **nextpnr comparison.** `cargo run --release -p verify-riscv --bin nextpnr-riscv -- TOP`
  exports a Struo-mapped netlist as a compressed `.stnet`, or as nextpnr JSON
  with `--nextpnr-json`, for reference runs with nextpnr-ecp5. It supports the
  core, the predictor units and the earlier board tops (`HardwareTestTop` and its
  static/bimodal/cached variants), not the compact or user-profile tops.
