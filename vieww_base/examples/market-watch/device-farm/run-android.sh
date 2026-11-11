#!/usr/bin/env bash
#
# Valdi vs Vieww on one phone. Runs ON the AWS Device Farm Android host, once
# per device, from testspec-android.yml. Device Farm has installed the Vieww
# APK (the run's app); the Valdi APK rides in the test package.
#
# Three configurations, one after another, on the same phone:
#   vieww-cpu  Vieww, its CPU rasterizer (the default)
#   vieww-gpu  Vieww, its planned GPU path (debug.mw.gpu=1)
#   valdi      Valdi, native Android views drawn by HWUI
#
# For each one:
#   1. cold start x5: force-stop, `am start -W`, TotalTime — the platform's own
#      launch-to-first-frame figure, the same for both frameworks
#   2. one full run of the comparison schedule, which the app plays itself and
#      narrates as `__MW__ {json}` lines in logcat. This script follows logcat,
#      and at every phase boundary closes the phase it measured:
#        - SurfaceFlinger timestats for the app's layer (frames presented,
#          frame-to-frame histogram): the one yardstick that times a
#          NativeActivity surface (Vieww) and HWUI views (Valdi) the same way
#        - gfxinfo (HWUI's own per-frame numbers; Valdi only, Vieww draws no
#          HWUI frames)
#        - process CPU time (utime + stime) and RSS
#        - a screenshot after the event phases (load, tick, jump)
#   3. full meminfo, thermal status and battery temperature after the run
#
# Everything lands in $DEVICEFARM_LOG_DIR/mw/; result.txt there is what
# ci/mobile/device_farm.py reads for pass/fail.

set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${DEVICEFARM_LOG_DIR:-/tmp}/mw"
mkdir -p "$out"
result="$out/result.txt"
: > "$result"
export ANDROID_SERIAL="${DEVICEFARM_DEVICE_UDID:-}"
[ -n "$ANDROID_SERIAL" ] || unset ANDROID_SERIAL

VIEWW_PKG="dev.vieww.marketwatch"
VALDI_PKG="com.snap.valdi.marketwatch"
VALDI_APK="$here/valdi-market-watch.apk"
COLD_STARTS=5
RUN_TIMEOUT=420 # seconds for one schedule, thermal pass included

kv() { echo "$1=$2" >> "$result"; }
sh_() { adb shell "$@" 2>/dev/null | tr -d '\r'; }
now_ms() { date +%s%3N; }

# ── the phone, as it describes itself ────────────────────────────────────────
kv device_name "${DEVICEFARM_DEVICE_NAME:-unknown}"
kv model "$(sh_ getprop ro.product.model)"
kv android "$(sh_ getprop ro.build.version.release)"
kv sdk "$(sh_ getprop ro.build.version.sdk)"
soc="$(sh_ getprop ro.soc.model)"; kv soc "${soc:-$(sh_ getprop ro.board.platform)}"
kv cpu_cores "$(sh_ 'grep -c ^processor /proc/cpuinfo')"
kv mem_total_kb "$(sh_ 'grep MemTotal /proc/meminfo' | awk '{print $2}')"
kv display "$(sh_ wm size | awk -F': ' '/Physical/ {print $2}')"
kv density "$(sh_ wm density | awk -F': ' '/Physical/ {print $2}')"
kv refresh_hz "$(sh_ dumpsys display | grep -m1 -oE 'fps=[0-9.]+' | cut -d= -f2)"

sh_ svc power stayon true >/dev/null
sh_ settings put global hide_error_dialogs 1 >/dev/null
sh_ input keyevent KEYCODE_WAKEUP >/dev/null
sh_ wm dismiss-keyguard >/dev/null
# No animations from the system between runs; the apps' own frames are untouched.
for s in window_animation_scale transition_animation_scale animator_duration_scale; do
    sh_ settings put global "$s" 0 >/dev/null
done

# ── install ──────────────────────────────────────────────────────────────────
if ! sh_ pm list packages | grep -q "^package:$VIEWW_PKG$"; then
    [ -n "${DEVICEFARM_APP_PATH:-}" ] && adb install -r -g "$DEVICEFARM_APP_PATH" >> "$out/install.log" 2>&1
fi
adb install -r -g "$VALDI_APK" >> "$out/install.log" 2>&1
for pkg in "$VIEWW_PKG" "$VALDI_PKG"; do
    path="$(sh_ pm path "$pkg" | head -1 | cut -d: -f2)"
    if [ -z "$path" ]; then
        kv "missing_$pkg" yes
        echo "status=FAIL" >> "$result"
        exit 1
    fi
    kv "apk_bytes_$pkg" "$(sh_ stat -c %s "$path")"
done
[ -n "${DEVICEFARM_APP_PATH:-}" ] && kv apk_file_bytes_vieww "$(stat -c %s "$DEVICEFARM_APP_PATH")"
kv apk_file_bytes_valdi "$(stat -c %s "$VALDI_APK")"

component() {
    local c
    c="$(sh_ cmd package resolve-activity --brief "$1" | tail -1)"
    case "$c" in */*) echo "$c" ;; *) echo "$1/android.app.NativeActivity" ;; esac
}

# SurfaceFlinger names a layer after its activity; this finds the app's one.
layer_of() { sh_ dumpsys SurfaceFlinger --list | grep -m1 "$1" ; }

cpu_ticks() { # utime + stime of a pid, in clock ticks
    sh_ cat "/proc/$1/stat" | awk '{print $14 + $15}'
}

# ── one configuration ────────────────────────────────────────────────────────
run_config() {
    local name="$1" pkg="$2" gpu="$3" tag="$4"
    local dir="$out/$name"
    mkdir -p "$dir"
    local comp; comp="$(component "$pkg")"
    sh_ setprop debug.mw.gpu "$gpu" >/dev/null
    sh_ setprop debug.mw.thermal 1 >/dev/null

    # 1. cold start x5
    local starts=()
    for _ in $(seq "$COLD_STARTS"); do
        sh_ am force-stop "$pkg" >/dev/null
        sh_ pm trim-caches 999G >/dev/null 2>&1 || true
        sleep 1
        t="$(sh_ am start -W -n "$comp" | awk -F': ' '/TotalTime/ {print $2}')"
        starts+=("${t:-NA}")
        sleep 2
    done
    sh_ am force-stop "$pkg" >/dev/null
    kv "${name}_cold_start_ms" "${starts[*]}"

    # 2. the schedule
    sleep 3
    adb logcat -c
    sh_ dumpsys SurfaceFlinger --timestats -disable >/dev/null
    sh_ dumpsys SurfaceFlinger --timestats -clear >/dev/null
    sh_ dumpsys SurfaceFlinger --timestats -enable >/dev/null
    sh_ am start -n "$comp" >/dev/null
    local pid="" phase="" t_phase cpu_phase layer=""
    local events="$dir/events.jsonl" runner="$dir/runner.jsonl"
    : > "$events"; : > "$runner"
    local deadline=$(( $(date +%s) + RUN_TIMEOUT ))

    close_phase() { # $1 = phase that just ended
        local p="$1" t_end cpu_end rss
        t_end="$(now_ms)"
        cpu_end="$(cpu_ticks "$pid")"
        rss="$(sh_ cat "/proc/$pid/status" | awk '/VmRSS/ {print $2}')"
        sh_ dumpsys SurfaceFlinger --timestats -dump > "$dir/timestats-$p.txt"
        sh_ dumpsys SurfaceFlinger --timestats -clear >/dev/null
        sh_ dumpsys gfxinfo "$pkg" > "$dir/gfxinfo-$p.txt"
        sh_ dumpsys gfxinfo "$pkg" reset >/dev/null
        case "$p" in load*|tick*|jump*) adb exec-out screencap -p > "$dir/shot-$p.png" ;; esac
        printf '{"phase":"%s","wall_ms":%s,"cpu_ticks":%s,"rss_kb":%s,"layer":"%s"}\n' \
            "$p" "$(( t_end - t_phase ))" "$(( cpu_end - cpu_phase ))" "${rss:-0}" "$layer" >> "$runner"
    }

    local status="TIMEOUT"
    while IFS= read -r -t 30 line || [ $? -gt 128 ]; do
        [ "$(date +%s)" -gt "$deadline" ] && break
        case "$line" in *__MW__*) ;; *) continue ;; esac
        json="${line#*__MW__ }"
        echo "$json" >> "$events"
        case "$json" in
            *'"ev":"start"'*)
                pid="$(sh_ pidof "$pkg")"
                kv "${name}_pid" "$pid"
                ;;
            *'"ev":"phase"'*)
                [ -n "$phase" ] && close_phase "$phase"
                phase="$(sed -E 's/.*"phase":"([^"]+)".*/\1/' <<< "$json")"
                [ -z "$layer" ] && layer="$(layer_of "$pkg")"
                [ -z "$pid" ] && pid="$(sh_ pidof "$pkg")"
                t_phase="$(now_ms)"; cpu_phase="$(cpu_ticks "$pid")"
                [ "$phase" = thermal ] && sh_ dumpsys thermalservice > "$dir/thermal-before.txt"
                ;;
            *'"ev":"done"'*)
                [ -n "$phase" ] && close_phase "$phase"
                status="PASS"
                break
                ;;
        esac
    done < <(adb logcat -v raw -s "$tag:I")
    sh_ dumpsys SurfaceFlinger --timestats -disable >/dev/null

    # 3. after the run
    sh_ dumpsys meminfo "$pkg" > "$dir/meminfo.txt"
    sh_ dumpsys thermalservice > "$dir/thermal-after.txt"
    kv "${name}_battery_temp_dC" "$(sh_ dumpsys battery | awk -F': ' '/temperature/ {print $2}')"
    kv "${name}_pss_kb" "$(awk '/TOTAL PSS:/ {print $3; exit} /^ *TOTAL / {print $2; exit}' "$dir/meminfo.txt")"
    adb logcat -d -v threadtime > "$dir/logcat.txt"
    sh_ am force-stop "$pkg" >/dev/null
    kv "${name}_status" "$status"
    [ "$status" = PASS ]
}

overall=PASS
# A minute of rest between configurations, so one run's heat is not the
# next run's throttle. The order alternates frameworks for the same reason.
run_config vieww-cpu "$VIEWW_PKG" 0 vieww || overall=FAIL
sleep 60
run_config valdi "$VALDI_PKG" 0 Valdi || overall=FAIL
sleep 60
run_config vieww-gpu "$VIEWW_PKG" 1 vieww || overall=FAIL
sh_ setprop debug.mw.gpu 0 >/dev/null

echo "status=$overall" >> "$result"
ls -la "$out" > "$out/listing.txt"
[ "$overall" = PASS ]
