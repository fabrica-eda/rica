#!/usr/bin/env bash
# Run the official nextpnr 0.11.1 region A/B experiment.
#
# The baseline is routed at seeds 1, 2, and 5.  Seed 5 is repeated with the
# no-op identity hook, and the routed cell maps are compared.  Each candidate
# is then routed at seed 5.  Candidate rules constrain only DP16KD memory
# cells; LUT/FF/CCU packing remains owned by nextpnr's packer.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
nextpnr_bin="${RICA_NEXTPNR_BIN:-nextpnr-ecp5}"
design_json="${RICA_NEXTPNR_JSON:-$repo_root/riscv/build/riscv-hardware-test-125.nextpnr.json}"
lpf_file="${RICA_NEXTPNR_LPF:-$repo_root/riscv/lfe5um5g-85f-evn.lpf}"
out_dir="${RICA_REGION_OUT:-${TMPDIR:-/tmp}/rica-region-ab}"
placer="${RICA_REGION_PLACER:-static}"
identity_seed="${RICA_REGION_IDENTITY_SEED:-5}"

if [[ -n "${RICA_REGION_CANDIDATE_SEEDS:-}" ]]; then
    read -r -a candidate_seeds <<< "$RICA_REGION_CANDIDATE_SEEDS"
else
    candidate_seeds=(5)
fi
if [[ ${#candidate_seeds[@]} -eq 0 ]]; then
    echo "RICA_REGION_CANDIDATE_SEEDS is empty" >&2
    exit 2
fi

if [[ ! -x "$nextpnr_bin" ]] && ! command -v "$nextpnr_bin" >/dev/null 2>&1; then
    echo "nextpnr binary not found: $nextpnr_bin" >&2
    exit 2
fi
version="$("$nextpnr_bin" --version 2>&1)"
if [[ "$version" != *"nextpnr-0.11.1"* ]]; then
    echo "expected official nextpnr-0.11.1, got: $version" >&2
    exit 2
fi
if [[ ! -f "$design_json" || ! -f "$lpf_file" ]]; then
    echo "missing design or LPF: $design_json / $lpf_file" >&2
    exit 2
fi

mkdir -p "$out_dir"
identity_hook="$script_dir/nextpnr_region_identity.py"
corridor_hook="$script_dir/nextpnr_region_corridor.py"
{
    printf 'nextpnr=%s\n' "$version"
    sha256sum "$design_json" "$lpf_file"
} > "$out_dir/provenance.txt"

if [[ -n "${RICA_REGION_CONFIGS:-}" ]]; then
    read -r -a candidate_configs <<< "$RICA_REGION_CONFIGS"
else
    candidate_configs=(
        "$script_dir/region_memory_overlap.json"
        "$script_dir/region_memory_left.json"
        "$script_dir/region_memory_right.json"
    )
fi
if [[ ${#candidate_configs[@]} -eq 0 ]]; then
    echo "RICA_REGION_CONFIGS is empty" >&2
    exit 2
fi
for candidate in "${candidate_configs[@]}"; do
    if [[ ! -f "$candidate" ]]; then
        echo "missing region candidate: $candidate" >&2
        exit 2
    fi
done

env_args=()
if [[ -n "${PRJTRELLIS_DB:-}" ]]; then
    env_args+=("PRJTRELLIS_DB=$PRJTRELLIS_DB")
fi
if [[ -n "${PYTHONPATH:-}" ]]; then
    env_args+=("PYTHONPATH=$PYTHONPATH")
fi

common_args=(
    --um5g-85k
    --package CABGA381
    --speed 8
    --json "$design_json"
    --freq 125
    --timing-allow-fail
    --lpf "$lpf_file"
    --placer "$placer"
    --router router1
)

run_case() {
    local name="$1"
    local seed="$2"
    local hook="$3"
    local config="$4"
    local log="$out_dir/$name.log"
    local routed="$out_dir/$name.routed.json"
    local report="$out_dir/$name.report.json"
    local -a hook_args=()
    if [[ -n "$hook" ]]; then
        hook_args+=(--pre-place "$hook")
    fi
    env "${env_args[@]}" RICA_REGION_CONFIG="$config" \
        "$nextpnr_bin" "${common_args[@]}" --seed "$seed" \
        "${hook_args[@]}" --write "$routed" --report "$report" \
        > "$log" 2>&1
    sha256sum "$routed" "$report" > "$out_dir/$name.sha256"
}

for seed in 1 2 5; do
    run_case "baseline-seed${seed}" "$seed" "" "${RICA_REGION_CONFIG:-$script_dir/region_corridor.json}"
done

run_case "identity-seed${identity_seed}" "$identity_seed" "$identity_hook" \
    "${RICA_REGION_CONFIG:-$script_dir/region_corridor.json}"

python3 - "$out_dir/baseline-seed${identity_seed}.routed.json" \
    "$out_dir/identity-seed${identity_seed}.routed.json" <<'PY'
import json
import sys


def routed_state(path):
    design = json.load(open(path, encoding="utf-8"))
    module = next(iter(design["modules"].values()))
    cells = {}
    for name, cell in module["cells"].items():
        attrs = cell.get("attributes", {})
        cells[name] = attrs.get("NEXTPNR_BEL")
    nets = {
        name: net.get("attributes", {}).get("ROUTING")
        for name, net in module["netnames"].items()
    }
    return cells, nets


left_cells, left_nets = routed_state(sys.argv[1])
right_cells, right_nets = routed_state(sys.argv[2])
if left_cells != right_cells or left_nets != right_nets:
    changed_cells = [
        name for name in sorted(set(left_cells) | set(right_cells))
        if left_cells.get(name) != right_cells.get(name)
    ]
    changed_nets = [
        name for name in sorted(set(left_nets) | set(right_nets))
        if left_nets.get(name) != right_nets.get(name)
    ]
    raise SystemExit(
        "identity hook changed %d cells and %d routed nets (first cells: %s; first nets: %s)"
        % (len(changed_cells), len(changed_nets), changed_cells[:5], changed_nets[:5])
    )
print("identity placement/routing equivalence: PASS (%d cells, %d nets)" %
      (len(left_cells), len(left_nets)))
PY

for candidate in "${candidate_configs[@]}"; do
    stem="$(basename "$candidate" .json)"
    for seed in "${candidate_seeds[@]}"; do
        run_case "${stem}-seed${seed}" "$seed" "$corridor_hook" "$candidate"
    done
done

{
    echo -e "case\tseed\tconfig\trouted\treport"
    for seed in 1 2 5; do
        echo -e "baseline-seed${seed}\t${seed}\t(no region)\t$out_dir/baseline-seed${seed}.routed.json\t$out_dir/baseline-seed${seed}.report.json"
    done
    echo -e "identity-seed${identity_seed}\t${identity_seed}\t(identity hook)\t$out_dir/identity-seed${identity_seed}.routed.json\t$out_dir/identity-seed${identity_seed}.report.json"
    for candidate in "${candidate_configs[@]}"; do
        stem="$(basename "$candidate" .json)"
        for seed in "${candidate_seeds[@]}"; do
            echo -e "${stem}-seed${seed}\t${seed}\t$candidate\t$out_dir/${stem}-seed${seed}.routed.json\t$out_dir/${stem}-seed${seed}.report.json"
        done
    done
} > "$out_dir/results.tsv"

echo "region A/B experiment: PASS"
cat "$out_dir/results.tsv"
