#!/usr/bin/env bash
# Run the reproducible baseline-identity and overlapping-corridor smoke.
# Set RICA_REGION_ROUTE=1 to run the corridor through routing as well.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
nextpnr_bin="${RICA_NEXTPNR_BIN:-nextpnr-ecp5}"
design_json="${RICA_NEXTPNR_JSON:-$repo_root/riscv/build/riscv-hardware-test-125.nextpnr.json}"
lpf_file="${RICA_NEXTPNR_LPF:-$repo_root/riscv/lfe5um5g-85f-evn.lpf}"
out_dir="${RICA_REGION_OUT:-${TMPDIR:-/tmp}/rica-region-smoke}"
seed="${RICA_REGION_SEED:-5}"

if [[ ! -x "$nextpnr_bin" ]] && ! command -v "$nextpnr_bin" >/dev/null 2>&1; then
    echo "nextpnr binary not found: $nextpnr_bin" >&2
    exit 2
fi
if [[ ! -f "$design_json" || ! -f "$lpf_file" ]]; then
    echo "missing design or LPF: $design_json / $lpf_file" >&2
    exit 2
fi

mkdir -p "$out_dir"
export RICA_REGION_CONFIG="${RICA_REGION_CONFIG:-$script_dir/region_corridor.json}"
env_args=()
if [[ -n "${PRJTRELLIS_DB:-}" ]]; then
    env_args+=("PRJTRELLIS_DB=$PRJTRELLIS_DB")
fi
if [[ -n "${PYTHONPATH:-}" ]]; then
    env_args+=("PYTHONPATH=$PYTHONPATH")
fi

run_nextpnr() {
    env "${env_args[@]}" "$nextpnr_bin" "$@"
}

common_args=(
    --um5g-85k
    --package CABGA381
    --speed 8
    --json "$design_json"
    --freq 125
    --timing-allow-fail
    --lpf "$lpf_file"
    --placer heap
    --seed "$seed"
)

run_nextpnr "${common_args[@]}" \
    --pre-place "$script_dir/nextpnr_region_identity.py" \
    --no-route > "$out_dir/baseline.identity.log" 2>&1
run_nextpnr "${common_args[@]}" \
    --no-route > "$out_dir/baseline.nohook.log" 2>&1

summary() {
    grep -E 'Info: (Max frequency|Max delay|Checksum|Placed [0-9]+ cells)' "$1"
}
diff -u <(summary "$out_dir/baseline.nohook.log") <(summary "$out_dir/baseline.identity.log")

route_args=(--no-route)
if [[ "${RICA_REGION_ROUTE:-0}" == 1 ]]; then
    route_args=(
        --router router1
        --write "$out_dir/corridor.routed.json"
        --report "$out_dir/corridor.report.json"
    )
fi
run_nextpnr "${common_args[@]}" \
    --pre-place "$script_dir/nextpnr_region_corridor.py" \
    "${route_args[@]}" > "$out_dir/corridor.log" 2>&1

echo "baseline identity: PASS"
grep -E 'REGION_(DEVICE_BOUNDS|MATCHES|DIRECT_COUNTS|PROPAGATED_COUNTS|ASSIGNMENTS|ERRORS)|Max frequency|Program finished' \
    "$out_dir/corridor.log"
