# 124.8 MHz RV64 board: wide JTAG writes corrupt loaded data

Update, 2026-09-06: the [falling-edge transport repair](../../../riscv/RV64_JTAG_FIX.md)
passes all 48 offset/immediate probes and all 100 physical ELFs using eight-byte
writes at the same 124.8/62.4 MHz clocks. The runner now defaults to that exact
repaired image and eight-byte writes. The record below preserves the original
failure and byte-write workaround; the old bitstream itself is unchanged.

Observed on the EVN on 2026-09-05. The Windows D2XX bridge successfully
enumerates FT2232H interface A and detects LFE5UM5G-85 IDCODE `0x81113043`.
The CPU/memory PLL remains at 124.8/62.4 MHz throughout these experiments.

The current bitstream is
`riscv/build/riscv-hardware-cached-4k-124p8-20260905-ce8.bit`, SHA-256
`cbb8505df08ae2f0e4183278fa46673609adea3a9e35206539ab84bc9b2c0f7f`.
Its original eight-byte WRITE64 loader accepts all commands, but `alu.elf`
does not reach `tohost`. This repeats after a fresh SRAM configuration.

An `addi x31,x0,0x5a` at address zero followed by `jal x0,0` reports x31
as `0x9a`. Walking immediate bits reports `0x20 -> 0x60`, `0x40 -> 0x80`,
`0x80 -> 0x100`, `0x100 -> 0x200`, and `0x200 -> 0`.
These are stable observations, not successful instruction tests.

| Image and loading method | Immediate/position probes matching | JTAG TCK |
| --- | ---: | ---: |
| Current, eight-byte WRITE64 | 0/48 | 6 MHz |
| Current, eight-byte WRITE64 | 0/48 | 1 MHz |
| Current, one-byte masked WRITE64 | 48/48 | 6 MHz |
| Historical image, eight-byte WRITE64 | 48/48 | 6 MHz |

The position probes use immediates `32,64,128,256,512,90` at offsets
`0,4,8,12,16,20,24,28`. The historical image is
`riscv/build/riscv-hardware-test-125-texo.bit`, SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`.
It has only six implemented counters; the present eleven-counter runner's
validation failure on that image is expected and is not an ISA failure.
The minimal program on that image does reach `tohost=1`.

The workaround is to place each byte in WRITE64 data[7:0], set mask=1, and
advance its address by one. `hardware-riscv-tests` now defaults to this
method, with `--write-bytes 1` making it explicit. `--write-bytes 8` retains
the original transfer for diagnosis or separately qualified images.
This changes loading time, not the CPU clock or execution pipeline.

The physical cause within the wide payload transfer/serialization path is
not yet established. The existing STA leaves the independent JTAG clock
domain unconstrained. Its reported +64 ps setup/+290 ps hold margins do
not establish JTAG timing. Slowing TCK did not repair this failure, so a
setup-only explanation is insufficient. Do not describe this workaround
as a repair or qualification of eight-byte transfers.

The scripts here preserve the diagnostic stimulus. They assume the test
image is already programmed, ER1 is selected, and the TAP is in Run-Test/Idle
after `hardware-riscv-tests`; they deliberately do not reprogram or reset
the TAP. `probe_*.py` overwrite the small diagnostic program in RAM and
restart the CPU. Run only one board client at a time.

```sh
python3 toolchain/mre/rv64-jtag-write64/probe_offsets.py
python3 toolchain/mre/rv64-jtag-write64/probe_byte_writes.py
```

`read_debug.py` reads the current status, x31, and eleven counters without
restarting the CPU. Its `nonuniform` entries mark counters that changed
while their bits were sampled; those live values are not coherent snapshots
and must not be used as benchmark measurements.

See [the physical evidence](../../measurements/2026-09-05-rv64-124p8-hardware/)
for the original failures, comparison logs, and successful byte-loaded runs.
The explicit mapped-board diagnostic currently reaches 17,401 mapped cells
but the pinned Struo-to-Celox adapter rejects the derived board reset with
`NonScalarControl(5005)` before simulation starts. Its adapter groups FF
outputs into vectors, then requires a scalar signal for reset control.
This attempt does **not** show that the complete mapped board passes or
fails WRITE64. The test is ignored in normal runs until that adapter
limitation is fixed; reproduce the limitation with:

```sh
cargo test --locked --offline --release -p verify-riscv \
  --test hardware_jtag_write -- --ignored --nocapture
```
