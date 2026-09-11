# Active toolchain reproducers

Only unresolved reproducers belong here. Fixed reproducers and obsolete board
experiments were removed; their sources remain in Git history and historical
results remain in `toolchain/measurements`. Historical links to removed MREs
point here intentionally.

The current workspace pins Celox `7ff0dd2` and Struo `4e9de73`. The Celox facade
parameter-width patch and Veryl constant-metadata patch in `toolchain/vendor`
are still required; deleting their old reproducers does not remove these fixes.
Celox is shared by the RTL runner and Struo adapter through the workspace patch.

- `struo-constant-loop`: unresolved constant arithmetic QoR issue. Run
  `cargo run --locked --offline -p rica-mre --bin struo-constant-loop`.
- Current JTAG STA release checking lives in `riscv/tools/jtag-sta`; the bitgen
  wrapper uses it directly. Old JTAG corruption and ECO probes were removed.
- Hardware-memory, parameter API, mapped DSP and predictor regressions remain
  in `riscv/verify/tests`; native Veryl testbenches live in `riscv/tests`.

Removed: Celox concat-width, parameter-repeat, parameterized-memory (including
its historical performance harness), cross-always-FF diagnostic, Struo case-mux,
associative-boolean and DSP diagnostics, Veryl constant-metadata reproduction,
and both repaired RV64 JTAG investigations. The cross-always-FF reduction had
never reproduced a standalone failure and is no longer an active investigation.
