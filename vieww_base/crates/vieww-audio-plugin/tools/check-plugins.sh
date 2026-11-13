#!/usr/bin/env bash
# Build the example plugins and prove the CLAP, VST3 and LV2 transcriptions
# are the formats' ABIs, not merely self-consistent.
#
#   tools/check-plugins.sh          build, bundle, C/C++ header hosts, Rust hosts
#   tools/check-plugins.sh --full   also free-audio/clap-validator and
#                                   Steinberg's VST3 SDK validator
#
# The CLAP headers, the VST3 interface headers, the LV2 headers,
# clap-validator and the VST3 SDK are fetched (pinned to the revisions the
# wrappers were validated against) into $WORK — they are test tools only;
# vieww-audio-plugin depends on none of them. Works on Linux and macOS; on
# Windows (Git Bash) the C/C++ hosts are skipped unless a gcc/g++ is on
# PATH.
#
# lv2lint is deliberately not run: it needs the meson-built lilv/serd/sord
# stack, which is heavy for a CI leg and mostly re-states the TTL checks
# the in-house host already asserts (the C host below reads the same
# manifests). The gate here is the in-house Rust host plus the C host
# compiled against the official headers.
set -euo pipefail

FULL=0
[[ "${1:-}" == "--full" ]] && FULL=1

CRATE="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$CRATE/../.." && pwd)"
WORK="${WORK:-$ROOT/target/plugin-check}"
OUT="$ROOT/target/plugins"
mkdir -p "$WORK"

CLAP_REV=a47f6badb49d948fd009998f28309cdab78979c9            # CLAP 1.2.x headers
VST3_IF_REV=4f547e8e102b47de4a8b8aaf343c73b700786372         # vst3_pluginterfaces 3.8.1
VST3_SDK_REV=3cdf9ca5d1f5b1b21e0a86832aa4abe55607bd96        # vst3sdk 3.8.1
CLAP_VALIDATOR_REV=b2f1d9b79b1d264a5747f46707d72b1aa40a02ef  # clap-validator 0.4.1
LV2_REV=3c57dae600a5ad8d05acd53ee3490f7d91cb7be6            # lv2 headers (include/lv2)

case "$(uname -s)" in
  Linux*)  OS=linux; LIB_PREFIX=lib; LIB_EXT=so ;;
  Darwin*) OS=macos; LIB_PREFIX=lib; LIB_EXT=dylib ;;
  *)       OS=windows; LIB_PREFIX=; LIB_EXT=dll ;;
esac
ARCH="$(uname -m)"; [[ "$ARCH" == "arm64" ]] && ARCH=aarch64

fetch() { # url rev dir [submodules...]
  local url=$1 rev=$2 dir=$3; shift 3
  if [[ ! -d "$WORK/$dir/.git" ]]; then
    git init -q "$WORK/$dir"
    git -C "$WORK/$dir" remote add origin "$url"
  fi
  git -C "$WORK/$dir" fetch -q --depth 1 origin "$rev"
  git -C "$WORK/$dir" checkout -q FETCH_HEAD
  for m in "$@"; do git -C "$WORK/$dir" submodule update -q --init --depth 1 "$m"; done
}

echo "== building the example plugins (static std, release)"
# The workspace links std dynamically for vieww Studio's preview reloading
# (.cargo/config.toml); a plugin a DAW loads must not. RUSTFLAGS replaces the
# config's flags wholesale.
RUSTFLAGS="-C debuginfo=0" CARGO_TARGET_DIR="$OUT" \
  cargo build -q --release -p vieww-audio-plugin --examples
BIN="$OUT/release/examples"
for p in vieww_gain vieww_synth; do
  f="$BIN/${LIB_PREFIX}${p}.${LIB_EXT}"
  [[ -f "$f" ]] || { echo "missing $f"; exit 1; }
  if [[ $OS == linux ]] && ldd "$f" | grep -q libstd; then
    echo "$f links libstd dynamically"; exit 1
  fi
done

echo "== bundling"
"$CRATE/tools/bundle.sh" "$BIN" "$OUT/bundles" vieww_gain vieww_synth

echo "== Rust hosts loading the built plugins"
VIEWW_PLUGIN_DIR="$BIN" cargo test -q -p vieww-audio-plugin --test load_built_plugins

echo "== Rust LV2 host writing and loading the built bundles"
# The tests write the TTLs into the bundles bundle.sh laid out (and the
# library into each), then drive them through the dlopen host.
VIEWW_PLUGIN_DIR="$BIN" VIEWW_LV2_BUNDLE_DIR="$OUT/bundles" \
  cargo test -q -p vieww-audio-plugin --test lv2_bundle --test lv2_synth_bundle

if command -v cc >/dev/null && command -v c++ >/dev/null; then
  echo "== C/C++ hosts compiled against the official headers"
  fetch https://github.com/free-audio/clap "$CLAP_REV" clap
  fetch https://github.com/steinbergmedia/vst3_pluginterfaces "$VST3_IF_REV" vst3_pluginterfaces
  fetch https://github.com/lv2/lv2 "$LV2_REV" lv2
  mkdir -p "$WORK/inc"; ln -sfn ../vst3_pluginterfaces "$WORK/inc/pluginterfaces"
  DL=""; [[ $OS == linux ]] && DL="-ldl"
  # Steinberg's funknown.cpp generates FUIDs through CoreFoundation on
  # macOS (without the framework the link dies on _CFRelease/_CFUUIDCreate,
  # cloud-vm 2026-11-11) and through CoCreateGuid on Windows, where the
  # mingw toolchain this script runs under does not link uuid for you the
  # way MSVC's SDK pragmas do (__imp_CoCreateGuid, 1-check 2026-11-13).
  # Linux needs nothing of the sort.
  FW=""; [[ $OS == macos ]] && FW="-framework CoreFoundation"
  UUID=""; [[ $OS == windows ]] && UUID="-luuid"
  cc -std=c11 -O1 -I"$WORK/clap/include" "$CRATE/tools/clap_host.c" -o "$WORK/clap_host" $DL -lm
  c++ -std=c++17 -O1 -I"$WORK/inc" "$CRATE/tools/vst3_host.cpp" \
    "$WORK/vst3_pluginterfaces/base/funknown.cpp" -o "$WORK/vst3_host" $DL $UUID $FW
  cc -std=c11 -O1 -I"$WORK/lv2/include" "$CRATE/tools/lv2_host.c" -o "$WORK/lv2_host" $DL -lm
  cc -std=c11 -I"$WORK/clap/include" "$CRATE/tools/clap_layout.c" -o "$WORK/clap_layout"
  c++ -std=c++17 -I"$WORK/inc" "$CRATE/tools/vst3_layout.cpp" -o "$WORK/vst3_layout"
  cc -std=c11 -I"$WORK/lv2/include" "$CRATE/tools/lv2_layout.c" -o "$WORK/lv2_layout"
  echo "-- layouts on this platform (compare with src/*/sys.rs layout tests)"
  "$WORK/clap_layout" | head -8
  "$WORK/vst3_layout" | head -12
  "$WORK/lv2_layout"
  cc -std=c11 -O2 -I"$WORK/clap/include" "$CRATE/tools/denormal_bench.c" -o "$WORK/denormal_bench" $DL
  for p in vieww_gain vieww_synth; do
    "$WORK/clap_host" "$BIN/${LIB_PREFIX}${p}.${LIB_EXT}"
    "$WORK/vst3_host" "$BIN/${LIB_PREFIX}${p}.${LIB_EXT}"
    "$WORK/lv2_host" "$OUT/bundles/$p.lv2"
    echo "-- $p: process() on normal vs subnormal input"
    "$WORK/denormal_bench" "$BIN/${LIB_PREFIX}${p}.${LIB_EXT}"
  done
else
  echo "== (no C/C++ compiler: header hosts skipped)"
fi

if [[ $FULL == 1 ]]; then
  echo "== free-audio/clap-validator"
  fetch https://github.com/free-audio/clap-validator "$CLAP_VALIDATOR_REV" clap-validator
  (cd "$WORK/clap-validator" && cargo build -q --release)
  CV="$WORK/clap-validator/target/release/clap-validator"
  for p in vieww_gain vieww_synth; do
    "$CV" validate --json "$OUT/bundles/$p.clap" > "$WORK/$p.clap-validator.json" || true
    python3 - "$WORK/$p.clap-validator.json" "$p" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1])); bad = 0; counts = {}
for r in d["results"]:
    code = r["status"]["code"]; counts[code] = counts.get(code, 0) + 1
    if code not in ("success", "skipped"):
        # Warnings are timing heuristics (see tools/denormal_bench.c for the
        # one that matters); failures fail the check.
        bad += code != "warning"
        print(" ", code, list(r["test"].values())[0]["test"], r["status"]["details"])
print(f"  {sys.argv[2]}: {counts}")
sys.exit(1 if bad else 0)
EOF
  done

  echo "== Steinberg VST3 validator"
  fetch https://github.com/steinbergmedia/vst3sdk "$VST3_SDK_REV" vst3sdk base cmake pluginterfaces public.sdk
  # Only the validator: the other hosting samples want GUI toolkits.
  echo 'add_subdirectory(validator)' > "$WORK/vst3sdk/public.sdk/samples/vst-hosting/CMakeLists.txt"
  cmake -S "$WORK/vst3sdk" -B "$WORK/vst3sdk/build" -DCMAKE_BUILD_TYPE=Release \
    -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF -DSMTG_ENABLE_VST3_PLUGIN_EXAMPLES=OFF \
    -DSMTG_ENABLE_VST3_HOSTING_EXAMPLES=ON -DSMTG_RUN_VST_VALIDATOR=OFF \
    -DSMTG_CREATE_PLUGIN_LINK=OFF > "$WORK/vst3sdk-cmake.log"
  cmake --build "$WORK/vst3sdk/build" --config Release --target validator -j 4 > "$WORK/vst3sdk-build.log"
  VV="$(find "$WORK/vst3sdk/build" -type f \( -name validator -o -name validator.exe \) | head -1)"
  for p in vieww_gain vieww_synth; do
    "$VV" -e "$OUT/bundles/$p.vst3" > "$WORK/$p.vst3-validator.log" 2>&1 || { tail -40 "$WORK/$p.vst3-validator.log"; exit 1; }
    grep "Result:" "$WORK/$p.vst3-validator.log" | sed "s/^/  $p: /"
    grep -q "0 tests failed" "$WORK/$p.vst3-validator.log"
  done
fi

echo "== all plugin checks passed"
