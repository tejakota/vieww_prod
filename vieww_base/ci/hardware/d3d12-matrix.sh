#!/usr/bin/env bash
# One CI run that checks the D3D12 GPU-drawn window, variant by variant.
#
#   ci/hardware/d3d12-matrix.sh            # from vieww_base, on the Windows machine
#
# Builds `baseline` once, runs `baseline --gpu` under each variant with a
# time limit, and prints one table: variant, GPU frames drawn, verdict (OK,
# FAULT, HUNG) and the first line that explains a failure. Full logs are in
# out/d3d12-matrix/<variant>.log (Artifacts: out/**).
#
# History: the first matrix (run 45211604020) found that damaged frames into
# the retained L[0] page-fault on the HD 620 and full redraws do not; the
# window path now redraws in full, and `retained` keeps the old behaviour
# here as the regression check (expected: FAULT on that laptop).
set -uo pipefail
cd "$(dirname "$0")/../.."
out=out/d3d12-matrix
mkdir -p "$out"
cargo build --release -p vieww-platform-winit --example baseline || exit 1
exe=target/release/examples/baseline

base="VIEWW_PRESENT=d3d12 VIEWW_TRACE_FRAMES=1 VIEWW_D3D12_TRACE=1 VIEWW_D3D12_DRED=1"
variants=(
  "default|"
  "default-again|"
  "retained|VIEWW_D3D12_RETAINED=1"
)
[ $# -gt 0 ] && variants=("$@")

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
  why=$(grep -m1 -E 'GPU stopped here|page fault at|d3d12 \[[a-z]+\] severity 1 |GPU renderer failed|did not finish' "$log" | cut -c1-160)
  printf '%-18s %6s %-8s %s\n' "$name" "$frames" "$verdict" "$why" >> "$out/summary.txt"
  grep -h '__VIEWW_BASELINE__' "$log" | sed 's/.*__VIEWW_BASELINE__ //' > "$out/$name.json"
done
cat "$out/summary.txt"
for j in "$out"/*.json; do [ -s "$j" ] && echo "$(basename "$j" .json): $(cat "$j")"; done
