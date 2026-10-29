#!/usr/bin/env bash
#
# A stripped release crash, symbolised offline, end to end (FW-12).
#
#     ci/check/crash-symbols.sh
#
# Builds vieww-foundation's `crash_symbols` example with `--profile
# dist-symbols`, splits it with ci/release/symbols.sh (debug info kept as
# <build-id>.debug, the binary stripped to what ships), runs it so it
# panics into a crash file, and symbolises that file with
# ci/release/symbolize.py. Passes when the report names both of the
# example's frames with its source lines, which the stripped binary's own
# backtrace (`<unknown>` throughout) cannot. Linux; needs objcopy or
# llvm-objcopy, readelf, and llvm-symbolizer or addr2line.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# RUSTFLAGS replaces .cargo/config.toml's Linux block (`prefer-dynamic`,
# for Studio's preview hosts): a shipping build links std statically, and
# `panic = "abort"` cannot link the shared, unwinding libstd. `--build-id`
# explicitly: the id is what the report and the symbols are matched by.
RUSTFLAGS="-C link-arg=-Wl,--build-id" \
    cargo build -q --profile dist-symbols -p vieww-foundation --example crash_symbols
bin="$work/crash_symbols"
cp target/dist-symbols/examples/crash_symbols "$bin"
ci/release/symbols.sh "$bin" "$work/symbols"
if readelf -S "$bin" | grep -q '\.debug_line'; then
    echo "crash-symbols: the shipped binary still carries line tables" >&2
    exit 1
fi
mkdir -p "$work/crashes"
VIEWW_CRASH_DIR="$work/crashes" "$bin" extra 2>/dev/null && { echo "crash-symbols: it did not crash" >&2; exit 1; }
report="$(ls "$work"/crashes/*)"
grep -E '^(message|frame: #[0-2] )' "$report"
python3 -I ci/release/symbolize.py "$report" "$work/symbols" | tee "$work/symbolized.txt"
for want in 'crash_symbols::outer_frame' 'crash_symbols::inner_frame\|crash_symbols.rs:12'; do
    if ! grep -q "$want" "$work/symbolized.txt"; then
        echo "crash-symbols: '$want' not recovered from the stripped binary's report" >&2
        exit 1
    fi
done
echo "crash-symbols: a stripped release crash decoded to function and line"
