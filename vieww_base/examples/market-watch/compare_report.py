#!/usr/bin/env python3
"""Valdi vs Vieww: turn a comparison run's raw output into one report.

    compare_report.py --farm DIR --out DIR
    compare_report.py --self-test

Input: ci/mobile/device_farm.py's output: one folder per phone, holding the
job's artifacts with mw/<config>/{events,runner}.jsonl, timestats-<phase>.txt,
gfxinfo-<phase>.txt and mw/result.txt, as run-android.sh writes them, plus
valdi-build.txt at the top (how the Valdi APK was built).

Writes OUT/COMPARE.md (for the job summary and the results branch) and
OUT/compare.json (every number, for anyone who wants to chart it).

What is compared, and with what yardstick: both frameworks as they ship on
phones. SurfaceFlinger's per-layer timestats are the yardstick: frames
  actually presented and the gap between them, measured by the compositor
  the same way for Vieww's NativeActivity surface and Valdi's HWUI views. A
  phase's frame rate is its presented frames over its length; jank is the
  share of gaps over 1.5 display frames. Each app's own numbers (Vieww's frame
  work, Valdi's JS render cost) and HWUI's gfxinfo (Valdi only) say *where*
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
    phases = {}
    for name in sorted(set(runner) | set(app)):
        r, a = runner.get(name, {}), app.get(name, {})
        p = {"app": a}
        wall = r.get("wall_ms") or 0
        if wall:
            p["wall_ms"] = wall
            ticks = r.get("cpu_ticks")
            # A negative delta means the pid changed under the phase (a crash
            # and restart): no figure rather than a wrong one.
            if isinstance(ticks, (int, float)) and ticks >= 0:
                p["cpu_pct"] = round(100.0 * ticks * (1000 / CLK_TCK) / wall, 1)
        p["rss_kb"] = r.get("rss_kb")
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
        gx_file = config_dir / f"gfxinfo-{name}.txt"
        gx = parse_gfxinfo(gx_file.read_text(errors="replace")) if gx_file.exists() else None
        if gx:
            p["hwui"] = gx
        phases[name] = p
    return phases


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
        for config in CONFIGS:
            d = mw / config
            if not d.exists():
                continue
            package = "com.snap.valdi.marketwatch" if config == "valdi" else "dev.vieww.marketwatch"
            starts = [float(x) for x in info.get(f"{config}_cold_start_ms", "").split() if x.replace(".", "").isdigit()]
            phone["configs"][config] = {
                "status": info.get(f"{config}_status", "?"),
                "cold_start_ms": starts,
                "cold_start_median": statistics.median(starts) if starts else None,
                "pss_kb": info.get(f"{config}_pss_kb"),
                "battery_temp_dC": info.get(f"{config}_battery_temp_dC"),
                "phases": phase_stats(d, package, budget),
            }
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
        statuses = ", ".join(f"{c}: {info[f'{c}_status']}" for c in CONFIGS if info.get(f"{c}_status"))
        md.append(f"Runs — {statuses or 'none started'}")
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
        md.append("| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |")
        md.append("|---|---|---|---|---|---|")
        for name in CONTINUOUS:
            p = {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in CONFIGS}
            if not any(p.values()):
                continue
            fps = {c: p[c].get("sf_fps") for c in CONFIGS}
            gap = {c: p[c].get("sf_gap_p95") for c in CONFIGS}
            jank = {c: p[c].get("sf_jank_pct") for c in CONFIGS}
            work = {c: (p[c].get("app", {}).get("work_ms") or p[c].get("app", {}).get("render_ms") or [None, None])[1]
                    for c in CONFIGS}
            cpu = {c: p[c].get("cpu_pct") for c in CONFIGS}
            md.append(f"| {name} | {cell(fps, 'fps', True)} | {cell(gap, 'gap', False, '', 0)} | "
                      f"{cell(jank, 'jank', False)} | {cell(work, 'work', False, '', 2)} | {cell(cpu, 'cpu', False)} |")
        md.append("")
        md.append("| event | app time (ms) | frames presented | CPU % |")
        md.append("|---|---|---|---|")
        for name in EVENTS + IDLE:
            p = {c: cfg.get(c, {}).get("phases", {}).get(name, {}) for c in CONFIGS}
            if not any(p.values()):
                continue
            t = {c: (p[c].get("app", {}).get("work_ms") or p[c].get("app", {}).get("render_ms") or [None])[0]
                 for c in CONFIGS}
            fr = {c: p[c].get("sf_frames") for c in CONFIGS}
            cpu = {c: p[c].get("cpu_pct") for c in CONFIGS}
            md.append(f"| {name} | {cell(t, 't', False, '', 2)} | {cell(fr, 'fr', name not in IDLE, '', 0)} | "
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
                v = {c: ph["configs"].get(c, {}).get("phases", {}).get(name, {}).get("sf_fps") for c in CONFIGS}
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
        "- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on "
        "`before_frame`: one change per drawn frame on both sides.\n"
        "- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat "
        "from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour."
    )
    return "\n".join(md) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--farm", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path("."))
    ap.add_argument("--header", default="")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args(argv)
    if a.self_test:
        return self_test()
    farm = parse_farm(a.farm)
    build = kv_file(a.farm / "valdi-build.txt") if a.farm else {}
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "compare.json").write_text(json.dumps({"valdi_build": build, "phones": farm}, indent=1))
    md = render(farm, {"header": a.header, "valdi_build": build})
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
    for c, pkg in (("vieww-cpu", "dev.vieww.marketwatch"), ("valdi", "com.snap.valdi.marketwatch")):
        d = mw / c
        d.mkdir(parents=True)
        (d / "events.jsonl").write_text(
            json.dumps({"fw": c, "ev": "stats", "phase": "live2k", "frames": 470,
                        ("work_ms" if c != "valdi" else "render_ms"): [5.0, 9.0, 12.0, 20.0],
                        "interval_ms": [16.6, 17.0, 33.0, 40.0]}) + "\n")
        (d / "runner.jsonl").write_text(json.dumps({"phase": "live2k", "wall_ms": 8000, "cpu_ticks": 400,
                                                    "rss_kb": 90000, "layer": "x"}) + "\n")
        (d / "timestats-live2k.txt").write_text(TIMESTATS.replace("dev.vieww.marketwatch", pkg))
        if c == "valdi":
            (d / "gfxinfo-live2k.txt").write_text(GFX)
    (mw / "result.txt").write_text(
        "device_name=Pixel 6\nandroid=14\nsoc=Tensor\nmem_total_kb=8000000\nrefresh_hz=60\n"
        "vieww-cpu_cold_start_ms=300 320 310 305 315\nvaldi_cold_start_ms=420 400 410 430 415\n"
        "vieww-cpu_status=PASS\nvaldi_status=PASS\nvieww-cpu_pss_kb=80000\nvaldi_pss_kb=120000\n"
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
    assert "--snap_flavor=production" in md and "quickjs" in md, md
    print("compare_report self-test: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
