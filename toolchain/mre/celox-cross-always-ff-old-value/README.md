# Celox cross-`always_ff` old-value reduction

This is a retained semantic probe for a failure originally seen in Rica's
registered branch-predictor decision queue. It is deliberately labelled a
reduction rather than a confirmed Celox MRE: the reduced circuit behaves
correctly on every backend tested.

The source keeps the original scheduling shape:

- a clock-only `always_ff` process captures `response_fallthrough_q` and one
  metadata bit;
- one reset-bearing process replaces the metadata on `request_fire`; and
- a separate reset-bearing process replaces `response_fallthrough_q`.

Reset establishes `response_fallthrough_q = 4`. The harness then asserts
`raw_response_capture` and `request_fire` on the same rising edge while
driving `request_fallthrough = 8`. IEEE nonblocking-assignment semantics, and
Veryl's `always_ff` contract, require the post-edge state to be:

```text
current_value=8, captured_value=4
current_metadata=1, captured_metadata=0
```

The Rust harness reads the Veryl source from disk and uses only public Celox,
Struo, and ECP5 adapter APIs. It embeds neither generated SystemVerilog nor
the Veryl source text in Rust.

## Run

From the Rica repository root:

```sh
cargo run --release --offline -p verify-riscv \
  --example celox-cross-always-ff-old-value
```

The example target is declared in `riscv/verify/Cargo.toml`; no source copying
or generated HDL is required. An explicit source path may be passed as the
only argument when testing a modified copy.

## Observed result

On 2026-09-02, with Rica's lock file selecting Celox 0.4.2 commit
`7bb38a0ce4d66f349d499b270d27375d7c1a01bb` and Struo commit
`eb6dd6ac184b2783e4738edd369a991131b3de7b`, the command reports the following.
The
Struo adapter uses Celox 0.3.1, with the workspace's `celox-sir-opt` patch at
`f5c548d5d4becb298f924ab06e1febb9eb3da064`.

```text
Celox source/native/O0: PASS: current=8, captured=4, current_metadata=1, captured_metadata=0
Celox source/WASM/O0: PASS: current=8, captured=4, current_metadata=1, captured_metadata=0
Struo ECP5 mapped/Celox 0.3.1 native: PASS: current=8, captured=4, current_metadata=1, captured_metadata=0
```

The third row runs `analyze_and_lower`, target-independent synthesis, ECP5
mapping, `ecp5_simulator`, and the Celox 0.3.1 native backend used by the
Struo adapter. Struo does not provide a distinct source-RTL simulator in this
flow; the first two rows are the source-RTL comparisons.

## Interpretation

The unreduced BranchPredictor previously produced
`response_fallthrough_q=8, decision_fallthrough_q=8` on the same edge under
Celox source/native/O0; `decision_fallthrough_q=4` was expected. Co-locating
the response packet capture and request-metadata replacement made that full
test pass. However, this retained three-process reduction, including the
clock-only/reset-bearing boundary, does not reproduce the failure. Therefore
the available evidence does **not** establish a standalone Celox scheduling
defect. The full-design observation remains either context-sensitive or
misattributed until the pre-change full source reproduces against at least
Native and WASM with an identical edge trace.
