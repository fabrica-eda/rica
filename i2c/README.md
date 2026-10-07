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

## Board transport and timing

The board build does not use external SCL/SDA pins. `JtagI2cDebugTop` drives the
I2C target from inside the FPGA through the ECP5 `JTAGG` user chain: ER1 is a
two-bit I2C bit-bang command register, and the SVF transport appends one dummy
scan clock because JTAGG presents JTDI one TCK late. ER2 exposes PLL lock and
other status independently of the I2C state machine.

The board's 12 MHz clock feeds one `EHXPLLL` whose `CLKOS` is the only 250 MHz
fabric clock (4.000 ns constraint). The 12 MHz pin is sampled as data through a
two-register synchronizer instead of being promoted to a second global clock
(promoting both left overused wires). A 65,536-cycle window counts about 3,146
reference edges and reports the result in register `0x03`.

The target is a one-hot FSM with next-state logic, receive shifting, bit
counting, SDA drive and the debug bus separated. Register reads use a two-level
decode (upper nibble selects a bank, low bits an entry); a flat seven-arm read
mux routed at only 232 MHz.

With the pinned toolchain the design closes 250 MHz in Texo, and the native
bitstream passes the hardware SVF regression: scratch write `0xa5`, ID `0x52`,
protocol `0x02`, PLL lock, reset release, activity and the 250/12 MHz ratio.

```sh
cargo run --locked --offline -p verify-i2c
riscv/tools/texo pnr i2c --package CABGA381 --speed 8 \
  --lpf i2c/lfe5um5g-85f-evn-250.lpf \
  --output i2c/build/jtag-i2c-target-250.txcp \
  --synthesis-goal-mhz 400 --jtagg-prefix jtag \
  --pll-binding i2c/pll-12-to-250.json
cargo run --locked --offline -p verify-i2c --bin bitgen-i2c
cargo run --locked --offline -p verify-i2c --bin hardware-i2c
openFPGALoader -b ecp5_evn --write-sram i2c/build/jtag-i2c-target-250.bit
openFPGALoader -b ecp5_evn i2c/build/jtag-i2c-hardware.svf
```

## History

This block was the first design brought up on the board with Struo and Texo.
It exposed several toolchain defects, all fixed upstream: lowering of module
constants and dynamic packed selects, open-drain I/O binding, Texo emitting a
nonexistent `JTAG.MODE` setting, a hold ECO that kept a stale route after a
pin-only rebinding, and a Texo configuration that was functionally wrong
although nextpnr implemented the same netlist correctly. It also found that a
32-bit activity counter, retimed into a carry chain at 225 MHz and above,
violated slice adjacency; the probe now uses only a 16-bit counter.
