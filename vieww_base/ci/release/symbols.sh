#!/usr/bin/env bash
#
# Split a release binary into what ships and what decodes its crashes (FW-12).
#
#     ci/release/symbols.sh BINARY SYMBOLS_DIR
#
# BINARY is built with `--profile dist-symbols` (the `dist` profile with
# line tables kept). This writes its debug info to
# SYMBOLS_DIR/<build-id>.debug, strips BINARY in place (what ships, the same
# code `--profile dist` produces) and links the two by build id and
# .gnu_debuglink. A crash report names the build id with every frame
# (vieww_foundation::crash::raw_frames), so `ci/release/symbolize.py REPORT
# SYMBOLS_DIR` finds the right file. Keep SYMBOLS_DIR with the release's
# artefacts, never in the app.
#
# ELF (Linux, Android .so). Apple builds keep their dSYM (`dsymutil`) and
# Windows its PDB; both are already separate, named by the LC_UUID and the
# CodeView GUID the report also carries.
set -euo pipefail
bin="${1:?usage: symbols.sh BINARY SYMBOLS_DIR}"
dir="${2:?usage: symbols.sh BINARY SYMBOLS_DIR}"
objcopy="$(command -v llvm-objcopy || command -v objcopy || true)"
[ -n "$objcopy" ] || { echo "symbols.sh: no llvm-objcopy or objcopy" >&2; exit 2; }
id="$(LC_ALL=C readelf -n "$bin" 2>/dev/null | awk '/Build ID:/ {print $3; exit}')"
[ -n "$id" ] || { echo "symbols.sh: $bin has no build id (link with --build-id)" >&2; exit 3; }
mkdir -p "$dir"
"$objcopy" --only-keep-debug "$bin" "$dir/$id.debug"
"$objcopy" --strip-all --add-gnu-debuglink="$dir/$id.debug" "$bin"
echo "symbols.sh: $(basename "$bin") build-id $id -> $dir/$id.debug ($(stat -c %s "$dir/$id.debug") bytes); shipping binary $(stat -c %s "$bin") bytes"
