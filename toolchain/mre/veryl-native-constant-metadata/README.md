# Veryl native testbench: signed constant metadata

[ISSUE.md](ISSUE.md) is an English issue draft. The standalone native
[testbench](src/signed_constant.veryl) reproduces incorrect sign extension in
Veryl 0.21.0, revision `2e32d53f056eef323225a91079af92574d18f726`.

Run from this directory with that Veryl binary:

```sh
veryl test --backend interpret --no-capture --seed 1
```

`--backend cranelift` and `--backend cc` reproduce the same failure.

| Build | Output for all-ones input | Result in each of the three modes |
| --- | --- | --- |
| Clean 0.21.0 | `00000000000000ff` | assertion failed, exit 1 |
| Same revision + existing constant metadata patch | `ffffffffffffffff` | assertion passed, exit 0 |

The local experiment applies only
[the existing patch](../../vendor/veryl-const-value/fix.patch) to
`crates/analyzer/src/conv/utils.rs`. The testbench source is identical before
and after. This directory contains a draft and its reproduction artifacts;
no Veryl issue has been published.

## What changed when converting the reproducer

The earlier Celox harness also failed for an unsigned constant mask. When
converted to a Veryl native testbench, that case **passed** on unmodified
0.21.0. Native interpreter controls for the parameter default, HDL override,
and an inline constant also passed. The signed-constant case is the one that
actually reproduces through Veryl's native runner, so it is the example used
in the issue draft. Celox observations are not reported as native Veryl
observations.

## Recorded runs

[evidence/results.json](evidence/results.json) records the binary and source
hashes, exit statuses, outputs, and raw log hashes. The logs in `evidence/` are
unmodified command output. `evidence/control-project/` contains the five-case
native investigation (four passed, the signed case failed) and its log is
`evidence/controls-interpret.log`.

For a new run with separate project and cache directories, from the Rica
repository root:

```sh
python3 toolchain/mre/veryl-native-constant-metadata/run.py \
  --veryl /absolute/path/to/veryl \
  --output /tmp/veryl-signed-constant-new-run \
  --backend interpret
```

The output directory must not already exist. The runner returns the actual
Veryl exit status and writes `run.log`, `result.json`, and the copied project.
Python 3.11 or later is required for this optional helper; the issue's direct
`veryl test` command does not require Python.

The isolated CLI checkout and original build/run records are under the ignored
`toolchain/worktrees/veryl-native-constant*` directories. The baseline binary
was saved as
`toolchain/worktrees/veryl-native-constant-records/bin/veryl-baseline` before
applying the patch.
