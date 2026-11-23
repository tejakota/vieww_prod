#!/usr/bin/env bash
# One CI run, every hypothesis about the D3D12 GPU-drawn window fault.
#
#   ci/hardware/d3d12-matrix.sh            # from vieww_base, on the Windows machine
#
# Builds `baseline` once, then runs `baseline --gpu` under each variant with a
# time limit, and prints one table: variant, GPU frames drawn, verdict, and
# the first line that explains a failure (DRED, the debug layer, vieww).
# Each variant's full log is kept in out/d3d12-matrix/<variant>.log, so
# "6 · Run a command" with Artifacts `out/**` brings everything back.
set -uo pipefail
cd "$(dirname "$0")/../.."
out=out/d3d12-matrix
mkdir -p "$out"
cargo build --release -p vieww-platform-winit --example baseline || exit 1
exe=target/release/examples/baseline

base="VIEWW_PRESENT=d3d12 VIEWW_TRACE_FRAMES=1 VIEWW_D3D12_TRACE=1 VIEWW_D3D12_DRED=1"
variants=(
  "default|"
  "full-redraw|VIEWW_D3D12_FULL_REDRAW=1"
  "pad-constants|VIEWW_D3D12_PAD_CONSTANTS=1"
  "readback-present|VIEWW_D3D12_READBACK_PRESENT=1"
  "fragment-blur|VIEWW_GPU_COMPUTE_BLUR=0"
  "debug-layer|VIEWW_D3D12_DEBUG=1 VIEWW_D3D12_GBV=0"
  "no-shader-cache|VIEWW_SHADER_CACHE=off"
)

printf '\n%-18s %6s %-8s %s\n' variant frames verdict why > "$out/summary.txt"
for entry in "${variants[@]}"; do
  name=${entry%%|*}; extra=${entry#*|}
  log="$out/$name.log"
  echo "=== $name: $extra"
  env $base $extra timeout 90 "$exe" --gpu >"$log" 2>&1
  code=$?
  frames=$(grep -c 'frame stages (gpu)' "$log")
  if grep -q '__VIEWW_BASELINE__' "$log" && ! grep -q 'GPU renderer failed' "$log"; then
    verdict=OK
  elif [ "$code" = 124 ]; then
    verdict=HUNG
  else
    verdict=FAULT
  fi
  why=$(grep -m1 -E 'GPU stopped here|page fault at|d3d12 \[[a-z]+\] severity [12] |GPU renderer failed|did not finish' "$log" | cut -c1-160)
  printf '%-18s %6s %-8s %s\n' "$name" "$frames" "$verdict" "$why" >> "$out/summary.txt"
done
cat "$out/summary.txt"
