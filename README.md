# RICA RISC-V bring-up

This workspace now contains the RISC-V implementation under `riscv/` and
the I2C block used to establish host-driven hardware debug under `i2c/`.

The former fifteen-cycle 250 MHz `Core250` design was a useful synthesis,
placement, routing, BRAM, PLL, JTAGG, setup, and hold stress case, but it is
not the CPU architecture being developed here. That frozen design and its
structural regression have moved to Texo's `examples/core250-qor`.

Run the active CPU's source RTL, synthesis, and logical-mapping regression with:

```sh
cargo run --locked --offline -p verify-riscv
```

The nominal CPU target is 125 MHz. The data-sheet-valid board PLL runs at
124.8 MHz. Performance work is measured as instruction
throughput, not Fmax alone: consecutive dependent ALU instructions sustain one
instruction per clock through registered EX bypasses, while load-, shift-, and
compare-use dependencies wait only for their producing stage. Board BRAM runs
at 62.4 MHz behind response-token handshakes and a two-line instruction buffer.
The CPU frontend separately keeps a three-slot head/middle/tail fetch FIFO, so
the predictor pipeline can remain full without conflating instruction-cache
capacity with prediction-response capacity.

The core implements writable RV64 `mcycle`/`minstret` counters with
`PerformanceCounter64`. Custom CSRs 0x7C0/0x7C1 select nine implementation
events: selectors 0..3 count dependency bubbles, memory waits, redirects, and
front-end bubbles in the shared-incrementer `PerformanceEventCounters4`, while
selectors 4..8 count conditional resolutions, direction misses, BTB misses,
RAS fallbacks, and prediction repairs. The current simulation suite contains
eight bundled ELFs and exercises the dual-clock memory model in both possible
serializations of coincident CPU/memory edges. The official ACT4 Zca catalog
is pinned at
`f7ba06c3d1a01428b172fe851ab56931a0624fee`; all 32 generated ELFs with set digest
`8938083a2a0e5cd9ae7b74c6b1f6be9c8afec0da593b07478c8385a421e2a12e`
pass the ordinary and dual-clock RTL models.

The current predictor-enabled source RTL regressions and Struo's proof-signed
logical mapping transformations pass, but the board map does not yet close
physical timing. From the exact current 31,450-cell Struo map,
official nextpnr 0.11.1 static seed 5 reaches 89.598 MHz and Texo 0.1.6 reaches
-1.813 ns setup WNS; both fail the 124.8 MHz target. The nextpnr critical path
runs from an automatically retimed fetch-line bit to a TAGE write-enable with
9.161 ns of routing, while Texo's worst capture is another retimed fetch-line
register. The nextpnr result also misses the one-CPU-period memory-to-CPU
boundary guard (9.205/8.012 ns). Disabling automatic retiming makes the routed
CPU period 2.038 ns worse, so the unusual retimed path is not catalogued as a
confirmed synthesis defect. No current bitstream or hardware qualification is
claimed; the retained hashes and path breakdowns are in
[`toolchain/measurements/2026-09-02-rica-qor.md`](toolchain/measurements/2026-09-02-rica-qor.md).

### Archived pre-predictor hardware checkpoint

The following result predates the current BTB/TAGE/RAS frontend and eleven-
counter readback ABI. It is retained as provenance, not as timing or hardware
qualification of the current RTL.

Texo 0.1.6 revision `f8c57e09` closed that earlier RV64C/six-counter board
design at the 124.8 MHz PLL clock. The recorded result for the checkpoint
formerly written as `riscv/build/riscv-hardware-test-125-ce8.texo.json` was
+0.011 ns setup WNS and +0.290 ns hold WNS, both with zero TNS. It contained
20,588 mapped cells, 16,771 implemented cells, 18,497 nets, and 228,641 routed
PIPs; STA checked 10,406 of 10,926 endpoints. That ignored JSON was not retained
immutably and has since been reused by an experimental predictor build; these
numbers are provenance, not a description of the file currently on disk. The
generated
`riscv/build/riscv-hardware-test-125-texo.bit` has SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`.
On the EVN at 124.8 MHz this exact historical bitstream passed all eight
bundled ELFs, all 51 pinned ACT4 RV64I ELFs, and all 32 pinned ACT4 RV64Zca
ELFs. The then-current hardware runner also read and validated its six 64-bit
performance counters after every ELF. A timing-closed predictor-enabled
bitstream and board run are still required before claiming current physical
coverage.

Run the I2C regressions with:

```sh
cargo run --locked --release --offline -p verify-i2c
```

See [riscv/README.md](riscv/README.md) for the CPU plan and
[i2c/README.md](i2c/README.md) for the debug protocol. Confirmed synthesis,
simulation, STA, P&R, and bitgen defects are tracked separately in
[TOOLCHAIN_ISSUES.md](TOOLCHAIN_ISSUES.md).
