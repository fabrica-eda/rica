# Running on hardware

Rica runs on the Lattice **LFE5UM5G-85F-EVN**. The board's on-board FT2232H
(interface A) provides JTAG; the host loads programs into BRAM, starts the CPU
and reads results through the JTAG debug transport
([platform.md](platform.md#jtag-debug-transport)). No extra wiring is needed.

Bitstreams are build outputs and are not stored in the repository; produce one
with [fpga-flow.md](fpga-flow.md).

## Recommended setup: tapir

[tapir](https://github.com/fabrica-eda/tapir) programs the FPGA's SRAM and
serves JTAG over the Xilinx Virtual Cable (XVC) protocol:

```sh
tapir program-sram riscv/build/IMAGE.bit
tapir xvc --listen 127.0.0.1:3721        # keep running in another terminal
cargo build --locked --release -p verify-riscv
target/release/hardware-riscv-tests --xvc 127.0.0.1:3721 --no-program ELF...
```

`tapir detect` lists FT2232 devices. Only one process can own the device, so
stop a running `tapir xvc` before programming again.

### WSL2

Forward the FT2232H into WSL with [usbipd-win](https://github.com/dorssel/usbipd-win)
from Windows: share it once (`usbipd bind --busid BUSID`, as administrator), then
attach it (`usbipd attach --wsl --busid BUSID`). The device has to be attached
again after it is unplugged or after a USB error. `usbipd list` shows the bus ID
(VID:PID `0403:6010`).

Alternatively, keep the FTDI D2XX driver on Windows and run
`riscv/tools/windows_d2xx_xvc.py` with Windows Python. It serves XVC from
interface A on loopback, which WSL can reach in its default configuration:

```powershell
python.exe \\wsl.localhost\DISTRO\path\to\rica\riscv\tools\windows_d2xx_xvc.py `
  --listen 127.0.0.1 --port 3721 --device-index 0
```

In that case, program with `hardware-riscv-tests --xvc 127.0.0.1:3721 --bitstream IMAGE.bit`,
which programs through openFPGALoader's XVC client.

## The hardware runner

| Option | Meaning |
| --- | --- |
| `--xvc HOST[:PORT]` | Use an XVC server for JTAG |
| `--no-program` | Keep the image already in SRAM |
| `--bitstream PATH` | Image to program (the default is an old M-mode console image, so pass this or `--no-program`) |
| `--act4`, `--act4-suite I\|M\|Zca\|Zba\|Zbb\|Zbs` | Run a generated ACT4 catalog |
| `--kernel-console` | Drive the console kernel's interactive protocol (needs `--xvc`) |
| `--expect-tohost VALUE` | Expect a value other than 1 in `tohost` |
| `--write-bytes 1\|2\|4\|8` | Bytes per masked write frame (default 8; smaller sizes only for old images) |
| `--run-tck N` | Length of the CPU run window in JTCK cycles |
| `ELF...` | Programs to run; without ELFs the eight bundled smoke tests run |

When the runner programs the board itself it calls `openFPGALoader`
(override with `OPENFPGALOADER`).

For each ELF the runner checks the debug identity, loads the program, starts
the CPU, waits for `tohost`, checks the result and reads all eleven performance
counters:

```text
METRIC sv39-fetch  mcycle=33696 minstret=8872 dependency=... memory=... redirect=... frontend=... conditional=... direction_wrong=... btb_miss=... ras_fallback=... repair=... CPI=...
PASS sv39-fetch    hardware tohost=0x0000000000000001
```

ELFs must be little-endian RV64 with a `tohost` symbol and fit in the reset
region (`0x0000_0000`–`0x0000_0fff`) and main RAM (`0x8000_0000`–`0x8003_efff`).
A test reports `1` for pass and an odd value for failure.

## Validating an image

The qualification run for the working profile is the full validation set: ACT4
(136 ELFs: RV64I, Zca, M, Zba, Zbb, Zbs), the project's assembly suites and
diagnostics, the privileged and Sv39 tests, and the kernels. For example, after
programming:

```sh
R=target/release/hardware-riscv-tests
$R --xvc 127.0.0.1:3721 --no-program --act4
$R --xvc 127.0.0.1:3721 --no-program --act4 --act4-suite Zca
$R --xvc 127.0.0.1:3721 --no-program riscv/build/user-kernel/*.elf
```

Rebuild timer-dependent ELFs (`build_rv64_machine_interrupts.py`, the kernels)
for the profile's timebase before running them. Compare `mcycle`/`minstret` with
the simultaneous-clock model (`verify-riscv --timed-clocks`); fixed-work tests
match exactly or within a few cycles. Timer-driven kernels retire different
instruction counts at different clock frequencies.

## Troubleshooting

- `found 0 FT2232 devices`: the adapter is not attached (re-run `usbipd attach`
  under WSL) or another process (an old `tapir xvc`) holds it.
- `failed to fill whole buffer` / `USB write failed` in the middle of a run: the
  USB link dropped. Re-attach, program again and rerun the whole suite.
- A test that never writes `tohost` times out; increase `--run-tck` only for
  genuinely long programs.
