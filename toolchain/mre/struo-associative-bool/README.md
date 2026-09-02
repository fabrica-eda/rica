# Struo associative Boolean lowering MRE

This MRE compares registered sixteen-input `&&` and `||` chains written in
ordinary left-associated source form with the same functions written as
explicit balanced trees. Registering every input and output isolates the
Boolean cone as a register-to-register path.

On Struo `68be24c`, both left-associated forms lower to synthesis depth 15 and
map to depth 5 at 3,320 ps. Both explicit trees lower to depth 4 and map to
depth 2 at 1,820 ps. All four designs contain 22 mapped cells, so the gap is
association alone rather than an area tradeoff. The frontend recursively
preserves Veryl's left-associated binary AST, while synthesis only balances
dedicated reductions and equality comparisons.

`assert_mre.rs` stops after ECP5 mapping and mechanically requires ordinary
source to match the explicit tree in synthesis depth, mapped depth, period,
and cell count. Install it as a local Struo example and run it against the
retained source:

```sh
cp /path/to/rica/toolchain/mre/struo-associative-bool/assert_mre.rs \
  /path/to/struo/crates/struo-cli/examples/rica-associative-bool.rs
cd /path/to/struo
cargo run --release --offline -p struo-cli \
  --example rica-associative-bool -- \
  /path/to/rica/toolchain/mre/struo-associative-bool/src/bool_mre.veryl
```

The command fails on `68be24c` with the 15/5-depth result. A corrected lowerer
prints identical metrics for each pair and exits successfully.

[Struo PR #83](https://github.com/fabrica-eda/struo/pull/83) contains the
validated frontend fix. It flattens consecutive source-level associative
operators, booleanizes every operand once, and emits a balanced reduction when
that changes synthesized expression depth by at least two levels. The depth
gate is intentional physical-QoR policy: a full-board control run that
re-associated every eligible short chain stopped with three overused wires,
whereas the selective candidate routed and materially improved both setup WNS
and TNS. This sixteen-input reproducer exceeds the threshold, so it still
requires the complete 4/2-depth, 1,820 ps, 22-cell result.
