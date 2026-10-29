"""Performance budgets: fail when a measurement drifts past its recorded figure.

    python3 perf_budget.py check  BUDGETS RECEIPT|SUMMARY... [--class NAME]
                                  [--baseline LAST.json] [--write-measured OUT.json]
                                  [--advisory]
    python3 perf_budget.py record BUDGETS RECEIPT|SUMMARY... [--class NAME]

FW-08. The receipts (`examples/fixtures --receipt`, `vieww.bench-receipt/1`)
and Device Farm's `summary.json` record every timing, and until this nothing
compared run N with run N-1: a fill cache that cost 1.9-6.6 ms on three
phones was caught only because somebody read a Device Farm table.

Two kinds of input, told apart by their shape:

* a **fixtures receipt**: each fixture's `cpu_p50_ms` (and `gpu_best_ms`
  for the census), under a machine class of `os / arch / cpu_model` unless
  `--class` names one (the self-hosted machines pass `kabylake-linux` and
  the like, so a kernel update does not orphan the budget);
* a **Device Farm summary** (`summary.json` from `ci/mobile/device_farm.py`):
  each phone's median frame time per pass (`cpu`, `gpu`, `auto`), under a
  class named after the phone model.

`check` fails (exit 1) on any metric more than `tolerance_percent` above its
budget **and** more than `min_delta_ms` above it in absolute terms (10% of a
0.3 ms frame is noise, not a regression), and prints the table; a class with no budget yet is reported, not
failed, the way `size-budget.sh` treats a target it has no figure for.
`record` writes today's figures for the classes it saw, to commit after
review. Standard library only.
"""

import json
import os
import re
import sys

SCHEMA = "vieww.perf-budgets/1"
DEFAULT_TOLERANCE = 10.0
DEFAULT_MIN_DELTA_MS = 1.0
# On phones, what ships (`auto`) and the GPU path. The CPU pass moves by
# 50% between runs on the same phone depending on which cores the scheduler
# gives it (known-failures.txt has the history), so it is not gated here.
PASSES = ("gpu", "auto")


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", (s or "unknown").lower()).strip("-")


def median_ms(text):
    """'median 15.59ms, worst …' or 'median 348.8µs, …' -> ms."""
    m = re.search(r"median\s+([0-9.]+)\s*(ms|µs|us)", text or "")
    if not m:
        return None
    v = float(m.group(1))
    return v / 1000.0 if m.group(2) in ("µs", "us") else v


def measurements(doc, forced_class=None):
    """{class: {metric: ms}} from one receipt or summary."""
    out = {}
    if isinstance(doc, dict) and "devices" in doc:  # Device Farm summary.json
        for row in doc.get("devices", []):
            r = row.get("result") or {}
            cls = forced_class or "phone-" + slug(r.get("model") or row.get("device"))
            for p in PASSES:
                v = median_ms(r.get(f"{p}_frame_time"))
                if v is not None:
                    out.setdefault(cls, {})[f"{p}_frame_time_p50_ms"] = v
        return out
    if isinstance(doc, dict) and str(doc.get("schema", "")).startswith("vieww.bench-receipt/"):
        m = doc.get("machine") or {}
        cls = forced_class or "/".join(slug(m.get(k)) for k in ("os", "arch", "cpu_model"))
        mode = doc.get("mode", "gallery")
        for f in doc.get("fixtures", []):
            name = f.get("name")
            if not name:
                continue
            if f.get("cpu_p50_ms") is not None:
                out.setdefault(cls, {})[f"{mode}/{name}/cpu_p50_ms"] = float(f["cpu_p50_ms"])
            if f.get("gpu_best_ms") is not None:
                out.setdefault(cls, {})[f"{mode}/{name}/gpu_best_ms"] = float(f["gpu_best_ms"])
        return out
    raise ValueError("neither a vieww.bench-receipt nor a Device Farm summary.json")


def load_budgets(path):
    try:
        with open(path) as f:
            b = json.load(f)
    except FileNotFoundError:
        b = {"schema": SCHEMA, "tolerance_percent": DEFAULT_TOLERANCE,
             "min_delta_ms": DEFAULT_MIN_DELTA_MS, "classes": {}}
    validate(b)
    return b


def validate(b):
    """The budgets file's shape, so a hand edit cannot quietly disable it."""
    if b.get("schema") != SCHEMA:
        raise ValueError(f"budgets schema is {b.get('schema')!r}, not {SCHEMA!r}")
    tol = b.get("tolerance_percent")
    if not isinstance(tol, (int, float)) or not 0 < tol <= 100:
        raise ValueError("tolerance_percent must be a number in (0, 100]")
    floor = b.get("min_delta_ms", DEFAULT_MIN_DELTA_MS)
    if not isinstance(floor, (int, float)) or floor < 0:
        raise ValueError("min_delta_ms must be a number >= 0")
    classes = b.get("classes")
    if not isinstance(classes, dict):
        raise ValueError("classes must be an object")
    for cls, metrics in classes.items():
        if not isinstance(metrics, dict) or not metrics:
            raise ValueError(f"class {cls!r} must map metric names to milliseconds")
        for name, v in metrics.items():
            if not isinstance(v, (int, float)) or v <= 0:
                raise ValueError(f"{cls} {name}: {v!r} is not a positive number of ms")


def gather(paths, forced_class):
    seen = {}
    for p in paths:
        with open(p) as f:
            for cls, metrics in measurements(json.load(f), forced_class).items():
                seen.setdefault(cls, {}).update(metrics)
    return seen


def check(budgets, seen):
    """(table rows, failures, unknown classes)."""
    tol = budgets["tolerance_percent"]
    floor = budgets.get("min_delta_ms", DEFAULT_MIN_DELTA_MS)
    rows, failures, unknown = [], [], []
    for cls, metrics in sorted(seen.items()):
        budget = budgets["classes"].get(cls)
        if budget is None:
            unknown.append(cls)
            continue
        for name, now in sorted(metrics.items()):
            was = budget.get(name)
            if was is None:
                rows.append((cls, name, None, now, None, "new (no budget)"))
                continue
            change = (now - was) / was * 100.0
            if change > tol and now - was > floor:
                verdict = "OVER"
                failures.append((cls, name, was, now, change))
            elif change < -tol:
                verdict = "faster (re-record?)"
            else:
                verdict = "ok"
            rows.append((cls, name, was, now, change, verdict))
    return rows, failures, unknown


MEASURED_SCHEMA = "vieww.perf-measured/1"
DEFAULT_BASELINE_TOLERANCE = 25.0


def load_baseline(path):
    """A previous run's `--write-measured` file, or {} when there is none."""
    try:
        with open(path) as f:
            doc = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return {}
    if doc.get("schema") != MEASURED_SCHEMA or not isinstance(doc.get("classes"), dict):
        return {}
    return {c: m for c, m in doc["classes"].items() if isinstance(m, dict) and m}


def write_measured(path, seen):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as f:
        json.dump({"schema": MEASURED_SCHEMA,
                   "classes": {c: {k: round(v, 3) for k, v in sorted(m.items())} for c, m in seen.items()}},
                  f, indent=2, sort_keys=True)
        f.write("\n")


def take(rest, flag, has_value=True):
    if flag not in rest:
        return None
    i = rest.index(flag)
    if not has_value:
        del rest[i]
        return True
    v = rest[i + 1]
    del rest[i:i + 2]
    return v


def main(argv):
    if len(argv) < 3 or argv[0] not in ("check", "record"):
        print(__doc__)
        return 2
    cmd, budgets_path, *rest = argv
    forced = take(rest, "--class")
    baseline_path = take(rest, "--baseline")
    measured_path = take(rest, "--write-measured")
    advisory = bool(take(rest, "--advisory", has_value=False))
    budgets = load_budgets(budgets_path)
    seen = gather(rest, forced)
    if measured_path:
        write_measured(measured_path, seen)
    if cmd == "record":
        for cls, metrics in seen.items():
            budgets["classes"][cls] = {k: round(v, 3) for k, v in sorted(metrics.items())}
        validate(budgets)
        with open(budgets_path, "w") as f:
            json.dump(budgets, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"perf-budget: recorded {sum(len(m) for m in seen.values())} metric(s) "
              f"for {', '.join(sorted(seen))} in {budgets_path}")
        return 0
    rows, failures, unknown = check(budgets, seen)
    # A class nobody has committed a budget for is held to its own last run
    # (`--baseline`, the previous run's `--write-measured` as published to
    # ci-results), at a wider tolerance: so a new machine is gated from its
    # second run on, with nothing to commit by hand. A committed budget,
    # once there, takes over.
    baseline = load_baseline(baseline_path) if baseline_path else {}
    held = {c: baseline[c] for c in unknown if c in baseline}
    if held:
        btol = budgets.get("baseline_tolerance_percent", DEFAULT_BASELINE_TOLERANCE)
        last = dict(budgets, tolerance_percent=btol, classes=held)
        brows, bfail, _ = check(last, {c: seen[c] for c in held})
        rows += [(c, n, w, now, ch, f"{v} (vs last run, ±{btol:g}%)") for c, n, w, now, ch, v in brows]
        failures += bfail
        unknown = [c for c in unknown if c not in held]
    tol = budgets["tolerance_percent"]
    print(f"| class | metric | budget ms | now ms | change | verdict (tolerance ±{tol:g}%) |")
    print("|---|---|---|---|---|---|")
    for cls, name, was, now, change, verdict in rows:
        w = "–" if was is None else f"{was:.3f}"
        c = "–" if change is None else f"{change:+.1f}%"
        print(f"| {cls} | {name} | {w} | {now:.3f} | {c} | {verdict} |")
    for cls in unknown:
        print(f"perf-budget: no budget and no previous run for class {cls!r}: not gated "
              f"this time{' (this run is the baseline for the next)' if measured_path else ''}")
    if failures:
        level = "warning" if advisory else "error"
        for cls, name, was, now, change in failures:
            print(f"::{level}::perf-budget: {cls} {name}: {now:.3f} ms against {was:.3f} ms "
                  f"({change:+.1f}%)")
        if advisory:
            print("perf-budget: over budget, reported only (--advisory: a shared hosted runner)")
            return 0
        return 1
    print("perf-budget: within budget" if rows else "perf-budget: nothing gated")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
