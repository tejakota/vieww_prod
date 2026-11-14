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
#      and at every phase boundary closes the phase it measured. A production
#      Valdi narrates nothing (its runtime logs are compiled out under
#      --snap_flavor=production), so the runner also carries the schedule
#      itself and closes the same boundaries by its own clock — see the long
#      comment inside run_config. Either way, at every boundary:
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
# Listed first, matched after: `pm list | grep -q` can fail under pipefail
# when grep exits at the match, which would read as "not installed".
packages="$(sh_ pm list packages)"
if ! grep -q "^package:$VIEWW_PKG$" <<< "$packages"; then
    [ -n "${DEVICEFARM_APP_PATH:-}" ] && adb install -r -g "$DEVICEFARM_APP_PATH" >> "$out/install.log" 2>&1
fi
# The Valdi APK rides in the test package. Say what the host has before
# installing it, so a missing or truncated file is not mistaken for a phone
# refusing a good APK.
if [ -f "$VALDI_APK" ]; then
    kv apk_file_bytes_valdi "$(stat -c %s "$VALDI_APK")"
    kv valdi_apk_sha256 "$(sha256sum "$VALDI_APK" | cut -c1-16)"
else
    kv valdi_apk_on_host "missing: $VALDI_APK"
    ls -la "$here" > "$out/package-listing.txt" 2>&1
fi
valdi_install="$(adb install -r -g "$VALDI_APK" 2>&1)"
printf '%s\n' "$valdi_install" >> "$out/install.log"
# A copy from an earlier run, signed with a different key, blocks `install -r`
# (the CI's debug key is new each run). Remove it and install fresh, once.
if grep -q 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' <<< "$valdi_install"; then
    sh_ pm uninstall "$VALDI_PKG" >> "$out/install.log"
    valdi_install="$(adb install -g "$VALDI_APK" 2>&1)"
    printf 'retry after uninstall:\n%s\n' "$valdi_install" >> "$out/install.log"
fi

# One line per app, in result.txt itself: the reason a phone refused an APK
# (INSTALL_FAILED_…, INSTALL_PARSE_FAILED_…) reaches the report, not only the
# artifact. An app that is not there skips its configurations; the others
# still run, so one failed install does not cost the whole phone.
installed() { # $1 package → prints the on-device path, if installed
    sh_ pm path "$1" | sed -n 's/^package://p' | sed -n 1p
}
declare -A have=()
for pkg in "$VIEWW_PKG" "$VALDI_PKG"; do
    path="$(installed "$pkg")"
    if [ -n "$path" ]; then
        have[$pkg]=yes
        kv "apk_bytes_$pkg" "$(sh_ stat -c %s "$path")"
    else
        have[$pkg]=no
        kv "missing_$pkg" yes
    fi
done
if [ "${have[$VALDI_PKG]}" = no ]; then
    reason="$(grep -oE '(INSTALL_[A-Z_]+|Failure \[[^]]*\]|adb: [^\n]*|error: [^\n]*)' <<< "$valdi_install" | tail -1)"
    kv install_error_valdi "${reason:-$(tail -1 <<< "$valdi_install")}"
fi
[ -n "${DEVICEFARM_APP_PATH:-}" ] && kv apk_file_bytes_vieww "$(stat -c %s "$DEVICEFARM_APP_PATH")"

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

# ── the schedule both apps play ─────────────────────────────────────────────
# market_watch::schedule (examples/market-watch/src/lib.rs), mirrored by the
# Valdi app's schedule() in compare/valdi/apps/market_watch/src/MarketWatch.tsx
# — `cargo test -p market-watch` checks the two agree. After a 1500 ms settle
# (both apps sleep exactly that before load2k) these are the phases and their
# lengths in seconds. The runner keeps it as a clock: a configuration that
# narrates nothing still gets its phases closed at the boundaries the app
# itself is closing them at.
MW_SETTLE_MS=1500
mw_schedule() { # $1 = thermal pass included → "name seconds" per line
    local rows k
    for rows in 2000 20000; do
        k=$(( rows / 1000 ))k
        printf 'load%s 2\nidle%s 2\ntick%s 1\njump%s 1\nscroll%s 8\nlive%s 8\nstorm%s 8\n' \
            "$k" "$k" "$k" "$k" "$k" "$k" "$k"
    done
    [ "${1:-0}" = 1 ] && printf 'load2k-thermal 2\nthermal 60\n'
    return 0
}

layer_frames() { # $1 timestats dump, $2 package → frames the package's layers presented
    [ -f "$1" ] || { echo 0; return; }
    awk -v pkg="$2" '
        /^layerName = / { inlayer = index($0, pkg) > 0 }
        inlayer && /totalFrames = / {
            for (i = 1; i <= NF; i++) if ($i == "totalFrames") { sum += $(i + 2); break }
        }
        END { print sum + 0 }' "$1"
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
    # The schedule the app plays, as a cursor: m_off is where the app is in
    # it, in ms since the launch. m_last is the schedule's final phase, whose
    # timestats the health check reads.
    local -a sched=()
    local launch_ms; launch_ms="$(now_ms)"
    local m_idx=0 m_off="$MW_SETTLE_MS" m_name="" m_secs=0 m_last=""
    while IFS= read -r line; do sched+=("$line"); done < <(mw_schedule 1)
    m_last="$(cut -d' ' -f1 <<< "${sched[${#sched[@]}-1]}")"
    # A phase closes exactly once — by its event, or by the clock, whichever
    # lands first; the two agree to well inside a poll.
    local -A closed=()

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

    enter_phase() { # $1 = phase name, starting now
        phase="$1"
        [ -z "$layer" ] && layer="$(layer_of "$pkg")"
        [ -z "$pid" ] && pid="$(sh_ pidof "$pkg")"
        t_phase="$(now_ms)"; cpu_phase="$(cpu_ticks "$pid")"
        [ "$phase" = thermal ] && sh_ dumpsys thermalservice > "$dir/thermal-before.txt"
    }

    local status="TIMEOUT"
    # Two clocks close the same boundaries.
    #
    # The first is the apps' own narration: every phase boundary is an
    # `__MW__ {json}` line in logcat, followed through polls of the dump —
    # the farm host's adb is a wrapper that refuses what it has not
    # whitelisted: a first attempt read `adb logcat -v raw`, and every poll
    # answered "`adb raw` is not permitted" (419 of them on the 2026-11-14
    # run) while the apps played their schedules unheard, and the run before
    # that fed the same refusal to a `read -t` loop that treated it as
    # end-of-stream. The dump is the form the device suite has always used
    # on the same host.
    #
    # The second is the schedule itself, kept as a clock: a production Valdi
    # narrates nothing — its runtime logs are compiled out
    # (--snap_flavor=production leaves SC_LOGGING_COMPILED_IN() false and the
    # JS console gate shut), and the 2026-11-14 run watched all three phones
    # play the whole schedule to its last frame — drawing stopped exactly at
    # the thermal phase's end — without one `__MW__` line ever reaching
    # logcat; vieww PASSED on the same phones while valdi timed out on every
    # one. So a phase whose window has passed closes at the same offset from
    # the launch at which the app itself closes it (both apps settle 1.5 s,
    # then the same lengths). Narration stays primary where it exists — an
    # event also re-aligns the cursor — and the clock only fills the silence.
    #
    # Consecutive dumps overlap — the buffer keeps its lines until they roll
    # — so a recurring line is recognised by its "t" (the app's own
    # monotonic milliseconds, which both apps put at the end of exactly the
    # lines that recur) and taken once; the one-time `start` line carries no
    # t and is taken once by flag.
    local last_t="-1" seen_start=false seen_at=""
    # One poll's worth of the narration, from the logcat dump. A function so
    # the loop's last act — after the clock has played the whole schedule —
    # can still drain the final boundary's `stats` and `done` lines, which
    # otherwise land in a dump nobody reads.
    scan_dump() {
        local dump="$dir/dump.txt" line json t ev ev_phase i off sn ss
        if [ -n "$pid" ]; then
            adb logcat -d --pid="$pid" > "$dump" 2>/dev/null || true
        else
            adb logcat -d > "$dump" 2>/dev/null || true
        fi
        while IFS= read -r line; do
            case "$line" in *__MW__*) ;; *) continue ;; esac
            json="${line#*__MW__ }"
            case "$json" in
                *'"ev":"start"'*)
                    # The one line with no t: once, by flag.
                    [ "$seen_start" = true ] && continue
                    seen_start=true
                    pid="$(sh_ pidof "$pkg")"
                    kv "${name}_pid" "$pid"
                    ;;
                *)
                    # Once, by t: this dump and every later one hold it. Two
                    # distinct lines can share a t — a boundary's stats and
                    # its phase are microseconds apart — so a tie also has
                    # to name an event kind not yet taken at that t.
                    t="$(sed -nE 's/.*"t":([0-9]+(\.[0-9]+)?).*/\1/p' <<< "$json" | head -1)"
                    [ -n "$t" ] || continue
                    ev="$(sed -nE 's/.*"ev":"([a-z]+)".*/\1/p' <<< "$json" | head -1)"
                    if awk -v a="$t" -v b="$last_t" 'BEGIN { exit (a > b) ? 0 : 1 }'; then
                        last_t="$t"; seen_at="|$ev|"
                    elif awk -v a="$t" -v b="$last_t" 'BEGIN { exit (a == b) ? 0 : 1 }' \
                        && ! grep -q "|$ev|" <<< "$seen_at"; then
                        seen_at="$seen_at$ev|"
                    else
                        continue
                    fi
                    ;;
            esac
            echo "$json" >> "$events"
            case "$json" in
                *'"ev":"phase"'*)
                    ev_phase="$(sed -E 's/.*"phase":"([^"]+)".*/\1/' <<< "$json")"
                    if [ "$ev_phase" != "$phase" ] && [ -z "${closed[$ev_phase]:-}" ]; then
                        if [ -n "$phase" ] && [ -z "${closed[$phase]:-}" ]; then
                            close_phase "$phase"; closed["$phase"]=1
                        fi
                        enter_phase "$ev_phase"
                        kv "${name}_pid" "$pid"
                        # Keep the clock cursor with the app: an event is the
                        # app's own statement of where it is.
                        i=0; off="$MW_SETTLE_MS"
                        while [ "$i" -lt "${#sched[@]}" ]; do
                            read -r sn ss <<< "${sched[$i]}"
                            if [ "$sn" = "$ev_phase" ]; then m_idx="$i"; m_off="$off"; break; fi
                            off=$(( off + ss * 1000 )); i=$(( i + 1 ))
                        done
                    fi
                    ;;
                *'"ev":"done"'*)
                    if [ -n "$phase" ] && [ -z "${closed[$phase]:-}" ]; then
                        close_phase "$phase"; closed["$phase"]=1
                    fi
                    status="PASS"
                    break
                    ;;
            esac
        done < <(tr -d '\r' < "$dump" | grep '__MW__' || true)
    }
    while :; do
        [ "$(date +%s)" -gt "$deadline" ] && break
        [ -n "$pid" ] || pid="$(sh_ pidof "$pkg")"

        # (a) the mirrored clock: enter a phase whose window has begun, close
        #     one whose window has ended — catching up over stalled polls.
        while [ "$m_idx" -lt "${#sched[@]}" ]; do
            read -r m_name m_secs <<< "${sched[$m_idx]}"
            [ $(( $(now_ms) - launch_ms )) -ge "$m_off" ] || break
            if [ "$phase" != "$m_name" ] && [ -z "${closed[$m_name]:-}" ]; then
                if [ -n "$phase" ] && [ -z "${closed[$phase]:-}" ]; then
                    close_phase "$phase"; closed["$phase"]=1
                fi
                enter_phase "$m_name"
            fi
            if [ $(( $(now_ms) - launch_ms )) -ge $(( m_off + m_secs * 1000 )) ]; then
                [ -z "${closed[$m_name]:-}" ] && { close_phase "$m_name"; closed["$m_name"]=1; }
                m_off=$(( m_off + m_secs * 1000 )); m_idx=$(( m_idx + 1 ))
                if [ "$m_idx" -ge "${#sched[@]}" ]; then status="PASS"; break; fi
            else
                break
            fi
        done
        if [ "$status" = PASS ]; then
            # The clock played the whole schedule. An app that narrates is
            # still finishing the last boundary: one final scan drains its
            # `stats` and `done` lines into events.jsonl before we go.
            scan_dump
            break
        fi

        # (b) the narration
        scan_dump
        [ "$status" = PASS ] && break
        sleep 0.3
    done
    sh_ dumpsys SurfaceFlinger --timestats -disable >/dev/null

    # A schedule that ran with the app dead still passes the clock above, so
    # a PASS also asks the compositor: the app's layer must have presented
    # frames in the schedule's last phase — 60 s of live load, so a live app
    # presents thousands; a crashed one, none.
    if [ "$status" = PASS ] && [ -n "$m_last" ]; then
        frames="$(layer_frames "$dir/timestats-$m_last.txt" "$pkg")"
        if [ "${frames:-0}" -eq 0 ]; then
            status="FAIL (no frames presented in $m_last: the app is not drawing)"
        fi
    fi

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
# A configuration whose app did not install is recorded as SKIPPED and fails
# the phone (the comparison is incomplete), but the others still run.
first=yes
for spec in "vieww-cpu $VIEWW_PKG 0 vieww" "valdi $VALDI_PKG 0 Valdi" "vieww-gpu $VIEWW_PKG 1 vieww"; do
    read -r name pkg gpu tag <<< "$spec"
    if [ "${have[$pkg]}" != yes ]; then
        kv "${name}_status" "SKIPPED (not installed)"
        overall=FAIL
        continue
    fi
    [ "$first" = yes ] || sleep 60
    first=no
    run_config "$name" "$pkg" "$gpu" "$tag" || overall=FAIL
done
sh_ setprop debug.mw.gpu 0 >/dev/null

echo "status=$overall" >> "$result"
ls -la "$out" > "$out/listing.txt"
[ "$overall" = PASS ]
