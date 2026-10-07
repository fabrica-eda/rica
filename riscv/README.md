# riscv/

The Rica CPU, its board wrappers and their tooling. Documentation lives in
[`../docs`](../docs); start with the [top-level README](../README.md).

| Path | Contents |
| --- | --- |
| `src/` | Veryl RTL: `Core.veryl` (pipeline), predictors, caches, execution units, CSRs, PMP, page-table walker, board memory and tops |
| `verify/` | `verify-riscv`: simulation runners, hardware runner, nextpnr export, Rust RTL tests |
| `tests/` | Assembly suites, ACT4 enable lists, Veryl native testbenches ([README](tests/README.md)) |
| `kernel/` | Rust test kernels ([docs/kernels.md](../docs/kernels.md)) |
| `tools/` | Texo launchers, checked bitgen, PLL/profile generation, test generators |
| `characterization/` | On-board delay measurement ([docs/timing-characterization.md](../docs/timing-characterization.md)) |
| `act4/` | ACT4 configurations |
| `platform-user.json` | Clock profile of the working board configuration |
| `*.lpf`, `pll-*.json`, `jtck-clock-constraints.json`, `timing-coverage-exceptions.json` | Physical constraints ([docs/fpga-flow.md](../docs/fpga-flow.md)) |
| `qualified-*.json` | Hashes of the measured timing inputs used for sign-off |
