# Celox native concat-width MRE

This MRE constructs a `FrontendArtifact` directly through Celox's public SDK.
It contains one atomic register bank with 129 direct data bits, two direct
selector bits, and one current-state XOR bit. There is no Veryl parsing,
Struo lowering, technology mapping, or serialized artifact in the reproducer.

Use a checkout whose lock file selects Celox 0.3.1. For example, copy the
source into a Struo checkout and let Cargo discover it as a local example:

```sh
mkdir -p /path/to/struo/crates/struo-celox/examples
cp /path/to/rica/toolchain/mre/celox-native-concat-width/native_concat_width.rs \
  /path/to/struo/crates/struo-celox/examples/celox-native-concat-width.rs
cd /path/to/struo
cargo run --release --offline -p struo-celox \
  --example celox-native-concat-width
```

Without arguments the program treats the exact known failure as a successful
reproduction. On Celox 0.3.1 it reports:

```text
129-bit data artifact.validate: PASS
129-bit data build_wasm: PASS
128-bit data build_native control: PASS
129-bit data build_native: EXPECTED FAILURE
JIT Code generation error: native merged-chain optimization: during native jump inlining: SIR verify [TYPE.CONCAT_WIDTH] at b0/i5: after single-predecessor inlining: concat operands total 4 bits, r134 has width 132
```

The one-bit threshold is intentional: changing `artifact(129)` to
`artifact(128)` reduces the bank from 132 to 131 bits and native compilation
passes. Because artifact validation and the WASM backend both accept the same
132-bit object, the failure belongs to the Celox native optimizer rather than
the Struo-to-Celox adapter.

To verify a candidate fix, use the same command with `--expect-fixed` after the
Cargo separator:

```sh
cargo run --release --offline -p struo-celox \
  --example celox-native-concat-width -- --expect-fixed
```

That mode succeeds only when the 132-bit native build succeeds. It therefore
cannot silently turn a repaired compiler into a failed reproduction.
