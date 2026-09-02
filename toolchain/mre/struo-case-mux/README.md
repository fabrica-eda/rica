# Struo Veryl case-mux serialization MRE

This MRE compares registered 64-bit 9- and 16-way `case` muxes with explicit
balanced selector-bit trees. Registering every input and output isolates the
mux cone as a register-to-register path.

`map_mre.rs` is a small Struo API harness that stops after ECP5 mapping, so
post-map simulation cannot obscure the lowering result. Install it as a local
example in the Struo checkout, then regenerate each mapped JSON:

```sh
mkdir -p /path/to/struo/crates/struo-cli/examples
cp /path/to/rica/toolchain/mre/struo-case-mux/map_mre.rs \
  /path/to/struo/crates/struo-cli/examples/rica-case-mux.rs
cd /path/to/struo
cargo run --release --offline -p struo-cli --example rica-case-mux -- \
  /path/to/rica/toolchain/mre/struo-case-mux/src/mux_mre.veryl \
  CaseMux9 /tmp/CaseMux9.json 125
```

Repeat with `BalancedMux9`, `CaseMux16`, and `BalancedMux16`. The JSON files are
generated artifacts and are deliberately not committed. To inspect one
specific mapped path:

```sh
python3 toolchain/mre/struo-case-mux/analyze_path.py \
  /tmp/CaseMux9.json \
  'ff_select_q[0]' \
  'ff_result_q[0]'
```

Observed on combined Struo commit
`31ce27e9e63677dd6cc44e05151e72a5e504a8ad`:

| Top | Lowered mux depth | Synth mux depth | Mapped period | Mapped depth | LUT4 | PFUMX |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `CaseMux9` | 9 | 9 | 5720 ps | 9 | 521 | 0 |
| `BalancedMux9` | 4 | 4 | 2380 ps | 4 | 384 | 192 |
| `CaseMux16` | 16 | 16 | 9220 ps | 16 | 976 | 0 |
| `BalancedMux16` | 4 | 4 | 2380 ps | 4 | 640 | 320 |

The result is unchanged with a 250 MHz mapper goal. Serialization is created
in `struo-frontend-veryl/src/lower.rs::lower_case`, before synthesis or ECP5
mapping: reversed arms are folded one at a time into nested `rtl.mux`
expressions. Synthesis bit-blasts those muxes without reassociation.

The old Core CSR path that exposed the same issue contained nineteen `LUT4`
cells followed by one `PFUMX`.
