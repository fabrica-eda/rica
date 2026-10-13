#!/usr/bin/env bash
# Use an extracted Linux source tree; outputs stay in the requested work path.
set -euo pipefail
source_dir=$(realpath "${1:?usage: build-kernel.sh SOURCE OUTPUT}")
output_dir=$(realpath -m "${2:?usage: build-kernel.sh SOURCE OUTPUT}")
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$output_dir"
args=(-C "$source_dir" "O=$output_dir" ARCH=riscv "LLVM=${LLVM:-1}" "HOSTCC=${HOSTCC:-cc}" "HOSTCXX=${HOSTCXX:-c++}")
if [[ -n ${RISCV_CLANG:-} ]]; then args+=("CC=$RISCV_CLANG"); fi
if [[ -n ${RISCV_LLD:-} ]]; then args+=("LD=$RISCV_LLD"); fi
make "${args[@]}" tinyconfig
bash "$source_dir/scripts/kconfig/merge_config.sh" -m -O "$output_dir" \
  "$output_dir/.config" "$script_dir/kernel.config"
make "${args[@]}" olddefconfig
make "${args[@]}" -j"${JOBS:-6}" Image
