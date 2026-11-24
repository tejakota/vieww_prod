#!/usr/bin/env bash
#
# Market Watch on a phone plugged into this computer by USB: no Device Farm,
# no Firebase credits. Runs the same two scenarios Test Lab runs (1: CPU
# rasterizer, 2: GPU path) through `adb`, then turns the log into the same
# report the farm jobs produce.
#
#   usb/run.sh path/to/vieww-market-watch.apk [out-dir]
#
# Needs: adb on PATH, python3, exactly one phone attached with USB debugging
# on. Takes about 6 minutes: ~70 s per scenario, plus a rest between them so
# the first run's heat does not throttle the second.
#
# Optional, before running:
#   SKIP_THERMAL=1   skip the 60 s thermal pass (about 2.5 minutes in total)
#   REST=60          seconds to rest between scenarios (default 60)

set -euo pipefail

apk="${1:?usage: usb/run.sh path/to/vieww-market-watch.apk [out-dir]}"
out="${2:-usb-out}"
pkg=dev.vieww.marketwatch
here="$(cd "$(dirname "$0")" && pwd)"
rest="${REST:-60}"

command -v adb >/dev/null || { echo "adb not found (install Android platform-tools)"; exit 1; }
[ -f "$apk" ] || { echo "no such file: $apk"; exit 1; }
[ "$(adb devices | awk 'NR>1 && $2=="device"' | wc -l)" -eq 1 ] || {
  echo "need exactly one authorised phone:"; adb devices -l; exit 1; }

sh_() { adb shell "$@" | tr -d '\r'; }
abi="$(sh_ getprop ro.product.cpu.abi)"
echo "phone: $(sh_ getprop ro.product.manufacturer) $(sh_ getprop ro.product.model), Android $(sh_ getprop ro.build.version.release), $abi"
case "$abi" in arm64*) ;; *) echo "this APK is arm64; the phone is $abi"; exit 1;; esac
echo "memory: $(sh_ 'grep MemTotal /proc/meminfo')  cores: $(sh_ 'grep -c ^processor /proc/cpuinfo')"

mkdir -p "$out/results/usb/phone"
adb install -r -g "$apk" >/dev/null
echo "installed $(stat -c %s "$apk" 2>/dev/null || wc -c <"$apk") bytes"

# Keep the screen on and the phone awake: the app must be in the foreground.
adb shell input keyevent KEYCODE_WAKEUP || true
adb shell svc power stayon usb || true
# `1` runs the 60 s thermal pass, `0` skips it. (An empty value cannot be
# sent through `adb shell`: the empty argument is dropped.)
adb shell setprop debug.mw.thermal "$([ "${SKIP_THERMAL:-0}" = 1 ] && echo 0 || echo 1)"

log="$out/results/usb/phone/logcat"
: > "$log"
for scenario in 1 2; do
  [ "$scenario" = 2 ] && { echo "resting ${rest}s"; sleep "$rest"; }
  adb shell am force-stop "$pkg"
  adb logcat -c
  echo "scenario $scenario ($([ $scenario = 1 ] && echo CPU || echo GPU)) ..."
  adb shell am start -W -a com.google.intent.action.TEST_LOOP -t application/javascript \
      --ei scenario "$scenario" -p "$pkg" >/dev/null
  # Wait for the app's own `done` line (it exits itself), up to 8 minutes.
  for _ in $(seq 1 240); do
    if adb logcat -d | grep -q '__MW__ {"fw":"vieww","ev":"done"'; then break; fi
    sleep 2
  done
  adb logcat -d >> "$log"
done
adb shell am force-stop "$pkg"
adb shell svc power stayon false || true
adb shell setprop debug.mw.thermal 1 || true

# Same converter and report as Test Lab: the app narrates the same lines.
python3 "$here/../firebase/to_farm.py" --results "$out/results" --out "$out/farm" --apk "$apk"
python3 "$here/../compare_report.py" --farm "$out/farm" --out "$out/report" \
  --header "USB phone · $(sh_ getprop ro.product.model) · vieww $(git -C "$here" rev-parse --short HEAD 2>/dev/null || echo local)" >/dev/null
echo
echo "report: $out/report/COMPARE.md"
echo "send me the whole '$out' folder (zipped)"
