#!/usr/bin/env bash
#
# Build the rasterizer with Profile-Guided Optimisation and prove the pixels
# did not move.
#
#   ci/check/pgo.sh [OUTPUT_DIR]     # default: target/pgo
#
# PGO is worth 5-15% on the CPU rasterizer's hot loops (the number the
# perf-budget fixtures measure; the exact figure is a property of the
# workload and the compiler, which is why this script measures rather than
# promises). It is a *release* tool, not a CI gate: it doubles the build
# (instrument, train, rebuild) and the shipping binaries are the ones that
# should pay for it — the Device Farm APKs, not the debug loop.
#
# # Why the pixel identity check is part of the script and not an afterthought
#
# PGO reorders code and tunes inlining from a profile; it does not change
# floating-point semantics — no reassociation, no fast-math, the same
# operations in the same order. The rasterizer's contract (FIDELITY.md §1:
# byte-identical by default) therefore survives a PGO build *by
# construction*, and the check below turns that construction into a receipt:
# the trained build renders the whole fixture gallery and the bytes must
# equal the plain release build's. If they ever do not, the profile has
# steered the compiler somewhere the contract does not go, and the script
# fails loudly instead of shipping faster-wrong pixels.
#
# # What the training workload is
#
# `examples/fixtures` — every reference screen and animation the perf budget
# itself uses. It exercises the scanline walk, the compositing kernels, the
# blur and shadow passes and the damage-replay paths in the proportions a
# real interface spends its frame on, which is the profile a real interface
# will reward.
#
# Requires: the pinned toolchain (rust-toolchain.toml — it lists
# `llvm-tools-preview`, which is `llvm-profdata`), and `cargo`.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

out="${1:-target/pgo}"
profile_dir="$out/profile"
trained_dir="$out/trained"
plain_dir="$out/plain"

# `llvm-profdata` ships with the toolchain's llvm-tools component — which
# rust-toolchain.toml lists, so a fresh clone has it. Resolve it through the
# active toolchain's sysroot: a rustup shim's directory does not carry it.
profdata="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-profdata"
if [ ! -x "$profdata" ]; then
    # Some distributions install it on PATH instead.
    if ! command -v llvm-profdata > /dev/null; then
        echo "pgo: llvm-profdata not found — does the toolchain have llvm-tools-preview?" >&2
        exit 2
    fi
    profdata="llvm-profdata"
fi

echo "pgo: stage 1 — the plain release build (the identity reference)"
cargo build --release -p fixtures --target-dir "$plain_dir"
mkdir -p "$out/gallery-plain"
"$plain_dir/release/fixtures" "$out/gallery-plain" > /dev/null

echo "pgo: stage 2 — the instrumented build"
mkdir -p "$profile_dir"
rm -f "$profile_dir"/*.profraw
RUSTFLAGS="-Cprofile-generate=$profile_dir" \
    cargo build --release -p fixtures --target-dir "$out/instrumented"

echo "pgo: stage 3 — the training run (the whole fixture gallery)"
LLVM_PROFILE_FILE="$profile_dir/default-%p.profraw" \
    "$out/instrumented/release/fixtures" "$out/gallery-instrumented" > /dev/null

echo "pgo: stage 4 — merging the profile"
"$profdata" merge -output "$out/merged.profdata" "$profile_dir"/*.profraw

echo "pgo: stage 5 — the trained build"
RUSTFLAGS="-Cprofile-use=$out/merged.profdata" \
    cargo build --release -p fixtures --target-dir "$trained_dir"

echo "pgo: stage 6 — the trained gallery, and the identity check"
mkdir -p "$out/gallery-trained"
"$trained_dir/release/fixtures" "$out/gallery-trained" > /dev/null

changed=0
while IFS= read -r file; do
    rel="${file#"$out/gallery-plain/"}"
    if [ ! -f "$out/gallery-trained/$rel" ]; then
        echo "  only in the plain build: $rel"; changed=1
    elif ! cmp -s "$file" "$out/gallery-trained/$rel"; then
        echo "  differs: $rel"; changed=1
    fi
done < <(find "$out/gallery-plain" -type f | sort)
while IFS= read -r file; do
    rel="${file#"$out/gallery-trained/"}"
    [ -f "$out/gallery-plain/$rel" ] || { echo "  only in the trained build: $rel"; changed=1; }
done < <(find "$out/gallery-trained" -type f | sort)

count="$(find "$out/gallery-trained" -type f | wc -l | tr -d ' ')"
if [ "$changed" -ne 0 ]; then
    echo "pgo: the trained build's pixels differ from the plain release build's." >&2
    echo "  PGO must not move a pixel; see the header of this script." >&2
    exit 1
fi

echo "pgo: all $count gallery files byte-identical between the plain and trained builds"
echo "pgo: the profile is $out/merged.profdata — build the shipping binaries with"
echo "  RUSTFLAGS=\"-Cprofile-use=$out/merged.profdata\" cargo build --release"
