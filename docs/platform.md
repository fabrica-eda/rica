# Board platform

This page describes the platform around the core as implemented by
`HardwareTestTop` and its board wrappers on the LFE5UM5G-85F-EVN.

## Clocks

The board's 12 MHz reference drives one EHXPLLL with a 12 MHz phase detector.
The CPU and memory clocks are integer divisions of the VCO with zero relative
phase. The memory clock also drives the timer.

| VCO template | VCO | Profiles built on it (CPU / memory) |
| --- | --- | --- |
| `riscv/pll-12-to-112.json` | 672 MHz | 112 / 56 MHz (working profile) |
| `riscv/pll-12-to-108.json` | 648 MHz | 108 / 54 MHz |
| `riscv/pll-12-to-124p8.json` | 624 MHz | 124.8 / 62.4 MHz (M-mode profiles), 104 / 52 MHz |
| `riscv/pll-12-to-84.json` | 588 MHz | 84 / 84 MHz (an earlier U-mode reference) |

The VCO templates come unchanged from `ecppll` (via
`riscv/tools/pll_template_from_ecppll.py`); the output dividers are set from the
profile. The working profile
[`riscv/platform-user.json`](../riscv/platform-user.json) selects the clocks, and
`python3 riscv/tools/user_platform.py --write` regenerates `riscv/pll-user.json`
and the RTL constants in `UserPlatform.veryl` from it (without `--write` it only
checks that they agree). Exactly 125 MHz is not used: it needs an input divider
that puts the phase detector below its rated range.

JTCK, the JTAG clock, is constrained at 6 MHz
([`riscv/jtck-clock-constraints.json`](../riscv/jtck-clock-constraints.json)).

## Memory map

Physical addresses are 32 bits. With `STRICT_MEMORY=1` (the working profile)
everything outside these regions, including any address with nonzero upper
32 bits, raises an access fault before a request is issued.

| Range | Data | Fetch | Contents |
| --- | --- | --- | --- |
| `0x0000_0000`–`0x0000_0fff` | Read/write | Execute | 4 KiB reset region (trampoline) |
| `0x0200_0000`–`0x0200_ffff` | Naturally aligned only | Access fault | Timer, software interrupt, console |
| `0x8000_0000`–`0x8003_efff` | Read/write | Execute | 252 KiB main RAM |

The 256 KiB BRAM holds main RAM; the reset region is stored in its final 4 KiB.
The JTAG loader writes a short reset trampoline below `0x1000` that jumps to
the ELF entry, normally `0x8000_0000`. Accesses that cross a region boundary
fault and write nothing; `mtval` holds the first inaccessible byte.

## Timer and software interrupt

| Address | Register | Access / reset |
| --- | --- | --- |
| `0x0200_0000` | MSIP | Bit 0 read/write, other bits zero; reset 0 |
| `0x0200_4000` | MTIMECMP | 64-bit read/write; reset all ones |
| `0x0200_bff0` | Timebase frequency | 64-bit read-only (the memory clock, e.g. 56,000,000) |
| `0x0200_bff8` | MTIME | 64-bit read/write; reset 0; increments every memory clock |

Aligned 64-bit accesses to MTIME and MTIMECMP are atomic. Smaller accesses touch
only the addressed byte lanes. Misaligned MMIO accesses raise access faults
(5 for loads, 7 for stores); reserved slots read zero and ignore writes. MTIP is
a registered `mtime >= mtimecmp`, so handlers must tolerate a short pending tail
after raising MTIMECMP. Holding the CPU in reset resets the timer and MSIP but
preserves RAM. The board freezes MTIME when a test writes a nonzero `tohost`
result, together with the performance counters.

This is not a complete CLINT/ACLINT, and there is no PLIC.

## JTAG console

With `ENABLE_CONSOLE=1` a byte console is available over the board's USB/JTAG
connection at `0x0200_1000` (the EVN's UART pins are not populated by default).

| Offset | Register | Behavior |
| --- | --- | --- |
| `0x00` | TX_DATA | A write of the low byte sends one byte; when full it is dropped and TX overflow is set |
| `0x08` | RX_DATA | A read including the low byte receives one byte; bit 8 is valid; reads 0 when empty |
| `0x10` | STATUS | Bit 0 TX space, bit 1 RX valid, bit 2 TX pending, bit 3 RX space |
| `0x18` | IRQ_ENABLE | Bit 0 enables the RX interrupt |
| `0x20` | ID | Read-only `0x434f4e01` |
| `0x28` | COUNTS | Bits 4:0 TX count, bits 12:8 RX count |
| `0x30` | ERRORS | Bit 0 TX overflow, bit 1 RX overflow; write 1 to clear |
| `0x38` | — | Reserved |

Both FIFOs hold 16 bytes. RX data with IRQ_ENABLE set drives MEIP; draining the
RX FIFO or clearing IRQ_ENABLE acknowledges it. CPU reset clears the FIFOs,
interrupt enable and errors. After `tohost` freezes the counters the host can
still drain remaining output.

## JTAG debug transport

`JtagDebugTransport.veryl` connects the ECP5 `JTAGG` user chain ER1 to the
board. The host shifts exactly 128 bits, LSB first, per command: 8-byte masked
memory writes, CPU run/reset control, console exchange (ER1 opcode 5), and
response selection. JTDO1 returns one stable selected response bit per scan.
Response banks 5 and 6 expose the low and high halves of the eleven performance
counters (selector in SELECT address bits 11:8), and bank 7 holds console
exchange results. The protocol version is `0x05`.

The receiver samples JTAGG on falling JTCK edges, registers CE/SHIFT to align
with JTDI (which arrives one TCK late), treats UPDATE as data in the JTCK domain,
and accepts each UPDATE once. Commands cross into the CPU domain through a
held mailbox and a toggle synchronizer. RAM writes are rejected while the CPU
runs.

Only a terminal `tohost` store acknowledged by memory is captured as a result;
the frozen counter snapshot includes that store's `minstret` increment.

## Board LEDs

The `Top` wrapper's standalone ROM increments x31 and loops; LEDs D5–D10 show
x31[5:0] (active low), D11 toggles with retired instructions, and D12 is lit
while the PLL is locked. The CPU leaves reset when the PLL locks.
