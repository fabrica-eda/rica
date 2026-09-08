# Celox: parameter override loses the declared bit width in repetition

Pinned Celox: `7ff0dd2b8c4a92efd9a1aabe376240c6dff3f9c9`.
A combinational module declares `param ENABLE: bit = 0`; the native builder sets
`.param("ENABLE", 1)`. With `a = 0xffffffffffffffff`, these equivalent masks differ:

```veryl
assign raw = a & {ENABLE repeat 64}; // observed 0x0000000000000001
assign boolean = a & {(ENABLE != 1'b0) repeat 64}; // observed 0xffffffffffffffff
```

No clock, FF, RAM or CPU forwarding is involved. The 10 input/enable pairs have
3 raw mismatches and zero explicit-boolean mismatches. The early Zbs unit also
fails its native oracle with the raw expression while its mapped ECP5 model
passes all 201,728 pairs. After the boolean expression is used, both pass.

The builder's `elaborate_parameterized_top` constructs each override with
`Value::new(*value, 64, false)` (`crates/celox/src/simulator/builder.rs`, line 148).
The observed low bit is consistent with repeating a 64-bit value of 1 and then
truncating to 64 bits. We have isolated the override/repetition construct; no
dependency patch or backend ownership claim is needed for the CPU workaround.
Default parameter values without an override were not characterized here.

## Reproduce

First build Rica's existing pinned release dependencies. Pass the exact Celox
rlib produced by that build to the standalone linker helper, using a fresh output
directory. The helper records the Rust source, rlib, command, binary and logs.

```sh
python3 toolchain/mre/celox-parameter-repeat/reproduce.py \
  --rlib target/release/deps/libcelox-1142c5086253b5ab.rlib \
  --output /tmp/rica-parameter-repeat-check --expect-unfixed
```

`--expect-unfixed` succeeds only when the three recorded raw mismatches occur
and the explicit-boolean control passes. Omit it to require correct raw behavior.
The artifact suffix is build-specific; the observed rlib SHA-256 is
`ccf75e0b2587bd73312672aae519707c99c54ec1afe1477295d32e40e87f23a6`.

[Recorded commands and hashes](../../measurements/2026-09-09-rv64-early-bitmanip/parameter-repeat-result.json),
[all observed values](../../measurements/2026-09-09-rv64-early-bitmanip/parameter-repeat-run.log),
[failed raw unit oracle](../../measurements/2026-09-09-rv64-early-bitmanip/simple-oracle-01.log),
[passing boolean unit oracle](../../measurements/2026-09-09-rv64-early-bitmanip/simple-oracle-02.log).

The initial failed CPU runs are excluded from IPC/performance results.
