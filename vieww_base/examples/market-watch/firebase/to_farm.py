#!/usr/bin/env python3
"""Firebase Test Lab's Game Loop results -> the layout compare_report.py reads.

    to_farm.py --results DIR --out DIR
    to_farm.py --self-test

Test Lab keeps each phone's device log as a file named `logcat` somewhere
under its results directory. On a Game Loop run Market Watch is launched once
per scenario (1: CPU rasterizer, 2: GPU path) and narrates the same `__MW__`
lines it prints for the device runner. This splits each phone's log at every
launch's `start` line and writes, per phone, the files the AWS runner would
have:

    OUT/<model>/mw/result.txt               device facts, statuses, mode=firebase
    OUT/<model>/mw/vieww-cpu/events.jsonl   that launch's __MW__ events
    OUT/<model>/mw/vieww-cpu/runner.jsonl   each phase's wall time
    OUT/<model>/mw/vieww-cpu/logcat.txt     that launch's `vieww: ` lines

There is no SurfaceFlinger timestats on Test Lab, so the report reads
presented fps and jank from the app's own frame log there (it says so).
"""

import argparse
import json
import pathlib
import re
import sys

PATHS = {"cpu": "vieww-cpu", "gpu": "vieww-gpu"}


def launches(text):
    """The log split into launches: [(events, rate_lines)], in order."""
    out = []
    for line in text.splitlines():
        if "__MW__ " in line:
            raw = line.split("__MW__ ", 1)[1].strip()
            try:
                event = json.loads(raw)
            except ValueError:
                continue
            if event.get("ev") == "start" or not out:
                out.append(([], []))
            out[-1][0].append(event)
        elif "vieww: " in line and out:
            out[-1][1].append(line)
    return out


def safe(name):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", name).strip("_") or "phone"


def convert(results, out):
    """Every phone's log under RESULTS into OUT; returns the phones written."""
    phones = []
    for log in sorted(p for p in results.rglob("logcat*") if p.is_file()):
        runs = launches(log.read_text(errors="replace"))
        if not runs:
            continue
        device = next((e for ev, _ in runs for e in ev if e.get("ev") == "device"), {})
        model = device.get("model") or log.parent.name
        mw = out / safe(f"{model}-{log.parent.name}") / "mw"
        info = {
            "device_name": model,
            "model": model,
            "android": device.get("android", ""),
            "soc": device.get("soc", ""),
            "mode": "firebase",
        }
        overall = "PASS"
        for events, rate in runs:
            start = next((e for e in events if e.get("ev") == "start"), {})
            config = PATHS.get(start.get("path"))
            if not config:
                continue
            d = mw / config
            d.mkdir(parents=True, exist_ok=True)
            (d / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))
            (d / "logcat.txt").write_text("".join(line + "\n" for line in rate))
            # Each phase's wall time, from one phase line to the next.
            marks = [(e["phase"], e["t"]) for e in events if e.get("ev") == "phase" and "t" in e]
            end = next((e["t"] for e in events if e.get("ev") == "done" and "t" in e), None)
            rows = []
            for i, (name, t) in enumerate(marks):
                t_next = marks[i + 1][1] if i + 1 < len(marks) else end
                if t_next is not None:
                    rows.append({"phase": name, "wall_ms": round(t_next - t)})
            (d / "runner.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
            budget = next((e.get("budget_ms") for e in events if e.get("ev") == "stats" and e.get("budget_ms")), None)
            if budget and "refresh_hz" not in info:
                info["refresh_hz"] = f"{1000.0 / budget:.1f}"
            done = any(e.get("ev") == "done" for e in events)
            info[f"{config}_status"] = "PASS" if done else "FAIL (the schedule did not finish)"
            if not done:
                overall = "FAIL"
        info["status"] = overall
        mw.mkdir(parents=True, exist_ok=True)
        (mw / "result.txt").write_text("".join(f"{k}={v}\n" for k, v in info.items()))
        phones.append(model)
    return phones


def self_test():
    import tempfile
    tmp = pathlib.Path(tempfile.mkdtemp())
    log = tmp / "results" / "Pixel8-34-en-portrait" / "logcat"
    log.parent.mkdir(parents=True)

    def mw(e):
        return f"11-17 10:00:00.000  123  123 I vieww   : __MW__ {json.dumps(e)}"

    lines = []
    for path, scenario in (("cpu", 1), ("gpu", 2)):
        lines += [
            mw({"fw": "vieww", "ev": "device", "scenario": scenario, "model": "Pixel 8", "android": "14", "soc": "Tensor G3"}),
            mw({"fw": "vieww", "ev": "start", "path": path, "phases": 2}),
            "11-17 10:00:00.100  123  123 I vieww   : vieww: window#primary asks for 120 Hz at start; asked",
            mw({"fw": "vieww", "ev": "phase", "phase": "scroll2k", "t": 1500.0}),
            mw({"fw": "vieww", "ev": "stats", "phase": "scroll2k", "frames": 960, "work_ms": [3, 4, 5, 6],
                "interval_ms": [8.3, 8.4, 8.6, 9.0], "jank": 2, "budget_ms": 8.33}),
            mw({"fw": "vieww", "ev": "phase", "phase": "live2k", "t": 9500.0}),
            mw({"fw": "vieww", "ev": "done", "t": 17500.0}),
            "11-17 10:00:20.000  999  999 I other   : unrelated",
        ]
    log.write_text("\n".join(lines) + "\n")
    out = tmp / "farm"
    assert convert(tmp / "results", out) == ["Pixel 8"]
    mwdir = next(out.rglob("mw"))
    info = dict(l.split("=", 1) for l in (mwdir / "result.txt").read_text().splitlines())
    assert info["vieww-cpu_status"] == "PASS" and info["vieww-gpu_status"] == "PASS", info
    assert info["refresh_hz"] == "120.0" and info["mode"] == "firebase", info
    runner = [json.loads(l) for l in (mwdir / "vieww-gpu" / "runner.jsonl").read_text().splitlines()]
    assert runner == [{"phase": "scroll2k", "wall_ms": 8000}, {"phase": "live2k", "wall_ms": 8000}], runner
    assert "asks for 120 Hz" in (mwdir / "vieww-cpu" / "logcat.txt").read_text()
    # And the report reads it: fps from the app's own frames, 960 in 8 s.
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
    import compare_report
    farm = compare_report.parse_farm(out)
    scroll = farm[0]["configs"]["vieww-gpu"]["phases"]["scroll2k"]
    assert scroll["app_fps"] == 120.0, scroll
    md = compare_report.render(farm, {"header": "", "valdi_build": {}})
    assert "| scroll2k | 120.0 / 120.0 / – |" in md, md
    assert "Firebase Test Lab" in md, md
    print("to_farm self-test: ok")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--results", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args(argv)
    if a.self_test:
        self_test()
        return 0
    if not a.results or not a.out:
        ap.error("--results and --out are required")
    phones = convert(a.results, a.out)
    print(f"to_farm: {len(phones)} phone(s): {', '.join(phones) or 'none'}")
    return 0 if phones else 1


if __name__ == "__main__":
    sys.exit(main())
