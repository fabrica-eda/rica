# Parameter declaration changes the memory simulation

This retains a source-RTL counterexample found while evaluating the RV64 LSU.
The complete original `HardwareMemory` is included; it has not yet been reduced
to a minimal circuit. Its SHA-256 is
`5cd2b9f6030feaab11a218c4c5d8e491280f05be42795c7ccebd0c449fa84fe6`.

The harness changes only `module HardwareMemory (` into
`module HardwareMemory #(param FAST_LSU: u32 = 0,) (`. The new parameter is
unused and is not overridden. Both versions receive the same consecutive
debug-byte writes, followed by instruction/data-port reads. All input ports
are explicitly initialized. Each edge receives one stable address/data pair.

Run from the Rica root:

```sh
cargo run -p verify-riscv --example celox-parameterized-memory \
  --locked --offline -- --expect-unfixed
```

On pinned Celox `7bb38a0ce4d66f349d499b270d27375d7c1a01bb`, the first data
word must be `0x3e373029221b140d`:

| Backend | Original source | Unused parameter |
| --- | --- | --- |
| Native, default optimization | correct | `0x453e373029221b14` |
| WASM, default optimization | `0x3e373000221b140d` | `0x453e003029221b14` |

The native parameterized result takes each byte from the following write.
The WASM results also have a missing byte. These observations establish a
simulation discrepancy, but do not isolate it to the Veryl frontend,
scheduler, optimizer, or a backend. A separate native check of the local
Celox 0.4.4 tree at `4342678d626ea6f70d93093668b93d79f2dce936` reproduced
the parameterized failure. That dependency update was not adopted.

The default command exits unsuccessfully if any comparison fails.
`--expect-unfixed` requires the exact recorded native control/pass and
parameter/failure pair; a successful exit with that flag is reproduction of
the defect, not a functional qualification. It also prints the WASM results.
The regression for the retained original native memory is
`riscv/verify/tests/hardware_memory_debug_stream.rs`.

The LSU prototype was rejected independently for routed timing and moved out
of normal RTL sources. CPU-model IPC from the failed parameterized simulation
is not used as a performance result. The qualified bitstream was tested on
the actual ECP5 and is unaffected by this software-simulator comparison.

Logs, discarded prototype sources and the complete physical comparison are in
[`../../measurements/2026-09-06-rv64-native-jtck`](../../measurements/2026-09-06-rv64-native-jtck/).
