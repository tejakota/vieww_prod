#!/usr/bin/env bash
# Fail if measured performance has drifted past its recorded budget (FW-08).
#
#     ci/check/perf-budget.sh [--class NAME] [RECEIPT|SUMMARY ...]
#     ci/check/perf-budget.sh --record [--class NAME] [RECEIPT|SUMMARY ...]
#
# With no files, runs the fixtures gallery (`--runs 10`) and checks its
# receipt. Files may be fixtures receipts (vieww.bench-receipt/1) or a
# Device Farm summary.json; perf_budget.py has the details. Budgets live in
# ci/check/perf-budgets.json, keyed by machine class; a class with no budget
# is reported and not failed, as size-budget.sh treats an unknown target.
#
# Same discipline as size-budget.sh: `--record` is a reviewed commit, and the
# tolerance (perf-budgets.json, 10% by default) is wide enough that ordinary
# work does not trip it. Gate only where the machine is stable — the
# self-hosted hardware and Device Farm phones, not hosted runners, whose
# neighbours move these numbers by more than the tolerance.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
budgets="$here/perf-budgets.json"
cmd=check
args=()
while [ $# -gt 0 ]; do
    case "$1" in
        --record) cmd=record ;;
        --class) args+=(--class "$2"); shift ;;
        *) args+=("$1") ;;
    esac
    shift
done
files=0
for a in "${args[@]}"; do case "$a" in --class) ;; *.json) files=$((files + 1)) ;; esac; done
if [ "$files" -eq 0 ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    (cd "$here/../.." && cargo run -q --release -p fixtures -- "$tmp/out" --runs 10 --receipt "$tmp/receipt.json")
    args+=("$tmp/receipt.json")
fi
exec python3 -I "$here/perf_budget.py" "$cmd" "$budgets" "${args[@]}"
