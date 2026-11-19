#!/usr/bin/env bash
#
# Train an aarch64 Android PGO profile **on the phone**, from the app itself.
#
#   ci/mobile/pgo-android.sh [SECONDS]        # default 120
#
# PENDING 4.2's phone measurements are why this exists: the CPU present path
# is the default on every phone, its rasterizer is the frame, and PGO is
# worth 5-15% on exactly those loops. A profile is per target and per
# workload — the x86_64 desktop gallery trained by `ci/check/pgo.sh` is
# neither — so the phone trains its own: this script builds the vieww demo
# APK instrumented (`-Cprofile-generate` + the example's `pgo-train`
# feature, which points LLVM's counters at the app's own storage in
# continuous mode — see `examples/android.rs`'s `pgo_train` module for why
# both are needed), installs it on the USB-connected device, lets the demo
# drive itself for a couple of minutes, force-stops it, and pulls and merges
# the counters the app wrote.
#
# The result is what the release build wants:
#
#     VIEWW_ANDROID_PGO_PROFILE=target/pgo-android/merged.profdata
#         # handed to the mobile-app android build job, or by hand:
#     RUSTFLAGS="-Cprofile-use=target/pgo-android/merged.profdata" \
#         cargo apk build -p <package> --lib --release
#
# # Why force-stop is safe here
#
# The counters are mmap'd (continuous mode, the `%c` the example asks for),
# so a SIGKILL — which is what `am force-stop` is — cannot lose them: every
# increment is already in the file. The example also writes explicitly at
# exit, for the paths where `%c` is not honoured.
#
# # The pixel check
#
# Not here, and deliberately: a profile cannot move a pixel (the same
# argument `ci/check/pgo.sh`'s header makes for the desktop), and the
# release gate's G3.0 row is the check that proves it for the binaries that
# ship. What this script proves is that the profile exists, is non-empty,
# and covers the workload — the merge fails loudly otherwise.
#
# Requires: a device on USB (`adb devices` shows it), the NDK (via
# ci/mobile/android-env.sh), the pinned toolchain (llvm-tools), and `run-as`
# reading the app's files (the demo's own package, debuggable in release via
# the android debug keystore is NOT enough — `run-as` needs a debug build,
# so this script builds the default release profile but installs with
# `adb install -r -d`; on a production build the counters are pulled by
# uninstalling instead, which loses them, so: run this on a userdebug device
# or an emulator, or make the demo debuggable for the training build).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

seconds="${1:-120}"
out="$root/target/pgo-android"
profile_dir="$out/profile"

# shellcheck source=ci/mobile/android-env.sh
. "$root/ci/mobile/android-env.sh"
android_env "pgo" || exit 1

command -v adb >/dev/null 2>&1 || { echo "pgo-android: adb not found" >&2; exit 2; }
device="$(adb devices | sed -n '2,/^$/p' | awk '$2 == "device" { print $1; exit }')"
[ -n "$device" ] || { echo "pgo-android: no device on USB (adb devices)" >&2; exit 2; }
echo "pgo-android: training on $device for ${seconds}s"

profdata="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-profdata"
[ -x "$profdata" ] || profdata="$(command -v llvm-profdata)" || {
    echo "pgo-android: llvm-profdata not found (llvm-tools-preview?)" >&2
    exit 2
}

mkdir -p "$profile_dir"

echo "pgo-android: stage 1 — the instrumented APK (pgo-train + -Cprofile-generate)"
RUSTFLAGS="-Cprofile-generate" \
    cargo apk build -p vieww-platform-winit --example android --release \
    --target aarch64-linux-android --features pgo-train
apk="$(find target -type f -name '*.apk' ! -name '*-unaligned.apk' -newermt '-15 minutes' 2>/dev/null | head -1)"
[ -n "$apk" ] || { echo "pgo-android: no APK was built" >&2; exit 1; }

echo "pgo-android: stage 2 — install, and let the demo drive itself"
adb install -r "$apk"
package="dev.vieww.demo"
adb shell am force-stop "$package" || true
adb logcat -c 2>/dev/null || true
adb shell am start -n "$package/android.app.NativeActivity"
echo "pgo-android: the demo runs its screens; waiting ${seconds}s"
sleep "$seconds"
echo "pgo-android: the counters so far (continuous mode writes as it runs):"
adb shell run-as "$package" ls -la files/ 2>/dev/null || true
adb shell am force-stop "$package" || true
sleep 2

echo "pgo-android: stage 3 — pull and merge"
rm -f "$profile_dir"/*.profraw
pulled=0
for f in $(adb shell run-as "$package" ls files/ 2>/dev/null | tr -d '\r'); do
    case "$f" in
    vieww-pgo-*.profraw)
        adb exec-out run-as "$package" cat "files/$f" > "$profile_dir/$f"
        pulled=$((pulled + 1))
        ;;
    esac
done
[ "$pulled" -gt 0 ] || {
    echo "pgo-android: no counter files came back — the app never wrote its counters." >&2
    echo "  Was the build --features pgo-train AND RUSTFLAGS=-Cprofile-generate?" >&2
    echo "  (Both together: the feature points the counters somewhere writable," >&2
    echo "   the instrumentation creates them. Either alone trains nothing.)" >&2
    exit 1
}
"$profdata" merge -output "$out/merged.profdata" "$profile_dir"/*.profraw

echo "pgo-android: the profile is $out/merged.profdata ($(wc -c <"$out/merged.profdata" | tr -d ' ') bytes, $pulled counter file(s))"
echo "  build the release APK with:"
echo "    RUSTFLAGS=\"-Cprofile-use=$out/merged.profdata\" cargo apk build -p <package> --lib --release"
echo "  or hand it to the CI job:"
echo "    VIEWW_ANDROID_PGO_PROFILE=$out/merged.profdata"
