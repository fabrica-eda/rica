# Celox top-level parameter typing fix

Rica applies this facade patch to Celox
`7ff0dd2b8c4a92efd9a1aabe376240c6dff3f9c9` (0.4.6).

The normal Veryl instantiation path calls `eval_expr` with the formal parameter
type before `Context::push_override`. Celox instead supplied an unsigned 64-bit
`Comptime` for every `.param(name, u64)` value, including a declared `bit`.
This violated the caller convention and made repetition use the wrong width.

The facade resolves parameter types/defaults in declaration order and passes
correctly sized and signed values. It uses the normalized value in both the
specialization signature and override map. A fresh header context handles
dependent widths regardless of builder call order and preserves child scope.
Narrow values are truncated; wide parameters receive the zero-extended u64 API
value before adopting the declared signedness.

Only `celox/src/simulator/builder.rs` changes in production source. Other Celox
crates remain at the original Git revision. Cargo applies this facade to both
Rica and Struo's dependency. [fix.patch](fix.patch) includes the source change and
four-backend caller regression for upstream review. `refresh.py` regenerates the
facade from a patched checkout at the pinned revision; `--check` only compares.
[sources.json](sources.json) records the copied source hashes.

A separate [Veryl constant metadata fix](../veryl-const-value/README.md) addresses
a failure reproduced without `.param()`. It does not replace the caller fix or
make passing an incorrectly typed `Comptime` valid. Both changes run during model
elaboration; no FF scheduler or execution backend changes.

[Diagnosis and verification](../../measurements/2026-09-09-celox-parameter-width/README.md).
