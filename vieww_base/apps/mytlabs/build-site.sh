#!/usr/bin/env bash
#
# Build the mytlabs page: Rust to wasm, wasm to a directory a static host can
# serve.
#
# Usage:
#   apps/mytlabs/build-site.sh                 # -> apps/mytlabs/dist/
#   apps/mytlabs/build-site.sh /tmp/out        # somewhere else
#
# # What it needs
#
#   * the `wasm32-unknown-unknown` target — `rustup target add
#     wasm32-unknown-unknown`, or, on a machine that cannot reach
#     `static.rust-lang.org`, the build-std recipe in `viewwsite`'s script;
#   * `wasm-bindgen` **exactly matching** the `wasm-bindgen` version in
#     `Cargo.lock` — `cargo install wasm-bindgen-cli --version <that>`. A
#     mismatch is a runtime error in the browser about a schema version, not a
#     build failure, which is why this script checks it.
#
# # Reviewing without any of this
#
# `cargo run -p mytlabs --example snapshot` writes the same page as static
# HTML on the host — no wasm toolchain, no browser — which is the fast loop.
# This script is for the page a person visits.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

out="${1:-apps/mytlabs/dist}"
target="wasm32-unknown-unknown"

# ── the toolchain ──────────────────────────────────────────────────────────

wanted="$(sed -n '/^name = "wasm-bindgen"$/,/^version/ s/^version = "\(.*\)"/\1/p' Cargo.lock | head -1)"
if ! command -v wasm-bindgen >/dev/null; then
        echo "error: wasm-bindgen is not on PATH." >&2
        echo "       cargo install wasm-bindgen-cli --version $wanted" >&2
        exit 1
fi
have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$have" != "$wanted" ]; then
        # Worth stopping for: a mismatch builds fine and fails in the browser
        # with a message about schema versions that names neither number.
        echo "error: wasm-bindgen $have, but Cargo.lock pins wasm-bindgen $wanted." >&2
        echo "       cargo install wasm-bindgen-cli --version $wanted --force" >&2
        exit 1
fi

flags=(--release -p mytlabs --target "$target")
if [ -n "${VIEWW_BUILD_STD:-}" ]; then
        export RUSTC_BOOTSTRAP=1
        flags+=(-Z build-std=std,panic_abort)
fi

# ── build ──────────────────────────────────────────────────────────────────

echo "==> cargo build ${flags[*]}"
# The profile is `viewwsite`'s, measured there: `-Oz` + fat LTO + strip is a
# fifth off the wire for no observable difference in how a page runs. The
# rasteriser spends its time in tight loops that `opt-level = "z"` does not
# meaningfully slow. Debug info is off because it is ten times the payload
# for something nobody loading the page can read.
CARGO_PROFILE_RELEASE_DEBUG=false \
CARGO_PROFILE_RELEASE_STRIP=symbols \
CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
CARGO_PROFILE_RELEASE_LTO=fat \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
        cargo build "${flags[@]}"

echo "==> wasm-bindgen --target web"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" \
        "target/$target/release/mytlabs.wasm"

echo "==> index.html and fonts"
cp apps/mytlabs/index.html "$out/index.html"
# The faces the page is set in. Files rather than bytes in the wasm: the
# browser caches them on their own, and a repeat visit pays for neither.
mkdir -p "$out/fonts"
for face in Geist-Regular Geist-Medium Geist-Bold GeistMono-Regular GeistMono-Medium; do
        cp "apps/mytlabs/assets/fonts/vw-$face.ttf" "$out/fonts/$face.ttf"
done
cp apps/mytlabs/assets/fonts/Geist-OFL.txt "$out/fonts/OFL.txt"

raw="$(stat -c%s "$out/mytlabs_bg.wasm" 2>/dev/null || stat -f%z "$out/mytlabs_bg.wasm")"
gz="$(gzip -9 -c "$out/mytlabs_bg.wasm" | wc -c | tr -d ' ')"
echo
echo "$out — index.html, mytlabs.js, mytlabs_bg.wasm, fonts/"
echo "wasm: $raw bytes, $gz gzipped — the second number is what a visitor waits for"
echo
echo "Serve it with any static host. Two headers matter:"
echo "  .wasm  ->  Content-Type: application/wasm   (or the browser refuses to"
echo "             stream-compile it and falls back, slowly, or not at all)"
echo "  .wasm  ->  Content-Encoding: br or gzip     (the wire is the wait)"
