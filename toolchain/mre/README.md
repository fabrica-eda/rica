# Toolchain reproducers

Minimal reproducers for unresolved toolchain defects belong here. There are
currently none: every reproducer found so far has been fixed upstream, and its
regression now lives in the upstream project. See
[docs/toolchain-issues.md](../../docs/toolchain-issues.md) for the catalog.

Related checks that remain in Rica:

- The JTAG-domain STA release check is in `riscv/tools/jtag-sta`, used by the
  checked bitgen wrapper.
- Hardware-memory, mapped DSP, predictor and parameter-API regressions are in
  `riscv/verify/tests` and `toolchain/tb/tests`; Veryl native testbenches are in
  `riscv/tests`.
