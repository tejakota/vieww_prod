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
# backtrace (`<unknown>` throughout) cannot.
#
# Linux (ELF: objcopy, llvm-symbolizer or addr2line), macOS (Mach-O: the
# dSYM cargo writes, atos or llvm-symbolizer) and Windows (PE: the PDB,
# llvm-symbolizer — the hosted images carry LLVM). Each runs the same
# release-shaped build and the same symbolizer.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# RUSTFLAGS replaces .cargo/config.toml's host blocks (`prefer-dynamic`,
# for Studio's preview hosts): a shipping build links std statically, and
# `panic = "abort"` cannot link the shared, unwinding libstd. On Linux,
# `--build-id` explicitly: the id is what the report and the symbols are
# matched by (Apple's linker always writes LC_UUID; MSVC's link.exe always
# writes the CodeView record with /DEBUG, which debug info implies).
case "$(uname -s)" in
Linux) os=linux; flags="-C link-arg=-Wl,--build-id"; exe="" ;;
Darwin) os=macos; flags=""; exe="" ;;
MINGW* | MSYS* | CYGWIN*) os=windows; flags=""; exe=".exe" ;;
*) echo "crash-symbols: no recipe for $(uname -s)" >&2; exit 2 ;;
esac
py="$(command -v python3 || command -v python)"
RUSTFLAGS="$flags" cargo build -q --profile dist-symbols -p vieww-foundation --example crash_symbols
out=target/dist-symbols/examples
bin="$work/crash_symbols$exe"
cp "$out/crash_symbols$exe" "$bin"
case "$os" in
macos) [ -d "$out/crash_symbols.dSYM" ] && cp -R "$out/crash_symbols.dSYM" "$bin.dSYM" ;;
windows) cp "$out/crash_symbols.pdb" "$work/crash_symbols.pdb" ;;
esac
ci/release/symbols.sh "$bin" "$work/symbols"
case "$os" in
linux)
	if readelf -S "$bin" | grep -q '\.debug_line'; then
		echo "crash-symbols: the shipped binary still carries line tables" >&2
		exit 1
	fi
	;;
macos) rm -rf "$bin.dSYM" ;; # what ships has no dSYM beside it
windows) rm -f "$work/crash_symbols.pdb" ;;
esac
mkdir -p "$work/crashes"
VIEWW_CRASH_DIR="$work/crashes" "$bin" extra 2>/dev/null && { echo "crash-symbols: it did not crash" >&2; exit 1; }
report="$(ls "$work"/crashes/*)"
grep -E '^(message|frame: #[0-2] )' "$report"
"$py" -I ci/release/symbolize.py "$report" "$work/symbols" | tee "$work/symbolized.txt"
# Function names may come back demangled (`crash_symbols::outer_frame`,
# llvm-symbolizer) or in Rust's v0 mangling (`…13crash_symbols11outer_frame`,
# atos on macOS): both contain the bare name.
for want in 'outer_frame.*crash_symbols.rs:18' 'inner_frame.*crash_symbols.rs:12'; do
    if ! grep -q "$want" "$work/symbolized.txt"; then
        echo "crash-symbols: '$want' not recovered from the stripped binary's report" >&2
        exit 1
    fi
done
echo "crash-symbols ($os): a stripped release crash decoded to function and line"
