#!/usr/bin/env bash
#
# FW-23 / G8.6: is a framework binary reproducible? Build the same package
# twice from this checkout into two fresh target directories and compare the
# bytes. Exit 1 with the differing sizes and digests if they disagree.
#
#   ci/release/repro-check.sh [PACKAGE]      # default: feature-rectangle
#
# Scope, honestly: this proves Rust binaries built from one path with one
# toolchain are bit-identical. Installers (dpkg, AppImage, MSI) add their own
# container metadata; SOURCE_DATE_EPOCH covers what honours it, and the
# studio bundle's copied toolchain is outside the claim.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
pkg="${1:-feature-rectangle}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
sha() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
for run in a b; do
	CARGO_TARGET_DIR="$work/$run" CARGO_INCREMENTAL=0 cargo build --quiet --release --locked -p "$pkg"
done
bin=$(find "$work/a/release" -maxdepth 1 -type f -perm -u+x -name "${pkg}*" ! -name '*.d' | head -1)
[[ -n "$bin" ]] || { echo "repro-check: no binary for $pkg" >&2; exit 2; }
name=$(basename "$bin")
a=$(sha "$work/a/release/$name")
b=$(sha "$work/b/release/$name")
echo "repro-check: $pkg  a=$a  b=$b"
if [[ "$a" != "$b" ]]; then
	ls -l "$work/a/release/$name" "$work/b/release/$name"
	echo "repro-check: NOT reproducible" >&2
	exit 1
fi
echo "repro-check: reproducible"
