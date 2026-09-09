# Veryl constant metadata consistency

Based on the published `veryl-analyzer` 0.21.0 crate, revision
`2e32d53f056eef323225a91079af92574d18f726`, package checksum
`27642985fd83ff90aeea356cbc70f63d2ac83988e1e1bae9e7b0bd63fe61fcf8`.
Only `src/conv/utils.rs` is changed; [fix.patch](fix.patch) is the upstream diff.

`eval_const_assign` already fits a numeric constant to its declared type for
`Variable` storage. However, it previously inserted the original `Comptime`
into the path table first. Constant folding then read a different value width,
signedness, and the initializer's cached expression context. The fix publishes
the fitted value and marks it for evaluation in each reference's own context.
It preserves the requirement that callers supply the correct declared type.

This is independently reproduced by ordinary HDL, without Celox `.param()`:

```veryl
module Top (a: input logic<64>, o: output logic<64>) {
    const P: bit = 64'd3;
    assign o = a & {P repeat 64};
}
```

The value of `P` is one bit (`1`), so `o` must equal `a`. A second control passes
`64'd1` to a child's `param P: bit`. Signed constants additionally verify that
subsequent uses do not retain the initializer's unsigned expression context.

The [regressions](../../../riscv/verify/tests/constant_metadata_width.rs) exercise
four HDL programs and five inputs on native, Cranelift, WASM, and interpreter.
[sources.json](sources.json) records copied source hashes. To reproduce from an
unmodified registry source directory, run `refresh.py SOURCE`; `--check` compares
in a temporary directory and does not modify either source tree.

This is narrower than the initial withdrawn analyzer experiment: it does not
reinterpret incorrectly typed caller overrides. The separate
[Celox patch](../celox-parameter-width/README.md) fixes that caller convention.
[Diagnosis and verification](../../measurements/2026-09-09-celox-parameter-width/README.md).

[Staged source audit](STAGED_REVIEW.json) compares the exact proposed commit with
the published crate archive, whose checksum matches the original Cargo.lock.
104 files are byte-for-byte identical; only `src/conv/utils.rs` changes.
The withdrawn experiment remains evidence only and is not the active patch.
