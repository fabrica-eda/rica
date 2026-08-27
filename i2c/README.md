# Debug I2C target

`src/I2cTarget250.veryl` implements a register-oriented I2C target at 7-bit
address `0x42`.

## Register protocol

- Write: `START`, address+W, 8-bit register address, one or more data bytes,
  `STOP`.
- Read: write the register address, issue a repeated `START`, address+R, then
  read one or more bytes.
- The register address increments after every transferred data byte.
- The target supports ACK/NACK but does not stretch SCL.

The internal debug interface is:

- `debug_address`: currently selected 8-bit register.
- `debug_write_data` and `debug_write_strobe`: one-cycle write transaction.
- `debug_read_data`: combinational value returned for `debug_address`.

`I2cDebugTop` provides the first register-bank integration test:

| Address | Access | Value |
|---|---|---|
| `0x00` | read-only | device ID `0x52` |
| `0x01` | read-only | protocol version `0x02` |
| `0x02` | read-only | I2C target address `0x42` |
| `0x03` | read-only | clock status |
| `0x10` | read/write | LED scratch register |

Scratch register `0x10` drives `led[7:0]`, providing an immediate write check
before a CPU is connected. Its external SCL/SDA ports are retained as the
open-drain mapper regression; they are not the intended PC transport on the
evaluation board.

SCL and SDA pass through two synchronizer stages. SDA is represented as
`sda_i` plus `sda_drive_low`; the target never drives high. After ECP5 mapping,
the verifier binds that pair to one physical `sda` inout. Struo emits a
`TRELLIS_IO` in bidirectional mode whose fabric output is fixed low and whose
tristate input releases the pin. Hardware still requires an external pull-up
when this standalone boundary is used. The board build transports I2C commands
through JTAGG and does not use an external SDA pin.

Clock status bits are:

| Bit | Meaning |
|---|---|
| 0 | EHXPLLL is locked |
| 1 | reset has been synchronously released in the 250 MHz domain |
| 2 | the 250 MHz activity counter has advanced |
| 3 | the 65536-cycle frequency-ratio window completed |
| 4 | the sampled 12 MHz edge count is within the 250/12 MHz tolerance |
| 5 | free-running heartbeat |

## Verified behavior

The Rust verifier checks the PLL/reset/frequency-ratio probe, a foreign-address
NACK, a two-byte sequential write,
a register-pointer write followed by repeated START, a two-byte sequential
read, and master ACK/NACK handling. It separately simulates `I2cDebugTop`,
writes `0x3c` to scratch register `0x10`, checks the LED value, and reads the
device ID and protocol version back over I2C. It then runs Struo frontend,
synthesis, and ECP5 mapping with a 250 MHz optimization goal, asserts that
open-drain binding removed both split logical ports, and checks the physical
port direction and `TRELLIS_IO`. A second mapping binds the JTAG interface to
one dedicated `JTAGG` cell and verifies that `texo-struo` preserves its
ER1/ER2 configuration and one `EHXPLLL` with a 4.000 ns generated-clock
constraint.

## Timing-oriented structure

The initial monolithic FSM produced 963 Boolean nodes, 625 ECP5 cells, and a
mapped estimate of 8.72 ns. The current one-hot implementation separates
next-state logic, receive shifting, bit counting, SDA drive control, and the
debug bus. With Struo `eea0fe44`, the standalone debug top reports 443 nodes,
68 RTL registers and 244 mapped cells. Its 250 MHz mapping estimate improves
the critical combinational path from 4.22 ns to 3.72 ns.

The board debug transport uses the on-board FT2232 JTAG connection and the
ECP5 `JTAGG` user scan chain. TCK/TDI/TDO/TMS remain dedicated JTAG pins; SCL
and SDA are generated inside the fabric by a JTAG-to-I2C bridge and therefore
do not receive ordinary LPF pin assignments or external pull-ups.
`JtagI2cDebugTop` implements ER1 as a two-bit I2C bit-bang command register.
JTAGG exports `JTDI` one TCK late, so the SVF transport appends one dummy scan
clock after the two command bits. The host command API remains two bits.
The board's 12 MHz clock drives an `EHXPLLL`; `CLKOS` is the sole 250 MHz
fabric clock. The 12 MHz pin is sampled as asynchronous data in that domain,
so it is not promoted to a second global clock. A 65536-cycle window observes
about 3146 reference rising edges and publishes the result at register `0x03`.

The final Texo P&R run closes the 4.000 ns constraint at **+84 ps setup
slack and +9 ps hold slack**. Native bitgen produces the final ECP5 bitstream
through Texo's public Cargo API. The hardware SVF passed after SRAM programming:
scratch write `0xa5`, ID `0x52`, protocol `0x02`, PLL lock, synchronized reset
release, activity, ratio-window completion, and the measured 250/12 ratio.

The workspace uses git-pinned Cargo dependencies because a release artifact is
unavailable. Texo `bdfb4acb` includes Struo `eea0fe44`, the required PLL
binding, the fix for releasing stale routes when hold ECO rebinds a packed FF
physical pin, and the timing-driven placement portfolio.

```sh
cargo run --locked --offline -p verify-i2c
cargo install --git https://github.com/fabrica-eda/texo.git \
  --rev bdfb4acb5aad08e91e3dea3a5fcbc84efd3c3ae7 \
  --root target/texo-git --locked texo-cli
target/texo-git/bin/texo pnr i2c --package CABGA381 --speed 8 \
  --lpf i2c/lfe5um5g-85f-evn-250.lpf \
  --output i2c/build/jtag-i2c-target-250.texo.json \
  --synthesis-goal-mhz 400 --jtagg-prefix jtag \
  --pll-binding i2c/pll-12-to-250.json
cargo run --locked --offline -p verify-i2c --bin bitgen-i2c
cargo run --locked --offline -p verify-i2c --bin hardware-i2c
openFPGALoader -b ecp5_evn --write-sram i2c/build/jtag-i2c-target-250.bit
openFPGALoader -b ecp5_evn i2c/build/jtag-i2c-hardware.svf
```
