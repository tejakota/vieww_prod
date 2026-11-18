#!/usr/bin/env bash
#
# Did this change make the CPU rasterizer do more work?
#
#   ci/check/raster-instructions.sh [BASE]     # BASE defaults to origin/main
#
# Counts the instructions the CPU path executes to draw Market Watch's live
# and storm phases (`examples/market-watch/examples/cpu_bench.rs`, the phone's
# 1081x2340 frame at 2.75x, the serial walk the phones use) under callgrind,
# at BASE and at the working tree, and fails when either phase grew by more
# than RASTER_INSTRUCTIONS_TOLERANCE percent (default 1.0).
#
# # Why instructions and not milliseconds
#
# A shared CI runner's wall clock moves 10-30% run to run (TRACKER: "single
# before/after runs are not evidence"); a 3% regression is invisible in it and
# a gate on it fails at random. Callgrind's instruction count is
# deterministic: the same binary on the same input executes the same
# instructions, on any machine, every time. It does not see cache misses or
# a slow divide, so it is the referee for "more work", not for "slower on a
# phone" — the phone runs answer that one.
#
# # Allowed growth
#
# A change that means to spend more instructions (a correctness fix, a new
# feature on the hot path) says so with an `Instructions-Changed:` trailer in
# a commit between BASE and HEAD, naming what grew and why; the script then
# reports the numbers without failing — the trailer carries the decision, as
# `Pixels-Changed:` does for raster-identity.sh.
#
# BASE is built in a worktree with its own target directory, with *this*
# tree's cpu_bench.rs copied in, so both sides measure the same schedule.
#
# Requires: valgrind, cargo.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo="$(git -C "$root" rev-parse --show-toplevel)"
base="${1:-origin/main}"
tolerance="${RASTER_INSTRUCTIONS_TOLERANCE:-1.0}"
frames="${RASTER_INSTRUCTIONS_FRAMES:-30}"
phases=(live storm)
bench_rel="vieww_base/examples/market-watch/examples/cpu_bench.rs"

command -v valgrind > /dev/null || { echo "raster-instructions: valgrind is not installed" >&2; exit 2; }

work="$(mktemp -d)"
cleanup() {
    git -C "$repo" worktree remove --force "$work/base" > /dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

base_sha="$(git -C "$repo" rev-parse --verify "$base^{commit}")"
git -C "$repo" worktree add --quiet --detach "$work/base" "$base_sha" > /dev/null
cp "$repo/$bench_rel" "$work/base/$bench_rel"

# count DIR LABEL PHASE -> prints the instruction count of that phase's raster
count() {
    local dir="$1" label="$2" phase="$3"
    local bin="$dir/vieww_base/target/instructions/release/examples/cpu_bench"
    local out="$work/$label-$phase.callgrind"
    VIEWW_RASTER_THREADS=1 valgrind --tool=callgrind --callgrind-out-file="$out" \
        --toggle-collect='*render_retained_in_place*' \
        "$bin" --frames "$frames" --runs 1 --phase "$phase" > /dev/null 2> "$work/$label-$phase.log"
    sed -n 's/^summary: *//p; s/^totals: *//p' "$out" | head -1
}

build() {
    local dir="$1"
    (cd "$dir/vieww_base" && cargo build -q --release -p market-watch --example cpu_bench \
        --target-dir target/instructions)
}

echo "raster-instructions: building at $base (${base_sha:0:10})"
build "$work/base"
echo "raster-instructions: building the working tree"
# The working tree builds in place, in its own target directory too, so the
# two binaries come from the same profile and flags.
(cd "$root" && cargo build -q --release -p market-watch --example cpu_bench --target-dir target/instructions)
ln -s "$repo" "$work/head-tree"

trailer=no
if git -C "$repo" log --format=%B "$base_sha..HEAD" 2>/dev/null | grep -q '^Instructions-Changed:'; then
    trailer=yes
fi

failed=0
summary="| phase | $base (${base_sha:0:10}) | this tree | change |\n|---|---|---|---|\n"
for phase in "${phases[@]}"; do
    before="$(count "$work/base" base "$phase")"
    after="$(count "$work/head-tree" head "$phase")"
    pct="$(python3 -c "print(f'{($after - $before) / $before * 100:+.2f}')")"
    summary+="| $phase | $before | $after | $pct% |\n"
    if python3 -c "import sys; sys.exit(0 if ($after - $before) / $before * 100 > $tolerance else 1)"; then
        failed=1
    fi
done
printf '%b' "$summary"
[ -n "${GITHUB_STEP_SUMMARY:-}" ] && printf '### CPU raster instructions\n\n%b\n' "$summary" >> "$GITHUB_STEP_SUMMARY"

if [ "$failed" -ne 0 ]; then
    if [ "$trailer" = yes ]; then
        echo "raster-instructions: grew by more than ${tolerance}%, allowed by an Instructions-Changed: trailer"
        exit 0
    fi
    echo "raster-instructions: a phase grew by more than ${tolerance}%." >&2
    echo "  If that is intended, say why in an Instructions-Changed: trailer." >&2
    exit 1
fi
echo "raster-instructions: within ${tolerance}% on every phase"
