"""Performance budgets: fail when a measurement drifts past its recorded figure.

    python3 perf_budget.py check  BUDGETS RECEIPT|SUMMARY... [--class NAME]
                                  [--baseline LAST.json] [--write-measured OUT.json]
                                  [--advisory]
    python3 perf_budget.py record BUDGETS RECEIPT|SUMMARY... [--class NAME]

FW-08. The receipts (`examples/fixtures --receipt`, `vieww.bench-receipt/1`)
and Device Farm's `summary.json` record every timing, and until this nothing
compared run N with run N-1: a fill cache that cost 1.9-6.6 ms on three
phones was caught only because somebody read a Device Farm table.

Three kinds of input, told apart by their shape:

* a **fixtures receipt**: each fixture's `cpu_p50_ms` (and `gpu_best_ms`
  for the census), under a machine class of `os / arch / cpu_model` unless
  `--class` names one (the self-hosted machines pass `kabylake-linux` and
  the like, so a kernel update does not orphan the budget);
* a **Device Farm summary** (`summary.json` from `ci/mobile/device_farm.py`):
  each phone's median frame time per pass (`cpu`, `gpu`, `auto`), under a
  class named after the phone model;
* a **baseline report** (`vieww.baseline/1`, the `examples/baseline.rs`
  windowed measurement): `peak_rss_mb`, FW-15's interim memory budget — the
  class should be forced with `--class` (the hardware legs use
  `hardware-linux` / `hardware-windows`), because the report names its `os`
  and `arch` but not the machine, and a budget keyed to "linux" would fire
  for somebody else's reasons the way an un-named target does in
  `size-budget.sh`.

Most metrics are milliseconds; `peak_rss_mb` is megabytes. The tolerance and
the absolute floor are unit-free on purpose — 10% of a 0.3 ms frame and 10%
of a 120 MB budget are both "moved enough to matter" — and the floor's name
(`min_delta_ms`) predates the second unit it now also serves.

`check` fails (exit 1) on any metric more than `tolerance_percent` above its
budget **and** more than `min_delta_ms` above it in absolute terms (10% of a
0.3 ms frame is noise, not a regression), and prints the table; a class with no budget yet is reported, not
failed, the way `size-budget.sh` treats a target it has no figure for.
`record` writes today's figures for the classes it saw, to commit after
review. Standard library only.

## Known limitations (FW-07)

A metric can be *known to miss its real budget* — the 60 Hz 16.7 ms frame
time, say — while still drift-gating against a recorded figure, so a slow
regression fails even though the absolute miss is accepted. Those live in
the budgets file under `known_limitations`, keyed like `classes`:

    "known_limitations": {
      "kabylake-linux": {
        "gallery/00-layers-nested/cpu_p50_ms": {
          "target_ms": 16.7,
          "since": "2026-10-15",
          "reason": "…the decision, named…"
        }
      }
    }

The `reason` is the comment the file format cannot carry: it names the
decision that accepted the miss and what would close it. The discipline is
`known-failures.txt`'s, word for word: anything *not* listed fails the run
for missing its target only when a budget entry exists to drift against,
and **a listed entry that comes in under its `target_ms` is flagged stale**
(a `::warning::`, never a pass) so a closed limitation does not linger
unnoticed — remove it in the same commit as the win that closed it. A
limitation never licenses drift: the tolerance check runs exactly as it
would without it.

## Ceilings (FW-16)

Drift is relative; some budgets are absolute. `ceilings` holds those — a
fixed number a metric may never exceed, on the device tiers named:

    "ceilings": {
      "auto_first_frame_ms": {
        "ceiling_ms": 500,
        "tiers": ["medium", "high"],
        "reason": "…the target, and where it came from…"
      }
    }

A Device Farm row's tier is the one the app itself chose (`tier medium` in
`auto_cache_budgets`), so the ceiling follows the framework's own reading of
the phone rather than a hand-kept list of models. First frame is
`auto_first_frame_ms` on iOS (the app's own clock) and `auto_cold_start_ms`
(`am start -W`'s total) on Android, the median of the runs either way. A
ceiling applies with or without a drift budget for the class, and
`--advisory` softens it like any other failure.
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


def median_of_runs(text):
    """'623 611 603' -> 611; anything else ('n/a (no console…)') -> None."""
    if not re.fullmatch(r"[\d.\s]+", (text or "").strip() or "x"):
        return None
    values = sorted(float(v) for v in text.split())
    n = len(values)
    return values[n // 2] if n % 2 else (values[n // 2 - 1] + values[n // 2]) / 2.0


def first_frame_ms(result):
    """The shipped (`auto`) pass's first-frame time: iOS's own figure, else
    Android's `am start -W` total."""
    for key in ("auto_first_frame_ms", "auto_cold_start_ms"):
        v = median_of_runs(result.get(key))
        if v is not None:
            return v
    return None


def device_tier(result):
    """The tier the app chose for itself, from `…(tier medium)…`."""
    m = re.search(r"\(tier ([a-z]+)\)", result.get("auto_cache_budgets") or "")
    return m.group(1) if m else None


def tiers(doc, forced_class=None):
    """{class: tier} for a Device Farm summary; {} for anything else."""
    out = {}
    if isinstance(doc, dict) and "devices" in doc:
        for row in doc.get("devices", []):
            r = row.get("result") or {}
            cls = forced_class or "phone-" + slug(r.get("model") or row.get("device"))
            tier = device_tier(r)
            if tier:
                out[cls] = tier
    return out


def check_ceilings(budgets, seen, tier_of):
    """(rows, failures) for every `ceilings` metric a tiered class reported."""
    rows, failures = [], []
    for name, entry in sorted((budgets.get("ceilings") or {}).items()):
        for cls, metrics in sorted(seen.items()):
            now = metrics.get(name)
            tier = tier_of.get(cls)
            if now is None or tier not in entry["tiers"]:
                continue
            ceiling = entry["ceiling_ms"]
            verdict = "ok" if now <= ceiling else "OVER CEILING"
            rows.append((cls, name, ceiling, now, tier, verdict))
            if now > ceiling:
                failures.append((cls, name, ceiling, now, tier))
    return rows, failures


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
            first = first_frame_ms(r)
            if first is not None:
                out.setdefault(cls, {})["auto_first_frame_ms"] = first
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
    if isinstance(doc, dict) and str(doc.get("schema", "")).startswith("vieww.baseline/"):
        cls = forced_class or "-".join(("baseline", slug(doc.get("os")), slug(doc.get("arch"))))
        # FW-15: the windowed baseline's peak RSS is the memory figure the
        # audit measured (150 MB Linux against a 40 MB target); the interim
        # 120 MB row lives in perf-budgets.json with the ratchet noted there.
        # Peak rather than idle-end: idle-end hides a leak that has not run
        # yet, and the budget is about what a user's machine is asked to hold.
        if doc.get("peak_rss_bytes") is not None:
            out.setdefault(cls, {})["peak_rss_mb"] = doc["peak_rss_bytes"] / (1024.0 * 1024.0)
        return out
    raise ValueError("not a vieww.bench-receipt, Device Farm summary.json or vieww.baseline report")


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
    ceilings = b.get("ceilings")
    if ceilings is not None:
        if not isinstance(ceilings, dict) or not ceilings:
            raise ValueError("ceilings must be a non-empty object or absent")
        for name, entry in ceilings.items():
            if not isinstance(entry, dict):
                raise ValueError(f"ceilings {name}: entry must be an object")
            c = entry.get("ceiling_ms")
            if not isinstance(c, (int, float)) or c <= 0:
                raise ValueError(f"ceilings {name}: ceiling_ms must be a positive number of ms")
            t = entry.get("tiers")
            if not isinstance(t, list) or not t or not all(isinstance(x, str) and x for x in t):
                raise ValueError(f"ceilings {name}: tiers must be a non-empty list of tier names")
            reason = entry.get("reason")
            if not isinstance(reason, str) or not reason.strip():
                raise ValueError(f"ceilings {name}: reason must be a non-empty string")
    limitations = b.get("known_limitations")
    if limitations is None:
        return
    if not isinstance(limitations, dict) or not limitations:
        raise ValueError("known_limitations must be a non-empty object or absent")
    for cls, metrics in limitations.items():
        if not isinstance(metrics, dict) or not metrics:
            raise ValueError(f"known_limitations {cls!r} must map metric names to entries")
        budget = classes.get(cls)
        if not isinstance(budget, dict) or not budget:
            raise ValueError(
                f"known_limitations {cls!r} has no budget class: a limitation "
                "drift-gates against a recorded figure, so the figure must exist")
        for name, entry in metrics.items():
            if name not in budget:
                raise ValueError(
                    f"known_limitations {cls} {name}: no budget figure to gate "
                    "drift against")
            if not isinstance(entry, dict):
                raise ValueError(f"known_limitations {cls} {name}: entry must be an object")
            target = entry.get("target_ms")
            if not isinstance(target, (int, float)) or target <= 0:
                raise ValueError(
                    f"known_limitations {cls} {name}: target_ms must be a "
                    "positive number of ms")
            reason = entry.get("reason")
            if not isinstance(reason, str) or not reason.strip():
                raise ValueError(
                    f"known_limitations {cls} {name}: reason must be a "
                    "non-empty string naming the decision that accepted the miss")


def gather_tiers(paths, forced_class):
    out = {}
    for p in paths:
        with open(p) as f:
            out.update(tiers(json.load(f), forced_class))
    return out


def gather(paths, forced_class):
    seen = {}
    for p in paths:
        with open(p) as f:
            for cls, metrics in measurements(json.load(f), forced_class).items():
                seen.setdefault(cls, {}).update(metrics)
    return seen


def check(budgets, seen):
    """(table rows, failures, unknown classes, stale limitations)."""
    tol = budgets["tolerance_percent"]
    floor = budgets.get("min_delta_ms", DEFAULT_MIN_DELTA_MS)
    limitations = budgets.get("known_limitations") or {}
    rows, failures, unknown, stale = [], [], [], []
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
            limitation = limitations.get(cls, {}).get(name)
            if change > tol and now - was > floor:
                # A known limitation never licenses drift: the tolerance
                # check runs exactly as it would without one.
                verdict = "OVER"
                failures.append((cls, name, was, now, change))
            elif limitation is not None and now <= limitation["target_ms"]:
                # known-failures.txt's discipline, word for word: a listed
                # entry that now passes is flagged stale, never counted a
                # pass — the limitation should be removed with the win.
                verdict = "stale limitation — remove it"
                stale.append((cls, name, limitation["target_ms"], now))
            elif change < -tol:
                verdict = "faster (re-record?)"
            else:
                verdict = "ok"
            rows.append((cls, name, was, now, change, verdict))
    return rows, failures, unknown, stale


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
    tier_of = gather_tiers(rest, forced)
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
        # A re-recorded figure that has come in under a limitation's target
        # is the moment to drop the limitation, said out loud while the file
        # is open in the editor.
        for cls, metrics in seen.items():
            for name, now in metrics.items():
                entry = (budgets.get("known_limitations") or {}).get(cls, {}).get(name)
                if entry and now <= entry["target_ms"]:
                    print(f"::warning::perf-budget: {cls} {name} recorded {now:.3f} ms, "
                          f"under its known-limitation target {entry['target_ms']:g} ms — "
                          "remove the stale limitation in this commit")
        return 0
    rows, failures, unknown, stale = check(budgets, seen)
    # A class nobody has committed a budget for is held to its own last run
    # (`--baseline`, the previous run's `--write-measured` as published to
    # ci-results), at a wider tolerance: so a new machine is gated from its
    # second run on, with nothing to commit by hand. A committed budget,
    # once there, takes over.
    baseline = load_baseline(baseline_path) if baseline_path else {}
    held = {c: baseline[c] for c in unknown if c in baseline}
    if held:
        btol = budgets.get("baseline_tolerance_percent", DEFAULT_BASELINE_TOLERANCE)
        last = dict(budgets, tolerance_percent=btol, classes=held,
                    known_limitations={})
        brows, bfail, _, _ = check(last, {c: seen[c] for c in held})
        rows += [(c, n, w, now, ch, f"{v} (vs last run, ±{btol:g}%)") for c, n, w, now, ch, v in brows]
        failures += bfail
        unknown = [c for c in unknown if c not in held]
    tol = budgets["tolerance_percent"]
    print(f"| class | metric | budget | now | change | verdict (tolerance ±{tol:g}%) |")
    print("|---|---|---|---|---|---|")
    for cls, name, was, now, change, verdict in rows:
        w = "–" if was is None else f"{was:.3f}"
        c = "–" if change is None else f"{change:+.1f}%"
        print(f"| {cls} | {name} | {w} | {now:.3f} | {c} | {verdict} |")
    for cls in unknown:
        print(f"perf-budget: no budget and no previous run for class {cls!r}: not gated "
              f"this time{' (this run is the baseline for the next)' if measured_path else ''}")
    for cls, name, target, now in stale:
        print(f"::warning::perf-budget: {cls} {name}: {now:.3f} ms is under its "
              f"known-limitation target {target:g} ms — the limitation is stale, "
              "remove it from perf-budgets.json in this commit")
    crows, cfail = check_ceilings(budgets, seen, tier_of)
    if crows:
        print()
        print("| class | metric | ceiling | now | tier | verdict |")
        print("|---|---|---|---|---|---|")
        for cls, name, ceiling, now, tier, verdict in crows:
            print(f"| {cls} | {name} | {ceiling:g} | {now:.1f} | {tier} | {verdict} |")
    if cfail:
        level = "warning" if advisory else "error"
        for cls, name, ceiling, now, tier in cfail:
            print(f"::{level}::perf-budget: {cls} ({tier} tier) {name}: {now:.1f} ms is over "
                  f"its {ceiling:g} ms ceiling")
    if failures or cfail:
        level = "warning" if advisory else "error"
        for cls, name, was, now, change in failures:
            print(f"::{level}::perf-budget: {cls} {name}: {now:.3f} against {was:.3f} "
                  f"({change:+.1f}%)")
        if advisory:
            print("perf-budget: over budget, reported only (--advisory: a shared hosted runner)")
            return 0
        return 1
    print("perf-budget: within budget" if rows or crows else "perf-budget: nothing gated")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
