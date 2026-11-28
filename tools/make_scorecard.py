#!/usr/bin/env python3
"""Per-app scorecard from device runs (SCORE-01).

    python3 tools/make_scorecard.py --commit SHA --out ci-results/SCORECARD \
        --app market-watch=path/to/compare.json [--app NAME=compare.json ...]

Reads `compare.json` (the report `firebase/compare_report.py` builds from the
USB runner or a farm run) and writes `SCORECARD.md` and `SCORECARD.csv`: one
row per app x phone x rasteriser path with cold start, scroll and storm frame
work (p50 / p95), presented fps, jank, PSS and the frames an idle screen
still presents. The three tiers (low / medium / high) are always listed per
app: a tier with no run is a row that says "no run", never a missing row, so
a release cannot quietly lose its Low-tier evidence. Standard library only.
"""
import argparse
import csv
import json
import re
import sys

TIERS = ("low", "medium", "high")
COLUMNS = ["app", "tier", "device", "path", "cold_start_ms", "scroll_p50_ms", "scroll_p95_ms",
           "scroll_fps", "storm_p50_ms", "storm_p95_ms", "storm_fps", "storm_jank_pct",
           "pss_mb", "idle_frames", "commit"]


def tier_of(config):
    m = re.search(r"tier ([a-z]+)", config.get("quality") or "")
    return m.group(1) if m else None


def phase_stats(phases, name):
    """(p50, p95, fps, jank%) for the first phase whose name starts with `name`."""
    # The 2,000-row phase is the reference size; the 20k one is the stress.
    for key in sorted(phases, key=lambda k: (not k.endswith("2k"), k)):
        if key.startswith(name) and "-" not in key:
            phase = phases[key]
            # The runner logs a phase's work as [p50, p95, p99, max].
            work = (phase.get("app") or {}).get("work_ms") or []
            p50 = work[0] if len(work) > 0 else None
            p95 = work[1] if len(work) > 1 else None
            return (p50, p95, phase.get("app_fps"), phase.get("app_jank_pct"))
    return (None, None, None, None)


def rows_for(app, doc, commit):
    rows = []
    for phone in doc.get("phones", []):
        for path, config in sorted((phone.get("configs") or {}).items()):
            phases = config.get("phases") or {}
            scroll = phase_stats(phases, "scroll")
            # Market Watch's heavy phase is `storm`; an app's own scenario
            # (CI-01, vieww_platform_winit::farm) has a `sheet` there instead.
            storm = phase_stats(phases, "storm")
            if storm == (None, None, None, None):
                storm = phase_stats(phases, "sheet")
            idle = [(p.get("app") or {}).get("frames") for k, p in phases.items() if k.startswith("idle")]
            pss = config.get("pss_kb")
            cold = config.get("cold_start_median")
            rows.append({
                "app": app, "tier": tier_of(config) or "unknown", "device": phone.get("device"),
                "path": path.replace("vieww-", ""),
                "cold_start_ms": None if cold is None else round(float(cold)),
                "scroll_p50_ms": scroll[0], "scroll_p95_ms": scroll[1], "scroll_fps": scroll[2],
                "storm_p50_ms": storm[0], "storm_p95_ms": storm[1], "storm_fps": storm[2],
                "storm_jank_pct": storm[3],
                "pss_mb": None if not pss else round(int(pss) / 1024.0, 1),
                "idle_frames": max((i for i in idle if i is not None), default=None),
                "commit": commit})
    return rows


def with_missing_tiers(app, rows, commit):
    out = list(rows)
    have = {r["tier"] for r in rows if r["app"] == app}
    for tier in TIERS:
        if tier not in have:
            out.append({"app": app, "tier": tier, "device": "no run", "path": "-", "commit": commit})
    return out


def cell(v, digits=1):
    if v is None:
        return "–"
    if isinstance(v, float):
        return f"{v:.{digits}f}"
    return str(v)


def markdown(rows, commit):
    lines = [f"# Per-app scorecard — commit `{commit}`", "",
             "Frame work is the app's own time per frame; budget is 16.7 ms at 60 Hz. "
             "A tier with no run is listed as **no run**.", ""]
    for app in sorted({r["app"] for r in rows}):
        lines += [f"## {app}", "",
                  "| tier | device | path | cold start ms | scroll p50 / p95 ms | scroll fps | storm or sheet p50 / p95 ms | fps | jank % | PSS MB | idle frames |",
                  "|---|---|---|---|---|---|---|---|---|---|---|"]
        for r in sorted((r for r in rows if r["app"] == app),
                        key=lambda r: (TIERS.index(r["tier"]) if r["tier"] in TIERS else 9, r["device"], r["path"])):
            if r["device"] == "no run":
                lines.append(f"| {r['tier']} | **no run** | | | | | | | | | |")
                continue
            lines.append("| " + " | ".join([
                r["tier"], str(r["device"]), r["path"], cell(r.get("cold_start_ms"), 0),
                f"{cell(r.get('scroll_p50_ms'), 2)} / {cell(r.get('scroll_p95_ms'), 2)}",
                cell(r.get("scroll_fps")),
                f"{cell(r.get('storm_p50_ms'), 2)} / {cell(r.get('storm_p95_ms'), 2)}",
                cell(r.get("storm_fps")), cell(r.get("storm_jank_pct")),
                cell(r.get("pss_mb")), cell(r.get("idle_frames"), 0)]) + " |")
        lines.append("")
    return "\n".join(lines)


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", action="append", required=True, help="NAME=compare.json")
    ap.add_argument("--commit", default="unknown")
    ap.add_argument("--out", default="SCORECARD")
    args = ap.parse_args(argv)
    rows = []
    for spec in args.app:
        name, _, path = spec.partition("=")
        with open(path) as f:
            rows += with_missing_tiers(name, rows_for(name, json.load(f), args.commit), args.commit)
    with open(args.out + ".md", "w") as f:
        f.write(markdown(rows, args.commit) + "\n")
    with open(args.out + ".csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=COLUMNS)
        w.writeheader()
        for r in rows:
            w.writerow({k: r.get(k) for k in COLUMNS})
    print(f"scorecard: {len(rows)} rows -> {args.out}.md, {args.out}.csv")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
