#!/usr/bin/env bash
# Build pinned nextpnr-0.11.1 + Python-enabled ECP5 support in an out-of-tree
# directory.  The caller supplies normal cmake/ninja/Boost/Python build deps;
# no files under the Rica source tree are written by this script.
set -euo pipefail

out_root="${RICA_NEXTPNR_BUILD_ROOT:-${TMPDIR:-/tmp}/rica-nextpnr-0.11.1}"
nextpnr_ref="${RICA_NEXTPNR_REF:-nextpnr-0.11.1}"
trellis_ref="${RICA_TRELLIS_REF:-1.4}"
cmake_bin="${CMAKE:-cmake}"

nextpnr_src="$out_root/nextpnr-src"
trellis_src="$out_root/prjtrellis-src"
trellis_build="$out_root/prjtrellis-build"
nextpnr_build="$out_root/nextpnr-build"
trellis_prefix="$out_root/prjtrellis-prefix"

mkdir -p "$out_root"
if [[ ! -d "$nextpnr_src/.git" ]]; then
    git clone --depth 1 --branch "$nextpnr_ref" --recursive \
        https://github.com/YosysHQ/nextpnr.git "$nextpnr_src"
fi
if [[ ! -d "$trellis_src/.git" ]]; then
    git clone --depth 1 --branch "$trellis_ref" --recursive \
        https://github.com/YosysHQ/prjtrellis.git "$trellis_src"
fi

trellis_db="${PRJTRELLIS_DB:-$trellis_src/database}"
if [[ ! -d "$trellis_db" ]]; then
    echo "missing Project Trellis database: $trellis_db" >&2
    exit 2
fi

"$cmake_bin" -S "$trellis_src/libtrellis" -B "$trellis_build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_PYTHON=ON \
    -DBUILD_SHARED=ON \
    -DBUILD_ECPBRAM=OFF \
    -DBUILD_ECPPACK=OFF \
    -DBUILD_ECPUNPACK=OFF \
    -DBUILD_ECPPLL=OFF \
    -DBUILD_ECPMULTI=OFF \
    -DCMAKE_INSTALL_PREFIX="$trellis_prefix"
"$cmake_bin" --build "$trellis_build" --target pytrellis --parallel

"$cmake_bin" -S "$nextpnr_src" -B "$nextpnr_build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DARCH=ecp5 \
    -DECP5_DEVICES=85k \
    -DBUILD_PYTHON=ON \
    -DBUILD_GUI=OFF \
    -DBUILD_RUST=OFF \
    -DBUILD_TESTS=OFF \
    -DUSE_IPO=OFF \
    -DCMAKE_INSTALL_PREFIX="$out_root/nextpnr-prefix" \
    -DTRELLIS_INSTALL_PREFIX="$trellis_prefix" \
    -DTRELLIS_LIBDIR="$trellis_build" \
    -DTRELLIS_DATADIR="$trellis_src"

PRJTRELLIS_DB="$trellis_db" \
PYTHONPATH="$trellis_build${PYTHONPATH:+:$PYTHONPATH}" \
    "$cmake_bin" --build "$nextpnr_build" --target nextpnr-ecp5 --parallel

"$nextpnr_build/nextpnr-ecp5" --version
echo "nextpnr source: $nextpnr_src"
echo "Trellis source: $trellis_src"
echo "nextpnr binary: $nextpnr_build/nextpnr-ecp5"
