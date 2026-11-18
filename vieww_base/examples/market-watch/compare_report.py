#!/usr/bin/env python3
"""Valdi vs Vieww: turn a comparison run's raw output into one report.

    compare_report.py --farm DIR [--machines DIR] --out DIR
    compare_report.py --self-test

Input: ci/mobile/device_farm.py's output: one folder per phone, holding the
job's artifacts with mw/<config>/{events,runner}.jsonl, timestats-<phase>.txt,
gfxinfo-<phase>.txt and mw/result.txt, as run-android.sh writes them, plus
valdi-build.txt at the top (how the Valdi APK was built).

--machines, for the wide comparison: a folder holding one verdict.json per
machine (ci/machine_verdict.py, from the compare-machines legs) — the CPU
path's headless raster on Linux, Windows and macOS, x64 and arm64, and
whether any pixel moved against main on that machine. A table of its own:
those are raster times on a desktop CPU, not frames presented on a phone.

Writes OUT/COMPARE.md (for the job summary and the results branch) and
OUT/compare.json (every number, for anyone who wants to chart it).

What is compared, and with what yardstick: both frameworks as they ship on
phones. SurfaceFlinger's per-layer timestats are the yardstick: frames
  actually presented and the gap between them, measured by the compositor
  the same way for Vieww's NativeActivity surface and Valdi's HWUI views. A
  phase's frame rate is its presented frames over its length; jank is the
  share of gaps over 1.5 display frames. Each app's own numbers (Vieww's frame
  work and, on new runs, the display waits `wait_ms` beside it — the other
  half of a frame's wall clock, optional, absent on old runs; Valdi's JS
  render cost) and HWUI's gfxinfo (Valdi only) say *where*
  the time went. CPU is the process's utime + stime over the phase.

Standard library only.
"""
import argparse
import json
import pathlib
import re
import statistics
import sys

CONTINUOUS = ["scroll2k", "live2k", "storm2k", "scroll20k", "live20k", "storm20k", "thermal"]
EVENTS = ["load2k", "tick2k", "jump2k", "load20k", "tick20k", "jump20k"]
IDLE = ["idle2k", "idle20k"]
CONFIGS = ["vieww-cpu", "vieww-gpu", "valdi"]
# The diagnostic leg: parsed and reported in its own A/B block beside the
# default CPU leg, but kept out of the headline tables, which stay the
# three-way comparison the legend describes. `vieww-cpuserial` is the same
# APK with the raster walk pinned serial (`debug.mw.threads=0`) — the
# same-unit answer to what the row-parallel pool bought on this phone.
AB_LEG = "vieww-cpuserial"
PARSED_CONFIGS = CONFIGS + [AB_LEG]
CLK_TCK = 100  # Android's USER_HZ


# ───────────────────────────────────────────────────────────────── parsing

def jsonl(path):
    rows = []
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            line = line.strip()
            if line.startswith("{"):
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return rows


def kv_file(path):
    out = {}
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                out[k.strip()] = v.strip()
    return out


def hist_percentile(hist, p):
    """`hist` maps a gap in ms to a count; the gap at percentile p."""
    total = sum(hist.values())
    if not total:
        return None
    need, seen = p * total, 0
    for ms in sorted(hist):
        seen += hist[ms]
        if seen >= need:
            return ms
    return max(hist)


def parse_timestats(text, package):
    """The app's layer from `dumpsys SurfaceFlinger --timestats -dump`.

    Sums every layer of the package (an app can own more than one: a surface
    view, a recreated window), keeping the present-to-present histogram.
    """
    blocks = re.split(r"\n(?=layerName = )", text)
    total = dropped = 0
    hist = {}
    found = False
    for block in blocks:
        name = re.search(r"layerName = (.*)", block)
        if not name or package not in name.group(1):
            continue
        found = True
        m = re.search(r"totalFrames = (\d+)", block)
        total += int(m.group(1)) if m else 0
        m = re.search(r"droppedFrames = (\d+)", block)
        dropped += int(m.group(1)) if m else 0
        h = re.search(r"present2present histogram is as below:\s*\n(.*)", block)
        if h:
            for ms, count in re.findall(r"(\d+)ms=(\d+)", h.group(1)):
                hist[int(ms)] = hist.get(int(ms), 0) + int(count)
    if not found:
        return None
    return {"frames": total, "dropped": dropped, "hist": hist}


def parse_gfxinfo(text):
    if "Total frames rendered" not in text:
        return None
    g = {}
    m = re.search(r"Total frames rendered: (\d+)", text)
    g["frames"] = int(m.group(1)) if m else 0
    m = re.search(r"Janky frames: (\d+) \(([\d.]+)%\)", text)
    if m:
        g["janky"], g["janky_pct"] = int(m.group(1)), float(m.group(2))
    for p in ("50", "90", "95", "99"):
        m = re.search(rf"{p}th percentile: (\d+)ms", text)
        if m:
            g[f"p{p}_ms"] = int(m.group(1))
    return g


def phase_stats(config_dir, package, budget_ms):
    """Every number for every phase of one configuration on one phone."""
    events = jsonl(config_dir / "events.jsonl")
    runner = {r["phase"]: r for r in jsonl(config_dir / "runner.jsonl")}
    app = {e["phase"]: e for e in events if e.get("ev") == "stats"}
    # The stage line the Vieww app emits per phase: where a frame's time
    # went, as `[p50, p90, max]` per stage — and, on a profiled run, the
    # `kinds` field splitting the raster stage by command kind. One line per
    # phase (the last wins), so a rerun's leftover line cannot double-count.
    stages = {e["phase"]: e for e in events if e.get("ev") == "stages"}
    phases = {}
    for name in sorted(set(runner) | set(app)):
        r, a = runner.get(name, {}), app.get(name, {})
        p = {"app": a}
        st = stages.get(name)
        if st:
            p["stages"] = {
                "raster_p50": (st.get("raster_ms") or [None])[0],
                "present_p50": (st.get("present_ms") or [None])[0],
                "acquire_p50": (st.get("acquire_ms") or [None])[0],
                "kinds": st.get("kinds"),
            }
        wall = r.get("wall_ms") or 0
        if wall:
            p["wall_ms"] = wall
            ticks = r.get("cpu_ticks")
            # A negative delta means the pid changed under the phase (a crash
            # and restart): no figure rather than a wrong one.
            if isinstance(ticks, (int, float)) and ticks >= 0:
                p["cpu_pct"] = round(100.0 * ticks * (1000 / CLK_TCK) / wall, 1)
        p["rss_kb"] = r.get("rss_kb")
        # Battery power over the phase (run-android.sh's power_sample), and
        # energy per presented frame — only where the phase ran on battery:
        # on USB the battery current is the charger's, not the app's.
        if r.get("batt_temp_dC"):
            p["batt_temp_c"] = round(r["batt_temp_dC"] / 10.0, 1)
        if r.get("discharging") and r.get("power_mw") and wall:
            p["power_mw"] = r["power_mw"]
            frames = a.get("frames") or 0
            if frames:
                p["energy_mj_per_frame"] = round(r["power_mw"] * (wall / 1000.0) / frames, 2)
        ts_file = config_dir / f"timestats-{name}.txt"
        ts = parse_timestats(ts_file.read_text(errors="replace"), package) if ts_file.exists() else None
        if ts:
            gaps = ts["hist"]
            p["sf_frames"] = ts["frames"]
            if wall:
                p["sf_fps"] = round(ts["frames"] / (wall / 1000.0), 1)
            p["sf_gap_p50"] = hist_percentile(gaps, 0.50)
            p["sf_gap_p95"] = hist_percentile(gaps, 0.95)
            p["sf_gap_p99"] = hist_percentile(gaps, 0.99)
            n = sum(gaps.values())
            if n and budget_ms:
                p["sf_jank_pct"] = round(100.0 * sum(c for ms, c in gaps.items() if ms > 1.5 * budget_ms) / n, 1)
        # No compositor counts (Firebase Test Lab has no timestats): the
        # app's own presented frames over the phase's wall time, and its own
        # count of frames that came a vsync late.
        if "sf_fps" not in p and wall and a.get("frames"):
            p["app_fps"] = round(a["frames"] / (wall / 1000.0), 1)
            p["app_jank_pct"] = round(100.0 * (a.get("jank") or 0) / a["frames"], 1)
        gx_file = config_dir / f"gfxinfo-{name}.txt"
        gx = parse_gfxinfo(gx_file.read_text(errors="replace")) if gx_file.exists() else None
        if gx:
            p["hwui"] = gx
        phases[name] = p
    return phases


# Device-log lines about the window's frame rate (`crate::cadence`,
# `crate::perf_hint`, the refresh-rate report in `crate::app`).
RATE_LOG_KEYS = ("Hz at start", "frame rate ", "performance hint", "reports ", "Hz;")


def parse_farm(root):
    phones = []
    if not root or not root.exists():
        return phones
    for result in sorted(root.rglob("mw/result.txt")):
        mw = result.parent
        info = kv_file(result)
        hz = float(info.get("refresh_hz") or 60) or 60.0
        budget = 1000.0 / hz
        phone = {"device": info.get("device_name", mw.parent.name), "info": info, "budget_ms": round(budget, 2),
                 "configs": {}}
        for config in PARSED_CONFIGS:
            d = mw / config
            if not d.exists():
                continue
            package = "com.snap.valdi.marketwatch" if config == "valdi" else "dev.vieww.marketwatch"
            starts = [float(x) for x in info.get(f"{config}_cold_start_ms", "").split() if x.replace(".", "").isdigit()]
            stats = {
                "status": info.get(f"{config}_status", "?"),
                "cold_start_ms": starts,
                "cold_start_median": statistics.median(starts) if starts else None,
                "pss_kb": info.get(f"{config}_pss_kb"),
                "battery_temp_dC": info.get(f"{config}_battery_temp_dC"),
                "phases": phase_stats(d, package, budget),
            }
            # The run's own statement of its raster policy (the `quality`
            # event): which thread request and cache budgets the frames ran
            # with, straight from the phone.
            quality = next((e.get("line") for e in jsonl(d / "events.jsonl") if e.get("ev") == "quality"), None)
            if quality:
                stats["quality"] = quality
            # What the window asked Android for and what its cadence decided:
            # the lines a frame-rate fix lives or dies by, from the device log.
            log = d / "logcat.txt"
            if log.is_file():
                rate = [line.split("vieww: ", 1)[1].strip()
                        for line in log.read_text(errors="replace").splitlines()
                        if "vieww: " in line and any(k in line for k in RATE_LOG_KEYS)]
                if rate:
                    stats["rate_log"] = rate[:12]
            phone["configs"][config] = stats
        phones.append(phone)
    return phones


# ───────────────────────────────────────────────────────────────── report

def fmt(v, unit="", nd=1):
    if v is None or v == "":
        return "–"
    if isinstance(v, (int, float)):
        return f"{v:.{nd}f}{unit}"
    return f"{v}{unit}"


def winner(values, higher_better):
    """The config with the best value, if it is clearly best (5%)."""
    vals = {k: v for k, v in values.items() if isinstance(v, (int, float))}
    if len(vals) < 2:
        return None
    best = max(vals, key=vals.get) if higher_better else min(vals, key=vals.get)
    others = [v for k, v in vals.items() if k != best]
    ref = max(others) if higher_better else min(others)
    if ref and abs(vals[best] - ref) / abs(ref) < 0.05:
        return None
    return best


def cell(values, key, higher_better, unit="", nd=1):
    w = winner(values, higher_better)
    return " / ".join(
        (f"**{fmt(values.get(c), unit, nd)}**" if c == w else fmt(values.get(c), unit, nd)) for c in CONFIGS
    )


def render(farm, meta):
    md = ["# Valdi vs Vieww — Market Watch", ""]
    md.append(meta.get("header", ""))
    md.append("")
    md.append(
        "Same screen in both frameworks (identical data generator, tick rule and schedule; "
        "`cargo test -p market-watch` checks the data matches Valdi's). "
        "Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold."
    )
    md.append("")
    build = meta.get("valdi_build") or {}
    if build:
        md.append(f"Valdi built as it ships: `{build.get('flags', '?').strip()}` at `{build.get('valdi_ref', '?')[:12]}` "
                  f"— {build.get('abi', '?')}, JS engine {build.get('engine', '?')}, "
                  f"debuggable {build.get('debuggable', '?')} (the flags Valdi's CLI uses for "
                  "`valdi package android --build_config release`).")
        md.append("")

    # ── phones
    md.append("## Phones (AWS Device Farm)")
    md.append("")
    if not farm:
        md.append("_No phone results: the farm leg did not run or produced nothing. See the run's artifacts._")
        md.append("")
    for ph in farm:
        info, cfg = ph["info"], ph["configs"]
        md.append(f"### {ph['device']} · Android {info.get('android', '?')} · {info.get('soc', '?')} · "
                  f"{fmt(float(info['mem_total_kb']) / 2**20 if info.get('mem_total_kb') else None, ' GB')} · "
                  f"{info.get('refresh_hz') or '?'} Hz")
        md.append("")
        # Statuses come from result.txt, which names a configuration even when
        # it never started (an app that did not install is "SKIPPED").
        statuses = ", ".join(f"{c}: {info[f'{c}_status']}" for c in PARSED_CONFIGS if info.get(f"{c}_status"))
        md.append(f"Runs — {statuses or 'none started'}")
        for c in ("vieww-cpu", "vieww-gpu"):
            lines = cfg.get(c, {}).get("rate_log")
            if lines:
                md.append(f"- {c} frame rate: " + " · ".join(f"`{x}`" for x in lines))
        if info.get("mode") == "firebase":
            md.append("")
            md.append("_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from "
                      "the app's own frame log, as Test Lab has no compositor counts._")
        if info.get("mode") == "quick":
            md.append("")
            md.append("_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, "
                      "so its numbers stand from the last full report._")
        # Energy, where any phase ran on battery (an unplugged phone on
        # wireless adb): average power and energy per presented frame.
        powered = [(name, {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in CONFIGS})
                   for name in CONTINUOUS]
        powered = [(n, p) for n, p in powered if any(q.get("power_mw") for q in p.values())]
        if powered:
            md.append("")
            md.append("| phase (on battery) | power (mW) | energy per frame (mJ) | battery °C |")
            md.append("|---|---|---|---|")
            for name, p in powered:
                pw = {c: p[c].get("power_mw") for c in CONFIGS}
                ej = {c: p[c].get("energy_mj_per_frame") for c in CONFIGS}
                tc = {c: p[c].get("batt_temp_c") for c in CONFIGS}
                md.append(f"| {name} | {cell(pw, 'pw', False, '', 0)} | {cell(ej, 'ej', False, '', 2)} | "
                          f"{cell(tc, 'tc', False, '', 1)} |")
        # Each Vieww leg's own statement of its raster policy, when it said
        # one: the thread request the frames actually ran with.
        for c in ["vieww-cpu", AB_LEG]:
            line = cfg.get(c, {}).get("quality")
            if line:
                md.append(f"- {c}: `{line}`")
        for pkg, label in (("com.snap.valdi.marketwatch", "Valdi"), ("dev.vieww.marketwatch", "Vieww")):
            if info.get(f"missing_{pkg}"):
                why = info.get("install_error_valdi") if label == "Valdi" else None
                md.append("")
                md.append(f"> **{label} app not installed on this phone**"
                          + (f": `{why}`" if why else " (reason in the run artifact's install.log)"))
        if info.get("valdi_apk_on_host"):
            md.append("")
            md.append(f"> Valdi APK not found on the Device Farm host: `{info['valdi_apk_on_host']}`")
        md.append("")
        md.append("| | Vieww CPU / Vieww GPU / Valdi |")
        md.append("|---|---|")
        g = lambda key: {c: cfg.get(c, {}).get(key) for c in CONFIGS}  # noqa: E731
        md.append(f"| cold start, median of 5 (ms) | {cell(g('cold_start_median'), 'cs', False, '', 0)} |")
        pss = {c: (float(cfg[c]['pss_kb']) / 1024 if cfg.get(c, {}).get('pss_kb') not in (None, '') else None)
               for c in CONFIGS}
        md.append(f"| memory after the run, PSS (MB) | {cell(pss, 'pss', False)} |")
        apk = {"vieww-cpu": info.get("apk_file_bytes_vieww"), "vieww-gpu": info.get("apk_file_bytes_vieww"),
               "valdi": info.get("apk_file_bytes_valdi")}
        apk = {k: (int(v) / 2**20 if v else None) for k, v in apk.items()}
        md.append(f"| APK (MB) | {cell(apk, 'apk', False)} |")
        md.append("")
        md.append("| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |")
        md.append("|---|---|---|---|---|---|---|")
        for name in CONTINUOUS:
            p = {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in CONFIGS}
            if not any(p.values()):
                continue
            fps = {c: p[c].get("sf_fps") or p[c].get("app_fps") for c in CONFIGS}
            gap = {c: p[c].get("sf_gap_p95") for c in CONFIGS}
            jank = {c: p[c].get("sf_jank_pct", p[c].get("app_jank_pct")) for c in CONFIGS}
            work = {c: (p[c].get("app", {}).get("work_ms") or p[c].get("app", {}).get("render_ms") or [None, None])[1]
                    for c in CONFIGS}
            # `wait_ms` is the other half of a frame's wall clock, what `work`
            # now excludes: the display waits (image acquire, fence). It is
            # optional — a line without it is an old run's, and wait stays
            # absent there, never a zero folded into work.
            wait = {c: (p[c].get("app", {}).get("wait_ms") or [None, None])[1] for c in CONFIGS}
            cpu = {c: p[c].get("cpu_pct") for c in CONFIGS}
            md.append(f"| {name} | {cell(fps, 'fps', True)} | {cell(gap, 'gap', False, '', 0)} | "
                      f"{cell(jank, 'jank', False)} | {cell(work, 'work', False, '', 2)} | "
                      f"{cell(wait, 'wait', False, '', 2)} | {cell(cpu, 'cpu', False)} |")
        md.append("")

        # The same-unit A/B the fourth leg exists for: the default CPU path
        # against the same path with the raster walk pinned serial, both
        # profiled, one phone, one run. Rendered only when the leg ran.
        if AB_LEG in cfg:
            md.append("<details open><summary>Vieww CPU · parallel vs serial raster "
                      "(same phone, same run)</summary>")
            md.append("")
            md.append("Cells read parallel / serial; the raster kinds are the p50 of the "
                      "raster stage split by command kind (a profiled run's `kinds` field).")
            md.append("")
            md.append("| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | wait p95 (ms) | jank % |")
            md.append("|---|---|---|---|---|---|")
            legs = ["vieww-cpu", AB_LEG]
            for name in CONTINUOUS:
                p = {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in legs}
                if not any(p.values()):
                    continue

                def pair(key, nd=2):
                    vals = [p[c].get("stages", {}).get(key) for c in legs]
                    return " / ".join(fmt(v, "", nd) for v in vals)

                work = {c: (p[c].get("app", {}).get("work_ms") or [None, None])[1] for c in legs}
                wait = {c: (p[c].get("app", {}).get("wait_ms") or [None, None])[1] for c in legs}
                jank = {c: p[c].get("sf_jank_pct") for c in legs}
                md.append(f"| {name} | {pair('raster_p50')} | {pair('present_p50')} | "
                          f"{fmt(work['vieww-cpu'], '', 2)} / {fmt(work[AB_LEG], '', 2)} | "
                          f"{fmt(wait['vieww-cpu'], '', 2)} / {fmt(wait[AB_LEG], '', 2)} | "
                          f"{fmt(jank['vieww-cpu'])} / {fmt(jank[AB_LEG])} |")
            kinds_rows = [(name, {c: cfg.get(c, {}).get("phases", {}).get(name, {}).get("stages", {}).get("kinds")
                                  for c in legs})
                          for name in CONTINUOUS]
            if any(any(k.values()) for _, k in kinds_rows):
                md.append("")
                md.append("| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |")
                md.append("|---|---|")
                for name, k in kinds_rows:
                    if not any(k.values()):
                        continue

                    def leg_kinds(one):
                        if not one:
                            return "–"
                        return " / ".join(fmt((one.get(kind) or [None])[0], "", 2)
                                           for kind in ("text", "paths", "shadows", "layers", "composite"))

                    md.append(f"| {name} | {leg_kinds(k['vieww-cpu'])} \\n {leg_kinds(k[AB_LEG])} |")
            md.append("")
            md.append("</details>")
            md.append("")
        md.append("")
        md.append("| event | app time (ms) | app wait (ms) | frames presented | CPU % |")
        md.append("|---|---|---|---|---|")
        for name in EVENTS + IDLE:
            p = {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in CONFIGS}
            if not any(p.values()):
                continue
            t = {c: (p[c].get("app", {}).get("work_ms") or p[c].get("app", {}).get("render_ms") or [None])[0]
                 for c in CONFIGS}
            wait = {c: (p[c].get("app", {}).get("wait_ms") or [None])[0] for c in CONFIGS}
            fr = {c: p[c].get("sf_frames") for c in CONFIGS}
            cpu = {c: p[c].get("cpu_pct") for c in CONFIGS}
            md.append(f"| {name} | {cell(t, 't', False, '', 2)} | {cell(wait, 'wait', False, '', 2)} | "
                      f"{cell(fr, 'fr', name not in IDLE, '', 0)} | "
                      f"{cell(cpu, 'cpu', False)} |")
        md.append("")
        hw = cfg.get("valdi", {}).get("phases", {})
        hw_rows = [(n, hw[n]["hwui"]) for n in CONTINUOUS if n in hw and hw[n].get("hwui")]
        if hw_rows:
            md.append("<details><summary>Valdi · HWUI's own frame numbers (gfxinfo)</summary>")
            md.append("")
            md.append("| phase | frames | janky % | p50 / p95 / p99 (ms) |")
            md.append("|---|---|---|---|")
            for n, gx in hw_rows:
                md.append(f"| {n} | {gx.get('frames')} | {fmt(gx.get('janky_pct'))} | "
                          f"{gx.get('p50_ms', '–')} / {gx.get('p95_ms', '–')} / {gx.get('p99_ms', '–')} |")
            md.append("")
            md.append("</details>")
            md.append("")

    if len(farm) > 1:
        md.append("### Across phones · median presented fps")
        md.append("")
        md.append("| phase | " + " | ".join(ph["device"] for ph in farm) + " |")
        md.append("|---|" + "---|" * len(farm))
        for name in CONTINUOUS:
            cells = []
            for ph in farm:
                v = {c: (lambda q: q.get("sf_fps") or q.get("app_fps"))(ph["configs"].get(c, {}).get("phases", {}).get(name, {})) for c in CONFIGS}
                cells.append(cell(v, "fps", True))
            md.append(f"| {name} | " + " | ".join(cells) + " |")
        md.append("")

    md.append("## Reading this")
    md.append("")
    md.append(
        "- **Presented fps** is what a user sees. A phase that changes something every frame can show at most "
        "the panel's rate.\n"
        "- **Idle** phases should present nothing; any frames there are wasted battery.\n"
        "- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU "
        "rasterizer or its GPU path.\n"
        "- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads "
        "its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers "
        "being unobservable in a build that ships; the compositor and HWUI numbers cover it.\n"
        "- **App wait** is the other half of a frame's wall clock beside app work: what the display made the "
        "frame wait (the image acquire and the flight-slot fence). Runs from before the field was logged show "
        "a dash there — and their work counts the waits in, so it runs high beside a new run's work + wait.\n"
        "- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on "
        "`before_frame`: one change per drawn frame on both sides.\n"
        "- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a "
        "minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live "
        "load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one "
        "core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought."
    )
    return "\n".join(md) + "\n"


def parse_machines(root):
    """Every machine leg's verdict.json under ROOT, in a stable order."""
    if not root or not root.exists():
        return []
    out = []
    for path in sorted(root.rglob("verdict.json")):
        try:
            out.append(json.loads(path.read_text()))
        except (OSError, ValueError):
            out.append({"leg": path.parent.name, "status": "unreadable verdict.json", "walks": {}, "machine": {}})
    return out


def render_machines(machines):
    """The machines table: per machine and walk, raster p50 per phase and the
    pixel verdict against main."""
    if not machines:
        return ""
    phases = ("load", "scroll", "live", "storm")
    lines = [
        "## Machines · the CPU path, headless",
        "",
        "Retained raster per frame at the phone's 1081x2340 @ 2.75 (ms, p50), on each machine's",
        "CPU with no window: a measure of the rasterizer, not frames presented. `serial` is the",
        "phones' walk; `default` uses the machine's cores. Pixels are compared with main on the",
        "same machine only.",
        "",
        "| machine | OS / CPU | cores | walk | " + " | ".join(phases) + " | pixels vs main | verdict |",
        "|---|---|---|---|" + "---|" * len(phases) + "---|---|",
    ]
    for m in machines:
        info = m.get("machine") or {}
        where = f"{info.get('os', '?')} / {info.get('arch', '?')}"
        walks = m.get("walks") or {"–": {}}
        for walk, rows in walks.items():
            cells = [f"{rows[p]['median_ms']:.2f}" if p in rows else "–" for p in phases]
            pix = sorted({r.get("pixels", "?") for r in rows.values()}) or ["–"]
            lines.append(f"| {m.get('leg', '?')} | {where} | {info.get('cores', '?')} | {walk} | "
                         + " | ".join(cells) + f" | {', '.join(pix)} | {m.get('status', '?')} |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--farm", type=pathlib.Path)
    ap.add_argument("--machines", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path("."))
    ap.add_argument("--header", default="")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args(argv)
    if a.self_test:
        return self_test()
    farm = parse_farm(a.farm)
    # At the top of the farm's folder, or one level down when the report
    # downloaded several artifacts (the farm's and the USB phone's).
    found = next(iter(sorted(a.farm.rglob("valdi-build.txt"))), None) if a.farm and a.farm.exists() else None
    build = kv_file(found) if found else {}
    machines = parse_machines(a.machines)
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "compare.json").write_text(
        json.dumps({"valdi_build": build, "phones": farm, "machines": machines}, indent=1))
    md = render(farm, {"header": a.header, "valdi_build": build}) if farm or not machines else (
        f"# Valdi vs Vieww\n\n{a.header}\n\nNo phones in this run.\n" if a.header else "")
    if machines:
        md = md.rstrip("\n") + "\n\n" + render_machines(machines)
    (a.out / "COMPARE.md").write_text(md)
    print(md)
    return 0


# ───────────────────────────────────────────────────────────────── self-test

TIMESTATS = """SurfaceFlinger TimeStats:
statsStart = 1
layerName = dev.vieww.marketwatch/android.app.NativeActivity#0
packageName = dev.vieww.marketwatch
totalFrames = 470
droppedFrames = 2
present2present histogram is as below:
16ms=440 17ms=20 33ms=10
layerName = com.other/Main#0
totalFrames = 999
present2present histogram is as below:
16ms=999
"""
GFX = """Total frames rendered: 478
Janky frames: 12 (2.51%)
50th percentile: 9ms
90th percentile: 13ms
95th percentile: 15ms
99th percentile: 24ms
"""


def self_test():
    import tempfile
    tmp = pathlib.Path(tempfile.mkdtemp())
    mw = tmp / "farm" / "Pixel_6-14" / "Customer_Artifacts" / "mw"
    for c, pkg in (("vieww-cpu", "dev.vieww.marketwatch"), ("vieww-cpuserial", "dev.vieww.marketwatch"),
                   ("valdi", "com.snap.valdi.marketwatch")):
        d = mw / c
        d.mkdir(parents=True)
        if c != "valdi":
            # A Vieww leg narrates: the stats line the tables read, the stages
            # line the A/B block reads (with `kinds`, as a profiled run emits)
            # and the quality line that says which raster policy it took. The
            # parallel leg's live2k line carries the new `wait_ms` half; the
            # serial leg's is an old run's, without it — as is both legs'
            # load2k line, an event phase's old-format stats.
            stats = {"fw": c, "ev": "stats", "phase": "live2k", "frames": 470,
                     "work_ms": [5.0, 9.0, 12.0, 20.0],
                     "interval_ms": [16.6, 17.0, 33.0, 40.0]}
            if c == "vieww-cpu":
                stats["wait_ms"] = [1.0, 2.0, 3.0, 4.0]
            (d / "events.jsonl").write_text(
                json.dumps(stats) + "\n" +
                json.dumps({"fw": c, "ev": "stats", "phase": "load2k", "frames": 30,
                            "work_ms": [4.0, 5.0, 6.0, 7.0]}) + "\n" +
                json.dumps({"fw": c, "ev": "stages", "phase": "live2k", "frames": 470,
                            "raster_ms": [4.0, 6.0, 8.0], "present_ms": [2.0, 3.0, 4.0],
                            "acquire_ms": [1.0, 1.0, 1.0],
                            "kinds": {"text": [1.5, 2.0, 3.0], "paths": [1.0, 1.0, 1.0],
                                      "shadows": [0.5, 0.5, 0.5], "layers": [0.0, 0.0, 0.0],
                                      "composite": [0.5, 0.5, 0.5]}}) + "\n" +
                json.dumps({"fw": c, "ev": "quality",
                            "line": f"blur off (tier medium), fill off (tier medium), raster threads {c}"}) + "\n")
        else:
            (d / "events.jsonl").write_text(
                json.dumps({"fw": c, "ev": "stats", "phase": "live2k", "frames": 470,
                            "render_ms": [5.0, 9.0, 12.0, 20.0],
                            "interval_ms": [16.6, 17.0, 33.0, 40.0]}) + "\n")
        (d / "runner.jsonl").write_text(json.dumps({"phase": "live2k", "wall_ms": 8000, "cpu_ticks": 400,
                                                    "rss_kb": 90000, "layer": "x"}) + "\n")
        (d / "timestats-live2k.txt").write_text(TIMESTATS.replace("dev.vieww.marketwatch", pkg))
        if c == "valdi":
            (d / "gfxinfo-live2k.txt").write_text(GFX)
    (mw / "result.txt").write_text(
        "device_name=Pixel 6\nandroid=14\nsoc=Tensor\nmem_total_kb=8000000\nrefresh_hz=60\n"
        "vieww-cpu_cold_start_ms=300 320 310 305 315\nvaldi_cold_start_ms=420 400 410 430 415\n"
        "vieww-cpu_status=PASS\nvieww-cpuserial_status=PASS\nvaldi_status=PASS\n"
        "vieww-cpu_pss_kb=80000\nvaldi_pss_kb=120000\n"
        "apk_file_bytes_vieww=5000000\napk_file_bytes_valdi=12000000\nstatus=PASS\n")
    (tmp / "farm" / "valdi-build.txt").write_text(
        "valdi_ref=e90a983749b3\nflags=--snap_flavor=production -c opt\nabi=arm64-v8a\nengine=quickjs\ndebuggable=false\n")
    farm = parse_farm(tmp / "farm")
    build = kv_file(tmp / "farm" / "valdi-build.txt")
    assert len(farm) == 1, farm
    live = farm[0]["configs"]["vieww-cpu"]["phases"]["live2k"]
    assert live["sf_frames"] == 470, live  # the other app's layer is not counted
    assert live["sf_fps"] == 58.8, live
    assert live["sf_gap_p95"] == 17, live
    assert live["sf_jank_pct"] == round(100 * 10 / 470, 1), live
    assert live["cpu_pct"] == 50.0, live  # 400 ticks = 4000 ms over 8000 ms
    assert live["stages"]["raster_p50"] == 4.0, live  # the stages line's p50
    assert live["stages"]["kinds"]["text"] == [1.5, 2.0, 3.0], live
    assert live["app"]["wait_ms"] == [1.0, 2.0, 3.0, 4.0], live  # the wait half, when the run logged it
    assert farm[0]["configs"]["vieww-cpu"]["quality"].endswith("vieww-cpu")
    # The A/B leg is parsed beside the contenders, cold starts empty.
    serial = farm[0]["configs"]["vieww-cpuserial"]
    assert serial["cold_start_median"] is None and serial["phases"]["live2k"]["stages"]["raster_p50"] == 4.0
    assert "wait_ms" not in serial["phases"]["live2k"]["app"], serial  # an old run's line: wait stays absent
    assert farm[0]["configs"]["valdi"]["phases"]["live2k"]["hwui"]["p95_ms"] == 15
    assert farm[0]["configs"]["vieww-cpu"]["cold_start_median"] == 310
    # A phone where the Valdi app was refused: the reason reaches the report.
    bad = tmp / "farm" / "Galaxy-16" / "mw"
    bad.mkdir(parents=True)
    (bad / "result.txt").write_text(
        "device_name=Galaxy\nandroid=16\nmissing_com.snap.valdi.marketwatch=yes\n"
        "install_error_valdi=Failure [INSTALL_FAILED_NO_MATCHING_ABIS]\n"
        "vieww-cpu_status=PASS\nvaldi_status=SKIPPED (not installed)\nstatus=FAIL\n")
    farm = parse_farm(tmp / "farm")
    assert len(farm) == 2, farm
    md = render(farm, {"valdi_build": build})
    assert "valdi: SKIPPED (not installed)" in md and "INSTALL_FAILED_NO_MATCHING_ABIS" in md, md
    assert "| live2k |" in md and "**310**" in md, md
    # The A/B block: rendered when the leg ran, with both legs' numbers and
    # the quality lines that say which policy each took.
    assert "parallel vs serial raster" in md, md
    assert "raster threads vieww-cpuserial" in md, md
    assert "4.00 / 4.00" in md, md  # raster p50, parallel / serial
    # The wait column, beside work wherever work is shown: a value where a run
    # logged it, a dash where one did not (an old run — never a zero).
    assert "app wait p95 (ms)" in md and "app wait (ms)" in md, md
    assert "| live2k | 58.8 / – / 58.8 | 17 / – / 17 | 2.1 / – / 2.1 | 9.00 / – / 9.00 | 2.00 / – / – | 50.0 / – / 50.0 |" in md, md
    assert "| live2k | 4.00 / 4.00 | 2.00 / 2.00 | 9.00 / 9.00 | 2.00 / – | 2.1 / 2.1 |" in md, md
    assert "| load2k | 4.00 / – / – | – / – / – | – / – / – | – / – / – |" in md, md
    assert md.count("1.50 / 1.00 / 0.50 / 0.00 / 0.50") == 2, md  # kinds p50, both legs
    assert "--snap_flavor=production" in md and "quickjs" in md, md
    # Machines: two legs' verdicts become rows beside the phones.
    mdir = tmp / "machines"
    for leg, status, moved in (("linux-x64", "ok", "same"), ("windows-arm64", "pixels moved", "moved")):
        (mdir / leg).mkdir(parents=True)
        (mdir / leg / "verdict.json").write_text(json.dumps({
            "leg": leg, "status": status,
            "machine": {"os": "linux" if "linux" in leg else "windows", "arch": "x86_64", "cores": 4},
            "walks": {"serial": {p: {"median_ms": 1.25, "p90_ms": 2.0, "hash": "ab", "pixels": moved}
                                 for p in ("load", "scroll", "live", "storm")}}}))
    machines = parse_machines(mdir)
    assert [m["leg"] for m in machines] == ["linux-x64", "windows-arm64"], machines
    mmd = render_machines(machines)
    assert "| linux-x64 | linux / x86_64 | 4 | serial | 1.25 | 1.25 | 1.25 | 1.25 | same | ok |" in mmd, mmd
    assert "| moved | pixels moved |" in mmd, mmd
    out = tmp / "out"
    assert main(["--farm", str(tmp / "farm"), "--machines", str(mdir), "--out", str(out)]) == 0
    both = (out / "COMPARE.md").read_text()
    assert "Machines · the CPU path" in both and "| live2k |" in both, both
    assert len(json.loads((out / "compare.json").read_text())["machines"]) == 2
    # Machines only (a branch run: no phones): still a report.
    assert main(["--farm", str(tmp / "none"), "--machines", str(mdir), "--out", str(out),
                 "--header", "run: x"]) == 0
    assert "No phones in this run" in (out / "COMPARE.md").read_text()
    # Energy: a phase that ran on battery reports power and energy per
    # frame; one on USB (charging) reports neither.
    e = tmp / "energy" / "Redmi" / "mw" / "vieww-cpu"
    e.mkdir(parents=True)
    (e / "events.jsonl").write_text(json.dumps({"fw": "vieww", "ev": "stats", "phase": "live2k", "frames": 480,
                                                "work_ms": [5, 9, 12, 20], "interval_ms": [16.6, 17, 33, 40]}) + "\n" +
                                    json.dumps({"fw": "vieww", "ev": "stats", "phase": "storm2k", "frames": 480,
                                                "work_ms": [5, 9, 12, 20], "interval_ms": [16.6, 17, 33, 40]}) + "\n")
    (e / "runner.jsonl").write_text(
        json.dumps({"phase": "live2k", "wall_ms": 8000, "cpu_ticks": 400, "rss_kb": 1, "layer": "x",
                    "power_mw": 1200, "power_samples": 26, "discharging": True, "batt_temp_dC": 312}) + "\n" +
        json.dumps({"phase": "storm2k", "wall_ms": 8000, "cpu_ticks": 400, "rss_kb": 1, "layer": "x",
                    "power_mw": 900, "power_samples": 26, "discharging": False, "batt_temp_dC": 315}) + "\n")
    (e.parent / "result.txt").write_text("device_name=Redmi\nrefresh_hz=60\nvieww-cpu_status=PASS\nstatus=PASS\n")
    energy = parse_farm(tmp / "energy")
    live = energy[0]["configs"]["vieww-cpu"]["phases"]["live2k"]
    storm = energy[0]["configs"]["vieww-cpu"]["phases"]["storm2k"]
    assert live["power_mw"] == 1200 and live["energy_mj_per_frame"] == 20.0, live
    assert "power_mw" not in storm and storm["batt_temp_c"] == 31.5, storm
    emd = render(energy, {"header": "", "valdi_build": {}})
    assert "| live2k | 1200 / – / – | 20.00 / – / – | 31.2 / – / – |" in emd, emd
    assert "| storm2k | 900" not in emd, "a charging phase reports no energy"
    print("compare_report self-test: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
