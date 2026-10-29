#!/usr/bin/env bash
#
# Split a release binary into what ships and what decodes its crashes (FW-12).
#
#     ci/release/symbols.sh BINARY SYMBOLS_DIR
#
# BINARY is built with line tables (`--profile dist-symbols`, or
# packaging/package.sh --symbols). Its debug info goes to SYMBOLS_DIR under
# the id a crash report names it by (vieww_foundation::crash::raw_frames),
# with a <id>.json saying the format and load address
# (ci/release/module_id.py), so `ci/release/symbolize.py REPORT SYMBOLS_DIR`
# finds it:
#
#   ELF (Linux, Android .so)  <build-id>.debug; BINARY stripped in place and
#                             linked to it by .gnu_debuglink.
#   Mach-O (macOS, iOS)       <LC_UUID>.dSYM, from BINARY.dSYM when the build
#                             made one (cargo does) or dsymutil; BINARY
#                             stripped of debug symbols in place.
#   PE (Windows)              <GUID><age>/ holding BINARY and its .pdb (the
#                             debug info was never in the .exe; it ships as
#                             it is).
#
# Keep SYMBOLS_DIR with the release's artefacts, never in the app.
set -euo pipefail
bin="${1:?usage: symbols.sh BINARY SYMBOLS_DIR}"
dir="${2:?usage: symbols.sh BINARY SYMBOLS_DIR}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
py="$(command -v python3 || command -v python || true)"
[ -n "$py" ] || { echo "symbols.sh: no python" >&2; exit 2; }
meta="$("$py" -I "$here/module_id.py" "$bin")"
field() { "$py" -I -c "import json,sys; v=json.loads(sys.argv[1])[sys.argv[2]]; print('' if v is None else v)" "$meta" "$1"; }
fmt="$(field format)"
id="$(field id)"
base="$(field base)"
[ -n "$id" ] || { echo "symbols.sh: $bin ($fmt) carries no build id / UUID / CodeView record" >&2; exit 3; }
mkdir -p "$dir"
name="$(basename "$bin")"
size() { wc -c <"$1" | tr -d ' '; }
case "$fmt" in
elf)
	objcopy="$(command -v llvm-objcopy || command -v objcopy || true)"
	[ -n "$objcopy" ] || { echo "symbols.sh: no llvm-objcopy or objcopy" >&2; exit 2; }
	"$objcopy" --only-keep-debug "$bin" "$dir/$id.debug"
	"$objcopy" --strip-all --add-gnu-debuglink="$dir/$id.debug" "$bin"
	what="$dir/$id.debug ($(size "$dir/$id.debug") bytes)"
	;;
macho)
	rm -rf "$dir/$id.dSYM"
	if [ -d "$bin.dSYM" ]; then
		cp -R "$bin.dSYM" "$dir/$id.dSYM"
	else
		dsym="$(command -v dsymutil || command -v llvm-dsymutil || true)"
		[ -n "$dsym" ] || { echo "symbols.sh: no dsymutil and no $bin.dSYM" >&2; exit 2; }
		"$dsym" "$bin" -o "$dir/$id.dSYM"
	fi
	if command -v strip >/dev/null && [ "$(uname -s)" = Darwin ]; then
		strip -S "$bin"
	elif command -v llvm-strip >/dev/null; then
		llvm-strip --strip-debug "$bin"
	fi
	what="$dir/$id.dSYM"
	;;
pe)
	pdb="${bin%.*}.pdb"
	[ -f "$pdb" ] || { echo "symbols.sh: no $pdb next to $bin" >&2; exit 3; }
	mkdir -p "$dir/$id"
	cp "$bin" "$pdb" "$dir/$id/"
	what="$dir/$id/ ($(basename "$pdb"), $(size "$pdb") bytes)"
	;;
esac
printf '{"format": "%s", "base": %s, "name": "%s"}\n' "$fmt" "$base" "$name" >"$dir/$id.json"
echo "symbols.sh: $name ($fmt) id $id -> $what; shipping binary $(size "$bin") bytes"
