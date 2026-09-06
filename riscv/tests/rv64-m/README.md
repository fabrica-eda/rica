# RV64M arithmetic regressions

`riscv/tools/generate_rv64_m_tests.py` generates the 13 instruction tests
from an independent Python integer oracle. Each instruction checks 121 pairs
of edge operands, a dependent consumer, destination aliasing with rs1, and x0.
Cases include signed overflow, zero divisors, high products and W sign extension.
Additional randomized arithmetic, cancellation and result backpressure checks
are in `verify/tests/muldiv_rtl.rs`; pipeline tests cover stalls, redirects and
precise exceptions.

```sh
python3 riscv/tools/generate_rv64_m_tests.py
target/release/rv64-riscv-tests --m-tests --compact-predictor --max-cycles 100000
```

The separate, complete ACT4 M catalog uses Sail-derived expected signatures:

```sh
target/release/rv64-riscv-tests --act4-suite M --compact-predictor --max-cycles 1000000
```

Its build configuration is `riscv/act4/test_config_m.yaml`; outputs belong in
`third_party/riscv-arch-test/work/rica-rv64im/elfs/rv64i/M/`.
