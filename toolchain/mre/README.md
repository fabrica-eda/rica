# Toolchain reproducer inventory

Only unresolved reproducers belong here. Fixed reproducers and obsolete board
experiments were removed; their sources remain in Git history and historical
results remain in `toolchain/measurements`. Historical links to removed MREs
point here intentionally.

The workspace pins upstream Celox 0.4.7 at `ab63d6c`, Struo at `70058b6`,
and Texo at `c48afcd`, with one revision of each across direct and transitive
dependencies. Both Celox vendor directories and the Celox Cargo override have
been removed. The separate Veryl constant-metadata patch remains required.
See the [Celox adoption record](../measurements/2026-09-11-celox-047/README.md).

There are currently no active MREs in this directory. The final constant-loop
MRE was fixed by [Struo PR #90](https://github.com/fabrica-eda/struo/pull/90)
and removed with its dedicated Cargo crate. The fixture and regression now live
upstream. See the [adoption record](../measurements/2026-09-11-constant-arithmetic/README.md).

- Current JTAG STA release checking lives in `riscv/tools/jtag-sta`; the bitgen
  wrapper uses it directly. Old JTAG corruption and ECO probes were removed.
- Hardware-memory, parameter API, mapped DSP and predictor regressions remain
  in `riscv/verify/tests`; native Veryl testbenches live in `riscv/tests`.

Removed: Celox concat-width, parameter-repeat, parameterized-memory (including
its historical performance harness), cross-always-FF diagnostic, Struo case-mux,
associative-boolean and DSP diagnostics, Veryl constant-metadata reproduction,
and both repaired RV64 JTAG investigations. The cross-always-FF reduction had
never reproduced a standalone failure and is no longer an active investigation.
