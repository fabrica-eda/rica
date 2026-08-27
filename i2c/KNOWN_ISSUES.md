# I2C bring-up findings

## Resolved by Struo `dc80755`

The core intentionally keeps `sda_i` and `sda_drive_low` separate for two-state
simulation. Struo now binds them after mapping with:

```rust
mapped.bind_open_drain_ios(&[
    OpenDrainIo::new("sda", "sda_i", "sda_drive_low")
])?;
```

The resulting nextpnr JSON has one `inout sda` and a `TRELLIS_IO` configured as
`BIDIR`. The fabric output is constant zero, so it cannot actively drive SDA
high. The verifier checks these properties rather than merely checking that
mapping returned success.

The same revision also resolves both frontend findings below:

- module constants used by FSM state expressions are lowered;
- dynamic packed selects are lowered instead of reporting the unrelated
  `non-constant replication count` diagnostic.

The I2C read datapath now exercises the latter directly:

```veryl
read_next_low = !read_data_latch[bit_count - 3'd1];
```

At seed 1, changing the previous seven-term fixed-index expansion to this
dynamic select increased the mapped cells from 181 to 188 but improved routed
Fmax from 252.02 MHz to 289.35 MHz. This is retained as a synthesis/mapping QoR
regression, not rewritten back into hand-expanded logic.

## Debug register mux timing finding

Adding the first real register bank initially expressed reads as one flat
seven-arm case over the complete 8-bit address. Struo mapped the complete top
to an estimated 4.22 ns, and nextpnr routed it at 232.34 MHz: a real 250 MHz
failure. The critical path was:

```text
register_index -> flat address/read mux -> read_data_latch
```

Splitting the same decode into an upper-nibble bank selection and lower 2-bit
entry selection reduced the mapped estimate to 3.80 ns and routed at 265.25 MHz
with the same nextpnr options and seed. This was an RTL structure problem, not
an open-drain I/O problem. The hierarchical decode is retained as the timing
regression case.

## JTAGG user-chain findings

The intended PC transport is the on-board FT2232 JTAG channel, not external
SCL/SDA header pins. The dedicated JTAG pins terminate in the ECP5 JTAG TAP;
fabric logic reaches them through the `JTAGG` primitive and one of its ER1/ER2
user scan chains. A small bridge can then bit-bang the I2C target entirely
inside the FPGA.

Struo `eea0fe44` binds the eleven logical JTAG ports to one dedicated `JTAGG`
cell. Texo `3c01ca56` imports that cell and the user-bound `EHXPLLL`. The final
flow reports +84 ps setup slack and +9 ps hold slack at 250 MHz.

The earlier bitgen failure is resolved. Texo no longer emits the nonexistent
`JTAG.MODE` enum, and the intended ER1/ER2 configuration survives the
checkpoint and generated Trellis configuration:

```text
enum: JTAG.ER1 ENABLED
enum: JTAG.ER2 ENABLED
```

The public `texo_cli::bitgen` Cargo API generates the bitstream without manual
configuration edits. In the PLL build ER2 exposes `pll_locked` independently
of the I2C state machine.

### 250 MHz hardware clock

Texo adds user-owned PLL boundary binding. The board's 12 MHz input
is divided/multiplied to 250 MHz by one `EHXPLLL`; its `CLKOS` output carries a
4.000 ns generated-clock constraint. PLL-only P&R passed with +648 ps setup and
+427 ps hold slack.

Promoting both the 12 MHz input and 250 MHz PLL output to DCCA networks in the
integrated design left 13 overused wires. The implemented architecture instead
uses only 250 MHz as a fabric clock and samples the 12 MHz level through a
two-register synchronizer. A 65536-cycle measurement sees 3142--3149 rising
reference edges as valid; the ideal value is about 3146.

The first 32-bit activity counter also exposed a Struo/Texo integration issue:
at synthesis goals of 225 MHz and above, retiming converted it into a carry
chain whose FCI endpoints violated slice adjacency and could not route. Four
8-bit segments avoided that transform; the final minimal probe needs only the
low 16 bits and removes the unused upper segments entirely.

## Resolved by Texo `299836dd`: hold-ECO stale pin binding

Once setup closed, Texo released dedicated LUT-to-FF pairs to insert minimum
delay for hold repair. Releasing a pair can rebind the FF data pin from the
dedicated `DI` path to general-routing `M` without moving the BEL. The route
freeze code detected BEL moves but not this pin-only change, retained the old
route, and failed with:

```text
invalid routing constraint: route arc ends away from sink pin
```

Texo `299836dd` releases routes whose driver or sink physical pin binding
changed and includes a regression test for rebound physical pins. On this
design the hold ECO progresses from -116 ps to +9 ps while preserving +84 ps
setup slack. P&R now completes without a local patch, and the resulting native
bitstream passes the full hardware SVF regression.

## Resolved by Texo `522d7caf`: routed-configuration functional mismatch

Hardware comparison with Texo `c3b1cb3f` on the same Struo-mapped
`JtagI2cDebugTop` netlist found a functional mismatch after implementation.
The test writes I2C pointer
`0x01` through ER1 and reads `register_index[0]` through an independently
validated ER2/JTDO2 path.

- Texo P&R plus Texo native bitgen: pointer bit 0 remains zero.
- Texo P&R configuration repacked with `ecppack`: pointer bit 0 remains zero.
- nextpnr-ecp5 plus `ecppack`: pointer bit 0 becomes one and the test passes.

Repacking the Texo textual configuration with a different encoder rules out
the Texo native bitstream codec. Using byte-for-byte the same Struo mapped
netlist with nextpnr rules out the RTL, Celox, and Struo mapping stages. The
remaining fault boundary is Texo implementation/configuration generation.

Texo `522d7caf` preserves the legal JTAG and carry configuration. Its native
P&R and bitgen now pass the pointer test and the complete hardware regression.
The repaired run closes 250 MHz with +106 ps setup and +378 ps hold slack.

The nextpnr comparison also passed the complete hardware regression: write
`0xa5` to scratch register `0x10`, read device ID `0x52`, ACK it, then read
protocol version `0x01`. At a 250 MHz constraint it reports a 280.19 MHz
system-clock Fmax. This nextpnr path is retained as a diagnostic oracle, not as
the intended production flow.
