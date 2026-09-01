# RICA RISC-V bring-up

This workspace now contains the RISC-V implementation under `riscv/` and
the I2C block used to establish host-driven hardware debug under `i2c/`.

The former fifteen-cycle 250 MHz `Core250` design was a useful synthesis,
placement, routing, BRAM, PLL, JTAGG, setup, and hold stress case, but it is
not the CPU architecture being developed here. That frozen design and its
structural regression have moved to Texo's `examples/core250-qor`.

Run the active CPU's RTL, synthesis, and mapping-equivalence regression with:

```sh
cargo run --locked --offline -p verify-riscv
```

The nominal CPU target is 125 MHz. The data-sheet-valid board PLL runs at
124.8 MHz. Performance work is measured as instruction
throughput, not Fmax alone: consecutive dependent ALU instructions sustain one
instruction per clock through registered EX bypasses, while load-, shift-, and
compare-use dependencies wait only for their producing stage. Board BRAM runs
at 62.4 MHz behind response-token handshakes and a two-line instruction buffer.

The core implements writable RV64 `mcycle`/`minstret` counters with
`PerformanceCounter64`. Custom CSRs 0x7C0/0x7C1 select and access four counters
implemented by the shared-incrementer `PerformanceEventCounters4`: selector 0
counts dependency bubbles, 1 memory waits, 2 redirects, and 3 front-end
bubbles. The current simulation suite contains eight bundled ELFs and
exercises the dual-clock memory model in both possible serializations of
coincident CPU/memory edges. The official ACT4 Zca catalog is pinned at
`f7ba06c3d1a01428b172fe851ab56931a0624fee`; all 32 generated ELFs with set digest
`8938083a2a0e5cd9ae7b74c6b1f6be9c8afec0da593b07478c8385a421e2a12e`
pass the ordinary and dual-clock RTL models.

Texo 0.1.6 revision `f8c57e09` closes the current RV64C/counter-enabled board
design at the 124.8 MHz PLL clock. Checkpoint
`riscv/build/riscv-hardware-test-125-ce8.texo.json` reports +0.011 ns setup WNS
and +0.290 ns hold WNS, both with zero TNS. It contains 20,588 mapped cells,
16,771 implemented cells, 18,497 nets, and 228,641 routed PIPs; STA checks
10,406 of 10,926 endpoints. The generated
`riscv/build/riscv-hardware-test-125-texo.bit` has SHA-256
`e53d19fd33bf89d365fb7d4b3c2b790b163fe13296906f7dc90bbb600b2a9320`.
On the EVN at 124.8 MHz this exact counter-enabled bitstream passes all eight
bundled ELFs, all 51 pinned ACT4 RV64I ELFs, and all 32 pinned ACT4 RV64Zca
ELFs. The hardware runner also reads and validates all six 64-bit performance
counters after every ELF.

Run the I2C regressions with:

```sh
cargo run --locked --release --offline -p verify-i2c
```

See [riscv/README.md](riscv/README.md) for the CPU plan and
[i2c/README.md](i2c/README.md) for the debug protocol.
