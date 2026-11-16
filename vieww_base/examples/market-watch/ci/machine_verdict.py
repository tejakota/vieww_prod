#!/usr/bin/env python3
"""One machine's verdict for the Market Watch CPU bench.

    machine_verdict.py --dir OUT --leg linux-x64 --runner "Linux/X64 (ubuntu-24.04)" [--allow-changed]
    machine_verdict.py --self-test

OUT holds cpu_bench's --json files: head-<walk>.json for this ref and, on a
branch, main-<walk>.json for main built on the same machine. Writes
OUT/verdict.json (what the report reads) and prints a Markdown block for the
job summary.

The rule: a phase whose byte hash differs between main and this ref, on the
same machine and the same walk, moved pixels. That fails the leg (exit 1)
unless --allow-changed (the commit carries a `Pixels-Changed:` trailer).
Times are reported, never judged: a hosted runner's neighbours make a single
run's milliseconds a hint, not a verdict.
"""
import argparse
import json
import pathlib
import sys
import tempfile

PHASES = ("load", "scroll", "live", "storm")


def load(path):
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    data["by_phase"] = {p["phase"]: p for p in data.get("phases", [])}
    return data


def verdict(out, leg, runner, allow):
    walks = {}
    moved_any = False
    machine = None
    for head_file in sorted(out.glob("head-*.json")):
        walk = head_file.stem[len("head-"):]
        head = load(head_file)
        main = load(out / f"main-{walk}.json")
        if head is None:
            continue
        machine = machine or {k: head.get(k) for k in ("os", "arch", "cores", "size")}
        phases = {}
        for name in PHASES:
            h = head["by_phase"].get(name)
            if h is None:
                continue
            row = {"median_ms": h["median_ms"], "p90_ms": h["p90_ms"], "hash": h["hash"]}
            m = main["by_phase"].get(name) if main else None
            if m is not None:
                row["main_median_ms"] = m["median_ms"]
                row["main_hash"] = m["hash"]
                row["pixels"] = "same" if m["hash"] == h["hash"] else "moved"
                moved_any |= row["pixels"] == "moved"
            else:
                row["pixels"] = "not compared"
            phases[name] = row
        walks[walk] = phases
    status = "ok"
    if not walks:
        status = "no results"
    elif moved_any:
        status = "pixels changed (allowed)" if allow else "pixels moved"
    result = {"leg": leg, "runner": runner, "machine": machine or {}, "status": status, "walks": walks}
    (out / "verdict.json").write_text(json.dumps(result, indent=1))
    return result


def markdown(result):
    m = result["machine"]
    lines = [
        f"### {result['leg']} · {result['status']}",
        "",
        f"{result['runner']} · {m.get('os', '?')}/{m.get('arch', '?')}, {m.get('cores', '?')} cores · frames {m.get('size', '?')}",
        "",
        "| walk | phase | raster p50 (ms) | p90 | main p50 | pixels vs main |",
        "|---|---|---|---|---|---|",
    ]
    for walk, phases in result["walks"].items():
        for name, r in phases.items():
            main = f"{r['main_median_ms']:.2f}" if "main_median_ms" in r else "–"
            lines.append(f"| {walk} | {name} | {r['median_ms']:.2f} | {r['p90_ms']:.2f} | {main} | {r['pixels']} |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dir", type=pathlib.Path)
    ap.add_argument("--leg", default="?")
    ap.add_argument("--runner", default="?")
    ap.add_argument("--allow-changed", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args(argv)
    if a.self_test:
        return self_test()
    result = verdict(a.dir, a.leg, a.runner, a.allow_changed)
    print(markdown(result))
    if result["status"] in ("pixels moved", "no results"):
        print(f"::error::{a.leg}: {result['status']}", file=sys.stderr)
        return 1
    return 0


def self_test():
    def bench(hashes, ms):
        return json.dumps({
            "os": "linux", "arch": "x86_64", "cores": 4, "walk": "1", "size": "1081x2340",
            "frames": 120, "runs": 3,
            "phases": [{"phase": p, "median_ms": ms, "p90_ms": ms + 1, "hash": h} for p, h in zip(PHASES, hashes)],
        })

    same = ["a1", "b2", "c3", "d4"]
    with tempfile.TemporaryDirectory() as d:
        out = pathlib.Path(d)
        # On main: nothing to compare with, still a pass.
        (out / "head-serial.json").write_text(bench(same, 5.0))
        r = verdict(out, "linux-x64", "Linux/X64", False)
        assert r["status"] == "ok", r
        assert r["walks"]["serial"]["storm"]["pixels"] == "not compared", r
        # A branch that draws the same bytes: pass, with main's time beside.
        (out / "main-serial.json").write_text(bench(same, 6.0))
        r = verdict(out, "linux-x64", "Linux/X64", False)
        assert r["status"] == "ok", r
        assert r["walks"]["serial"]["live"]["main_median_ms"] == 6.0, r
        # One phase moved: fail, unless the commit says it means to.
        (out / "main-serial.json").write_text(bench(["a1", "b2", "XX", "d4"], 6.0))
        r = verdict(out, "linux-x64", "Linux/X64", False)
        assert r["status"] == "pixels moved", r
        assert r["walks"]["serial"]["live"]["pixels"] == "moved", r
        assert r["walks"]["serial"]["storm"]["pixels"] == "same", r
        assert main(["--dir", d, "--leg", "x"]) == 1
        assert main(["--dir", d, "--leg", "x", "--allow-changed"]) == 0
        assert json.loads((out / "verdict.json").read_text())["status"] == "pixels changed (allowed)"
    with tempfile.TemporaryDirectory() as d:
        # A leg that produced nothing fails rather than passing silently.
        assert main(["--dir", d, "--leg", "empty"]) == 1
    print("machine_verdict self-test: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
