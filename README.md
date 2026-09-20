# RICA RISC-V bring-up

This workspace now contains the RISC-V implementation under `riscv/` and
the I2C block used to establish host-driven hardware debug under `i2c/`.

Historical measurement logs, trial scripts, machine-specific records and binary
artifacts under `toolchain/measurements/` are local archives excluded from Git.
Links and commands pointing into that directory in older reports require the
local archive; those files are not included in a fresh checkout. Maintained
comparison tools and their portable input data live under `riscv/tools/`.

The ECP5 RV64 architecture comparison, measurements, and implementation plan
are recorded in [the architecture plan](riscv/RV64_ARCHITECTURE_PLAN.md).
The current development build is scalar RV64 with a **64-entry BTB, two-bit
direction prediction, eight-entry RAS and 4 KiB instruction cache**
(`HardwareCompact4KTestTop`). The current image retains the mandatory
**124.8/62.4 MHz PLL clocks**, with **+1 ps setup / +290 ps hold** and
14,406 setup/hold checks, including 515 in the JTAG domain, in normal STA.
It passed **100/100 distinct physical ELFs**, including **118 executions**
with repeats and eleven counter checks per execution, through Windows D2XX.

The JTAG receiver now samples on falling edges and registers the control
boundary. This repairs the placement-dependent zero-response failure and the
former WRITE64 corruption. The hardware runner defaults to the native-JTCK-STA
image and **eight-byte writes**; the updated default also passed the basic eight ELFs.
CPU performance
remains within one cycle of the previous compact image on the original four
diagnostics, whose speedup over the static image was about **1.223×**.
The new timing-margin and LSU candidates failed routed timing and were not
adopted. Their implementations, results and next steps are recorded in the
[JTCK/LSU report](riscv/RV64_JTCK_LSU_PLAN.md).
See the [earlier cause, repair and physical results](riscv/RV64_JTAG_FIX.md),
[architecture/performance study](riscv/RV64_COMPACT_PREDICTOR.md), and
[repair evidence](toolchain/measurements/2026-09-06-rv64-jtag-zero/README.md).
The [static 4 KiB record](riscv/RV64_124P8_TIMING.md) remains the baseline.

The former fifteen-cycle 250 MHz `Core250` design was a useful synthesis,
placement, routing, BRAM, PLL, JTAGG, setup, and hold stress case, but it is
not the CPU architecture being developed here. That frozen design and its
structural regression have moved to Texo's `examples/core250-qor`.

Run the selected predictor's source RTL regression with:

```sh
cargo run --locked --offline -p verify-riscv --bin verify-riscv -- --rtl-only --compact-predictor
```

The mandatory board CPU PLL runs at 124.8 MHz. Performance work is measured as instruction
throughput, not Fmax alone: consecutive dependent ALU instructions sustain one
instruction per clock through registered EX bypasses, while load-, shift-, and
compare-use dependencies wait only for their producing stage. Board BRAM runs
at 62.4 MHz behind response-token handshakes. The selected board configuration
uses a synchronous L1 instruction cache; a two-line buffer is also available
for comparisons. FENCE.I invalidates instruction storage and restarts fetch.
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

### Archived September 2 predictor checkpoint

The September 2 predictor-enabled source RTL regressions and Struo's
proof-signed logical mapping transformations passed, but that board map did
not close physical timing. From that checkpoint's 31,450-cell Struo map,
official nextpnr 0.11.1 static seed 5 reaches 89.598 MHz and Texo 0.1.6 reaches
-1.813 ns setup WNS; both fail the 124.8 MHz target. The nextpnr critical path
runs from an automatically retimed fetch-line bit to a TAGE write-enable with
9.161 ns of routing, while Texo's worst capture is another retimed fetch-line
register. The nextpnr result also misses the one-CPU-period memory-to-CPU
boundary guard (9.205/8.012 ns). Disabling automatic retiming makes the routed
CPU period 2.038 ns worse, so the unusual retimed path is not catalogued as a
confirmed synthesis defect. No current bitstream or hardware qualification is
claimed for that checkpoint; the retained hashes and path breakdowns are in
[`toolchain/measurements/2026-09-02-rica-qor.md`](toolchain/measurements/2026-09-02-rica-qor.md).

### Archived pre-predictor hardware checkpoint

The following result predates the predictor frontend and eleven-
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
performance counters after every ELF. The current compact/L1I image has separate
timing and physical evidence linked above, including its byte-write loading
requirement; the historical results do not qualify its eight-byte JTAG path.

Run the I2C regressions with:

```sh
cargo run --locked --release --offline -p verify-i2c
```

See [riscv/README.md](riscv/README.md) for the CPU plan and
[i2c/README.md](i2c/README.md) for the debug protocol. Confirmed synthesis,
simulation, STA, P&R, and bitgen defects are tracked separately in
[TOOLCHAIN_ISSUES.md](TOOLCHAIN_ISSUES.md).

Veryl native testbenches run on Celox with `cargo test --locked --offline -p rica-tb`;
see [test instructions](riscv/tests/README.md). The [MRE inventory](toolchain/mre/README.md)
currently has no active reproducers; fixed cases are covered by upstream regressions.

## Nix development environment

On x86_64 or aarch64 Linux (including WSL2), enter the pinned development shell:

```sh
nix develop
cargo --version
mbx --version
cargo test --workspace --locked
```

Enable Nix flakes and `nix-command` if your installation does not already do so.
The shell supplies Rust, rustfmt, Clippy, rust-analyzer, C/C++ compilers, CMake,
pkg-config, OpenSSL, Python and mbx. Rust follows `rust-toolchain.toml`. Plain `cargo`
commands automatically run through mbx's shared build cache; no global Cargo
configuration or `mbx setup` is required. The mbx package comes from
[tignear/nix-packages](https://github.com/tignear/nix-packages), pinned in
`flake.lock`. Existing mbx user configuration is respected.

For automatic activation, install direnv, enable its shell hook, then run
`direnv allow` in this repository. FPGA target packs, released Fabrica tools,
and board programming tools still follow the project-specific instructions above.

Update the shared cache package with `nix flake update nix-packages`, or all
pinned development dependencies with `nix flake update`.
