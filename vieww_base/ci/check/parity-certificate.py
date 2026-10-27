#!/usr/bin/env python3
"""Build the parity certificate (FW-26) from GPU suite rows.

Usage: parity-certificate.py <rows-dir> [<rows-dir> ...] -o parity-certificate.json [--markdown table.md]

Every GPU scene suite (Vulkan, D3D12, Metal) appends one JSON line per
comparison against the CPU oracle to <rows-dir>/<backend>.jsonl when
VIEWW_PARITY_OUT is set (crates/vieww-hal/tests/support/parity_row.rs).
This script merges them into one certificate: per backend, the contract,
the tests measured, interior violations and the worst channel delta,
with the commit and runner it came from. It exits 1 if any row broke its
contract, so a CI leg that collects rows also enforces them.
"""
import argparse
import datetime
import json
import os
import pathlib
import subprocess
import sys


def commit():
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    except Exception:
        return os.environ.get("GITHUB_SHA", "unknown")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--markdown")
    a = ap.parse_args()
    rows = []
    for d in a.dirs:
        for f in sorted(pathlib.Path(d).glob("*.jsonl")):
            for line in f.read_text().splitlines():
                if line.strip():
                    rows.append(json.loads(line))
    if not rows:
        # The suites themselves decide pass/fail; with no rows (every test
        # skipped for want of a device) there is nothing to certify.
        print("::warning::parity-certificate: no rows (the GPU suites skipped, or ran without VIEWW_PARITY_OUT); no certificate written")
        return 0
    backends = {}
    for r in rows:
        b = backends.setdefault(r["backend"], {
            "backend": r["backend"],
            "adapters": set(),
            "contract": r["contract"],
            "comparisons": 0,
            "tests": set(),
            "interior_violations": 0,
            "worst_delta": 0,
            "failed_tests": set(),
        })
        b["adapters"].add(r.get("adapter") or "unnamed")
        b["comparisons"] += 1
        b["tests"].add(r["test"])
        b["interior_violations"] += r["interior_violations"]
        b["worst_delta"] = max(b["worst_delta"], r["worst_delta"])
        if r["interior_violations"] > 0:
            b["failed_tests"].add(r["test"])
    out = []
    for b in sorted(backends.values(), key=lambda b: b["backend"]):
        out.append({
            "backend": b["backend"],
            "adapters": sorted(b["adapters"]),
            "contract": b["contract"],
            "comparisons": b["comparisons"],
            "tests": len(b["tests"]),
            "interior_violations": b["interior_violations"],
            "worst_delta_anywhere": b["worst_delta"],
            "holds": not b["failed_tests"],
            "failed_tests": sorted(b["failed_tests"]),
        })
    cert = {
        "format": "vieww.parity-certificate/1",
        "commit": commit(),
        "generated": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "runner": os.environ.get("RUNNER_NAME", ""),
        "oracle": "vieww-paint NativeRenderer (CPU)",
        "backends": out,
    }
    pathlib.Path(a.out).write_text(json.dumps(cert, indent=2) + "\n")
    md = ["| Backend | Adapter(s) | Contract | Tests | Interior violations | Worst delta (edges incl.) | Holds |",
          "|---|---|---|---|---|---|---|"]
    for b in out:
        md.append(f"| {b['backend']} | {', '.join(b['adapters'])} | {b['contract']} | {b['tests']} | "
                  f"{b['interior_violations']} | {b['worst_delta_anywhere']}/255 | {'yes' if b['holds'] else 'NO'} |")
    text = "\n".join(md) + "\n"
    if a.markdown:
        pathlib.Path(a.markdown).write_text(text)
    print(text)
    return 0 if all(b["holds"] for b in out) else 1


if __name__ == "__main__":
    sys.exit(main())
