# Registered DSP pipeline model mismatch

The seven-stage `DspMultiply64` RTL passes the native Celox 0.4.5 integer
oracle tests. Its mapped netlist under Struo `bb4d268` contains 16 MULT18X18D,
419 CCU2C and 1859 TRELLIS_FF cells, with no retiming applied.

The Celox 0.3.1 mapped adapter (with Rica's existing `celox-sir-opt` backport)
returns `0x000000000000000000000000ffff0001` on cycle 12 for a stream whose
seven-cycle delayed expected result is zero. The regression lives in
`riscv/verify/tests/muldiv_rtl.rs`.

`check_mapped.py` evaluates the serialized DSP/CCU2C/FF graph independently,
with every FF sampling the pre-edge state. It passes all 177 products and the
seven-cycle latency for the exact failing mapped JSON.

Upgrading the mapped adapter to Celox 0.4.5 `4c2c227` reproduces the same
cycle-12 failure. Disabling all SIR optimizations passes. Disabling each pass
individually narrows the failure to the interaction of `InlineCommitForwarding`
and `SplitCoalescedStores`: disabling either one passes; disabling each of the
other 25 passes still fails.

`SplitCoalescedStores` places each fragment immediately after its source value
is computed. After forwarding a WORKING store to STABLE, this can overwrite
part of an FF bank before another fragment reads its old value. The fix keeps
splitting enabled but places stores after preceding accesses to their state
object and observable events. The full DSP fixture passes with all default
optimizations under this fix. A regression also checks that independent state
objects still permit early stores. Celox 0.4.6 without the fix fails on all
three backends; commit `7ff0dd2` passes native, Cranelift and WASM with all
default optimizations enabled. The fix is [Celox PR #762](https://github.com/celox-sim/celox/pull/762).

CPU candidate 07 meets 124.8 MHz with 200 ps additional setup margin
(setup +24 ps, hold +4 ps). Bitgen initially rejected missing CIB ties for DSP
sites whose C inputs use three fixed aliases. Texo now follows all fixed aliases.
Candidate 09 preserves the exact routes and timing, emits all 288 C-input ties,
and produces bit SHA `7297078f67fa530468cf57e13cce3bdcb600dbf20611788b4facf05abe021733`.
It passes 133 physical ELFs plus 32 diagnostic repeats. Texo `acdfe43` with final
Struo/Celox pins reproduces the identical bitstream from a fresh synthesis and
STA pass (candidate 10); see [Texo PR #68](https://github.com/fabrica-eda/texo/pull/68).

Hardware multiplication improves from 16,294 to 10,147 cycles at 124.8 MHz.
The whole-CPU model's 9,630-cycle result is not used as a hardware performance
number. A separate native/mapped MDU regression agrees on result and completion
latency for 936 operations. In the CPU model, MDU waiting is also 256 x 11 cycles;
most of the remaining model/hardware gap is memory waiting (1,285 vs 1,793).
The two serializations of coincident clock edges are functional stress models,
not an atomic simultaneous-edge timing model.
