#!/usr/bin/env bash
#
# Everything a Mac with a real GPU and a logged-in desktop can close, in one
# unattended run — the macOS twin of `ci/hardware/linux.sh` and
# `ci/hardware/windows.ps1`, run by vieww's `hardware-macos` job (2 · Devices) or by hand.
#
#   ci/hardware/macos.sh                    # every step
#   ci/hardware/macos.sh baseline stress    # only some
#
# Steps, and what each proves (the hosted macOS leg runs on a paravirtual GPU
# in a VM; this is the same code on the Mac's own silicon):
#   env       the machine, GPU, power state and toolchain, recorded first
#   gpu       the Metal suites (vieww-hal) on this Mac's GPU, and a GPU frame
#             presented to a real window (wait_loop)
#   moltenvk  the Vulkan suites through MoltenVK, when a Vulkan loader and the
#             MoltenVK ICD are installed (the path package.sh --bundle-moltenvk
#             ships); SKIP otherwise
#   gallery   the CPU fixture gallery, 30 runs, with a receipt and the FW-08
#             perf-budget check for this machine's class (the GPU census is
#             Vulkan-only, so it runs under `moltenvk`)
#   baseline  a real window launched N times on the CPU path and N on the GPU
#             path (`examples/baseline.rs`), then FW-15's idle gate (zero
#             frames, idle CPU and wakeups) and the RSS budget
#   size      shipped (`--profile dist`) size of the windowed app, the dylibs
#             it loads (otool -L), and the framework-core size gate
#   stress    the animation stress suite five times: one reproducible p95
#   desktop   the on-desktop suite (`ci/certify/desktop-suite.sh`)
#   audio     vieww-audio on Core Audio: device lists, the live backend tests,
#             a soak on the default output; with a loopback device (BlackHole
#             wired as default output and input, or a real cable) the
#             playback-to-capture check; with a MIDI cable or IAC bus, a MIDI
#             round trip
#
# Environment:
#   VIEWW_HW_OUT=dir         results directory (default hardware-results/macos-<stamp>)
#   VIEWW_HW_LAUNCHES=5      baseline launches per path
#   VIEWW_HW_IDLE_SECS=60    the idle stretch the idle gate measures
#   VIEWW_HW_AUDIO_SOAK=600  seconds of the default-output soak
#   VIEWW_HW_AUDIO_GAIN=0    level played (0 = silent; 0.2 to hear it)
#   VIEWW_HW_AUDIO_LOOP=coreaudio  set when the default output is wired to the
#                            default input (BlackHole 2ch as both, or a cable)
#   VIEWW_MIDI_LOOP='<out id>><in id>'  a MIDI loop between two ports
#
# Needs: the pinned toolchain, Xcode command-line tools, python3. As a GitHub
# self-hosted runner it must run **inside the logged-in user's GUI session**
# (`./run.sh` in Terminal, or `./svc.sh install` which installs a LaunchAgent
# for that user — not a LaunchDaemon), or the window steps have no WindowServer
# to open on. macOS asks once for Screen Recording / Accessibility consent for
# the terminal that starts the runner; grant it before the first run.
#
# Every step runs even when an earlier one failed. Exit status is non-zero if
# any step failed; a step this machine cannot run is SKIP, never PASS.

set -uo pipefail
# macOS's /bin/bash is 3.2: an empty array expansion under `set -u` is an
# error there, so every optional array below expands as ${a[@]+"${a[@]}"}.

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root" || exit 1
stamp="$(date +%Y%m%d-%H%M%S)"
out="${VIEWW_HW_OUT:-$root/hardware-results/macos-$stamp}"
mkdir -p "$out"
summary="$out/summary.txt"
launches="${VIEWW_HW_LAUNCHES:-5}"
failed=0

steps=("$@")
[ ${#steps[@]} -eq 0 ] && steps=(env gpu moltenvk gallery baseline size stress desktop audio)
want() { local s; for s in "${steps[@]}"; do [ "$s" = "$1" ] && return 0; done; return 1; }

run() {
    local name="$1"; shift
    echo
    echo "==> $name: $*"
    if "$@" 2>&1 | tee "$out/$name.log"; then
        echo "PASS  $name" | tee -a "$summary"
    else
        echo "FAIL  $name (see $name.log)" | tee -a "$summary"
        failed=1
    fi
}
skip() { echo "SKIP  $1 ($2)" | tee -a "$summary"; }
note() { echo "      $*" | tee -a "$summary"; }

# BSD userland: no `date +%N`, no `timeout`. Nanoseconds from python3; a
# timeout from coreutils' gtimeout when Homebrew installed it, else perl's
# alarm (exec keeps the pending alarm, so the child is killed at the deadline).
now_ns() { python3 -c 'import time; print(time.time_ns())'; }
tmo() {
    local secs="$1"; shift
    if command -v gtimeout >/dev/null 2>&1; then gtimeout "$secs" "$@"
    else perl -e 'alarm shift; exec @ARGV or die "exec: $!"' "$secs" "$@"; fi
}
# A WindowServer session this process can draw into: the console user is us.
has_gui() { [ "$(stat -f %Su /dev/console 2>/dev/null)" = "$(id -un)" ]; }
has_vulkan() { command -v vulkaninfo >/dev/null 2>&1 && vulkaninfo --summary >/dev/null 2>&1; }

{
    echo "vieww macOS hardware run $stamp"
    echo "commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)$(git diff --quiet 2>/dev/null || echo ' (dirty)')"
    echo "host: $(hostname -s) · macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion)) · $(uname -m)"
    echo "cpu: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
    echo "rustc: $(rustc -V 2>/dev/null || echo 'NOT RESOLVED') (pinned: $(grep -m1 '^channel' rust-toolchain.toml | cut -d'"' -f2))"
    echo
} > "$summary"

if want env; then
    system_profiler SPDisplaysDataType > "$out/gpus.txt" 2>&1
    has_vulkan && vulkaninfo --summary > "$out/vulkaninfo.txt" 2>&1
    {
        grep -m2 -E 'Chipset Model|Metal' "$out/gpus.txt" | sed 's/^ */gpu: /'
        case "$(pmset -g batt 2>/dev/null | head -1)" in
            *"AC Power"*) echo "power: on mains" ;;
            *"Battery Power"*) echo "power: ON BATTERY — throttling and power saving will move every number" ;;
            *) echo "power: unknown" ;;
        esac
        echo "low power mode: $(pmset -g 2>/dev/null | awk '/lowpowermode/{print $2}' | head -1)"
        echo "gui session: $(has_gui && echo yes || echo 'NO — window steps will SKIP (start the runner from the logged-in user)')"
        echo "vulkan (MoltenVK): $(has_vulkan && echo present || echo absent)"
        echo "energy: not sampled on macOS (no unprivileged per-package counter) — energy will be null"
    } | tee -a "$summary"
    pinned="$(grep -m1 '^channel' rust-toolchain.toml | cut -d'"' -f2)"
    rustc -V 2>/dev/null | grep -q " $pinned " ||
        note "WARNING: rustc is not the pinned $pinned — numbers are for a different compiler"
fi

if want gpu; then
    run gpu-metal cargo test --release -p vieww-hal --features metal -- --ignored --test-threads=1
    if has_gui; then
        run gpu-window env VIEWW_REQUIRE_HARDWARE=display,gpu \
            cargo test --release -p vieww-platform-winit --test wait_loop
    else
        skip gpu-window "no GUI session for this user"
    fi
fi

if want moltenvk; then
    if has_vulkan; then
        run moltenvk-hal cargo test --release -p vieww-hal --features vulkan -- --ignored --test-threads=1
        run moltenvk-census cargo run --release -p fixtures -- "$out/census" --census \
            --runs 30 --receipt "$out/census/receipt.json" --shadow "$out/census/shadow.jsonl"
    else
        skip moltenvk "no Vulkan loader / MoltenVK ICD (brew install molten-vk vulkan-loader vulkan-tools)"
    fi
fi

if want gallery; then
    run gallery cargo run --release -p fixtures -- "$out/fixtures" \
        --runs 30 --receipt "$out/fixtures/receipt.json" --shadow "$out/fixtures/shadow.jsonl"
    if [ -f "$out/fixtures/receipt.json" ]; then
        receipts=("$out/fixtures/receipt.json")
        [ -f "$out/census/receipt.json" ] && receipts+=("$out/census/receipt.json")
        python3 -I ci/check/perf_baseline.py hardware-self-hosted-linux-and-windows hardware-macos "$out/perf-baseline.json" || true
        run perf-budget ci/check/perf-budget.sh --baseline "$out/perf-baseline.json" \
            --write-measured "$out/perf-measured.json" "${receipts[@]}"
    fi
fi

if want baseline; then
    if ! has_gui; then
        skip baseline "no GUI session — start the runner as the logged-in user"
    elif run baseline-build cargo build --release -p vieww-platform-winit --example baseline; then
        mkdir -p "$out/baseline"
        bin="$root/target/release/examples/baseline"
        note "baseline start-up is warm-cache (macOS has no unprivileged page-cache drop)"
        for mode in cpu gpu; do
            ok=1
            for i in $(seq 1 "$launches"); do
                flag=(); env_extra=()
                if [ "$mode" = gpu ]; then
                    flag=(--gpu)
                    env_extra=(VIEWW_PLANNER_SHADOW="$out/baseline/gpu-shadow.jsonl")
                fi
                if ! env ${env_extra[@]+"${env_extra[@]}"} VIEWW_BASELINE_LAUNCH_UNIX_NS="$(now_ns)" \
                    tmo 120 "$bin" ${flag[@]+"${flag[@]}"} --out "$out/baseline/$mode-$i.json" \
                    >> "$out/baseline-$mode.log" 2>&1; then
                    ok=0
                fi
            done
            if [ "$ok" = 1 ]; then echo "PASS  baseline-$mode ($launches launches)" | tee -a "$summary"
            else echo "FAIL  baseline-$mode (see baseline-$mode.log)" | tee -a "$summary"; failed=1; fi
        done
        python3 "$root/ci/hardware/summarize_baseline.py" "$out/baseline" |
            tee "$out/baseline/summary.txt" | sed 's/^/      /' | tee -a "$summary"

        # FW-15's idle gate and RSS budget, the same thresholds as linux.sh.
        idle_secs="${VIEWW_HW_IDLE_SECS:-60}"
        wake="$out/baseline/wakeup.json"
        if env VIEWW_BASELINE_LAUNCH_UNIX_NS="$(now_ns)" tmo $((idle_secs + 120)) "$bin" \
            --settle 1 --idle "$idle_secs" --animate 1 --out "$wake" \
            >> "$out/baseline-wakeup.log" 2>&1; then
            run idle-gate python3 -I - "$wake" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
frames, wakeups, cpu = r.get("idle_frames"), r.get("idle_wakeups_per_s"), r.get("idle_cpu_percent")
failed = []
if frames is None:
    print("idle-gate: no idle observations in the report — cannot gate"); sys.exit(1)
if frames != 0:
    failed.append(f"idle presented {frames} frame(s): the idle gate measures an app that presents nothing")
if wakeups is None:
    print("idle-gate: this OS does not report wakeups — not gated on them")
elif wakeups > 5.0:
    failed.append(f"idle wakeups {wakeups:.1f}/s at zero frames (budget 5) — a poller, not input")
if cpu is None:
    print("idle-gate: this OS does not report process CPU — not gated on it")
elif cpu > 0.5:
    failed.append(f"idle CPU {cpu:.2f}% at zero frames (budget 0.5%)")
for line in failed:
    print("idle-gate: FAIL " + line)
sys.exit(1 if failed else 0)
PY
        else
            echo "FAIL  idle-gate (the wakeup launch itself failed; see baseline-wakeup.log)" | tee -a "$summary"
            failed=1
        fi
        # No hardware-macos row yet: perf-budget reports a class without a
        # budget as "new", which is the honest first run; record one with
        # `ci/check/perf-budget.sh --record --class hardware-macos` from a
        # receipt this script wrote, as a reviewed commit.
        [ -f "$wake" ] && run rss-budget ci/check/perf-budget.sh --class hardware-macos "$wake"
    fi
fi

if want size; then
    run size-build cargo build --profile dist -p vieww-platform-winit --example baseline --target-dir target/size
    app="$root/target/size/dist/examples/baseline"
    if [ -f "$app" ]; then
        bytes="$(stat -f %z "$app")"
        echo "$bytes" > "$out/size-baseline-bytes.txt"
        otool -L "$app" > "$out/size-baseline-otool.txt" 2>&1
        note "windowed baseline app, dist profile: $bytes bytes"
        note "dynamic libraries it loads: $(($(wc -l < "$out/size-baseline-otool.txt") - 1)) (see size-baseline-otool.txt)"
    fi
    run size-budget ci/check/size-budget.sh
fi

if want stress; then
    p95s=()
    for i in 1 2 3 4 5; do
        run "stress-$i" cargo run --release -p test-animation-stress -- "$out/stress-$i"
        v="$(grep -m1 '^render_p95_ms=' "$out/stress-$i/metrics.txt" 2>/dev/null | cut -d= -f2)"
        [ -n "$v" ] && p95s+=("$v")
    done
    [ ${#p95s[@]} -gt 0 ] &&
        note "animation stress p95 over ${#p95s[@]} runs (ms): $(printf '%s\n' "${p95s[@]}" | sort -n | tr '\n' ' ')"
fi

if want desktop; then
    if has_gui; then
        run desktop ci/certify/desktop-suite.sh --log "$out/desktop-app.log"
    else
        skip desktop "no GUI session for this user"
    fi
fi

if want audio; then
    system_profiler SPAudioDataType > "$out/audio-devices.txt" 2>&1
    note "audio devices: $(grep -E '^        [A-Za-z].*:$' "$out/audio-devices.txt" | sed 's/^ *//; s/:$//' | tr '\n' ';')"
    run audio-devices cargo test --release -p vieww-audio --lib backend:: -- --nocapture
    run audio-live env VIEWW_LIVE_GAIN="${VIEWW_HW_AUDIO_GAIN:-0}" VIEWW_SOAK_SECS=10 \
        cargo test --release -p vieww-audio --test live_backends -- --ignored --nocapture --test-threads=1
    run audio-player env VIEWW_LIVE_GAIN="${VIEWW_HW_AUDIO_GAIN:-0}" \
        cargo test --release -p vieww-audio --test player_smoke -- --ignored --nocapture
    run audio-soak env VIEWW_REQUIRE_LOOPBACK=1 VIEWW_LIVE_GAIN="${VIEWW_HW_AUDIO_GAIN:-0}" \
        VIEWW_SOAK_SECS="${VIEWW_HW_AUDIO_SOAK:-600}" \
        cargo test --release -p vieww-audio --test device_loopback default_output_soak -- --ignored --nocapture
    if [ -n "${VIEWW_HW_AUDIO_LOOP:-}" ]; then
        run audio-loopback env VIEWW_REQUIRE_LOOPBACK=1 VIEWW_LOOP_BACKEND="$VIEWW_HW_AUDIO_LOOP" \
            cargo test --release -p vieww-audio --test device_loopback default_device -- --ignored --nocapture
    else
        skip audio-loopback "VIEWW_HW_AUDIO_LOOP not set (BlackHole 2ch as default output and input, then =coreaudio)"
    fi
    if [ -n "${VIEWW_MIDI_LOOP:-}" ]; then
        run audio-midi env VIEWW_REQUIRE_LOOPBACK=1 \
            cargo test --release -p vieww-audio --test device_loopback midi_port -- --ignored --nocapture
    else
        skip audio-midi "VIEWW_MIDI_LOOP not set (a MIDI cable, or an IAC bus, between two ports)"
    fi
fi

echo
echo "================ summary ================"
cat "$summary"
echo
echo "results: $out"
exit "$failed"
