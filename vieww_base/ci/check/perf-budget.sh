#!/usr/bin/env bash
# Fail if measured performance has drifted past its recorded budget (FW-08).
#
#     ci/check/perf-budget.sh [--class NAME] [RECEIPT|SUMMARY ...]
#     ci/check/perf-budget.sh --record [--class NAME] [RECEIPT|SUMMARY ...]
#     ci/check/perf-budget.sh [--advisory] [--baseline LAST.json] \
#                             [--write-measured OUT.json] [RECEIPT ...]
#
# --write-measured saves this run's figures; --baseline holds a class with
# no committed budget to the previous run's saved figures (the CI legs
# fetch them from ci-results), so a machine is gated from its second run
# without anyone recording by hand. --advisory reports and never fails, for
# hosted runners.
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
        --class | --baseline | --write-measured) args+=("$1" "$2"); shift ;;
        --advisory) args+=(--advisory) ;;
        *) args+=("$1") ;;
    esac
    shift
done
files=0
skip=""
for a in "${args[@]}"; do
    if [ -n "$skip" ]; then skip=""; continue; fi
    case "$a" in --class | --baseline | --write-measured) skip=1 ;; *.json) files=$((files + 1)) ;; esac
done
if [ "$files" -eq 0 ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    (cd "$here/../.." && cargo run -q --release -p fixtures -- "$tmp/out" --runs 10 --receipt "$tmp/receipt.json")
    args+=("$tmp/receipt.json")
fi
py="$(command -v python3 || command -v python)"
exec "$py" -I "$here/perf_budget.py" "$cmd" "$budgets" "${args[@]}"
