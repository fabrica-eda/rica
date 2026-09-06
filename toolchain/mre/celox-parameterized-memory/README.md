# Parameter declaration changes the memory simulation

**Resolved locally on 2026-09-06:** Celox was updated from 0.4.2 to the official
0.4.5 revision `9aa2457811b3ef2b002e90d2ae4f5aafb0195099`. The update alone
failed identically. Three fixes are now applied through the
[vendored crates and upstream patch](../../vendor/celox/README.md).
The source pair passes all 24 backend/optimization combinations with these
fixes. [Evidence and validation](../../measurements/2026-09-06-celox-memory-fix/README.md)
includes the before/after logs and negative controls.

Submitted upstream as [Celox PR #755](https://github.com/celox-sim/celox/pull/755),
including the subsequent FF optimization. The pinned local implementation is
retained while upstream review is pending.

A subsequent [FF optimization and performance comparison](../../measurements/2026-09-06-celox-ff-performance/README.md)
removes unnecessary staging through dependency scheduling and native StateSSA
promotion. There are now four patched crates. The original three correctness
fixes remain in place, including the merged-phase fallback for other frontends.

## Reproducer

The complete original `HardwareMemory` is included. Its SHA-256 is
`5cd2b9f6030feaab11a218c4c5d8e491280f05be42795c7ccebd0c449fa84fe6`.
The harness changes only `module HardwareMemory (` into
`module HardwareMemory #(param FAST_LSU: u32 = 0,) (`. The new parameter is
unused and is not overridden. Both versions receive the same consecutive
debug-byte writes, followed by instruction/data-port reads. All input ports
are explicitly initialized. Each edge receives one stable address/data pair.

Run from the Rica root:

```sh
cargo run -p verify-riscv --example celox-parameterized-memory --locked --offline
cargo run -p verify-riscv --example celox-parameterized-memory --locked --offline -- --matrix
cargo test -p verify-riscv --test hardware_memory_debug_stream --locked --offline
```

The first command checks native and WASM at default optimization. `--matrix`
checks native x86, Cranelift, interpreter and WASM at O0/O1/O2 for both sources.
Both commands fail if **any** comparison fails. The normal Rica regression
also checks the active memory with/without the parameter, native and WASM,
and all three optimization levels.

## Version update and historical observations

Both pinned 0.4.2 (`7bb38a0ce4d66f349d499b270d27375d7c1a01bb`) and unpatched
0.4.5 return this table at default optimization. The expected first word is
`0x3e373029221b140d`:

| Backend | Original source | Unused parameter |
| --- | --- | --- |
| Native | correct | `0x453e373029221b14` |
| WASM | `0x3e373000221b140d` | `0x453e003029221b14` |

A previous native check at local Celox 0.4.4 commit
`4342678d626ea6f70d93093668b93d79f2dce936` also reproduced the shift.
The earlier conclusion that ownership was unresolved describes that previous
investigation, not the current result.

For historical reproduction only, `--expect-unfixed` requires the exact
recorded native original/pass and parameter/failure pair. It must fail with
the fixed dependency. Success with that flag means the old defect was
reproduced, not that a CPU model passed.

## Isolated causes

1. **Shared-clock FF event assembly:** the ordinary `Simulator.tick()` path
   concatenated each reset group's evaluation and publication. One group
   therefore read another group's newly committed value on the same edge.
   In this memory, the translated address retained the old registered address
   while the RAM sampled the next debug byte. Adding the unused parameter
   changed variable IDs and map ordering, exposing the error. The fix puts
   **all evaluations before all publications** for a shared event and merges
   them into one optimizer unit. The already scheduled fused comb/FF path was
   not the failing path.
2. **WASM destination coverage:** optimization coalesced four 15-bit row
   fields into a 60-bit store starting at bit 15. Its seven-bit shift covers
   67 bits from the starting byte, but the scalar read-modify-write kept only
   64 bits. A row bit used to choose a BRAM bank was lost. The fix selects the
   wider existing store path whenever `bit_shift + width > 64`.
3. **Native padded arrays at O0:** mandatory late native optimization still
   runs at O0. It coalesced eight one-bit FF stores into one eight-bit store
   after choosing byte-strided storage. The generated store wrote the first
   element's byte instead of all eight elements. Static aggregate loads and
   stores now split at logical element boundaries before instruction selection.

Small independent tests are included in `fixes.patch`: two swapping FFs with
separate reset groups, narrow arrays, and a shifted-store boundary test.
The backend test covers value and unknown-mask planes and adjacent bits.
In the correctness-only baseline, removing each fix separately made its
corresponding test fail again; restoring the fixes made them pass. Thus the original one-byte shift is
attributed to event assembly, with two additional backend defects found by
the expanded checks.

## CPU and hardware scope

The LSU prototype was rejected independently for routed timing. The historical
failed CPU run supplies no IPC result. The new validation replays both the
normal CPU and archived `FAST_LSU=0` CPU in an isolated copy, using the fixed
dependency and the same 100 ELFs with both coincident-clock orders.
Its logs and exact source/dependency hashes are in the new evidence directory.
The normal 31 RTL sources remain the qualified native-JTCK baseline.

These fixes address simulation correctness. They do not establish a faster
FPGA clock, higher IPC, or physical qualification of an LSU candidate.
The earlier physical measurements remain in the immutable
[native-JTCK evidence](../../measurements/2026-09-06-rv64-native-jtck/).
