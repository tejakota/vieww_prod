"""The machines the Market Watch comparison runs its CPU-path bench on.

`ci/projects plan` runs this to expand the `compare-machines` job of
examples/market-watch/ci.toml into one job per machine, reading `matrix=`
from stdout. Inputs, as environment variables:

    REF   the ref being run (refs/heads/main, refs/heads/topic, ...)
    LEGS  which of your own machines were ticked on the form:
          "linux=true windows=false mac=true"

GitHub's machines are every leg every time: the six operating-system and
CPU-architecture pairs it hosts, so a run sees x86-64 and arm64 on Linux,
Windows and macOS. Your own machines (the self-hosted vieww-linux,
vieww-windows and vieww-macos runners, Devices ▸ Linux / Windows / Mac) join
when ticked, and from main only — the rule every hardware job here keeps,
because a self-hosted runner executes whatever the branch says.
"""
import json
import os

HOSTED = [
    {"name": "linux-x64", "runner": "ubuntu-24.04", "os": "linux"},
    {"name": "linux-arm64", "runner": "ubuntu-24.04-arm", "os": "linux"},
    {"name": "windows-x64", "runner": "windows-2025", "os": "windows"},
    {"name": "windows-arm64", "runner": "windows-11-arm", "os": "windows"},
    {"name": "macos-arm64", "runner": "macos-15", "os": "macos"},
    {"name": "macos-x64", "runner": "macos-15-intel", "os": "macos"},
]

MINE = {
    "linux": {"name": "my-linux", "runner": ["self-hosted", "Linux", "X64", "vieww-linux"], "os": "linux"},
    "windows": {"name": "my-windows", "runner": ["self-hosted", "vieww-windows"], "os": "windows"},
    "mac": {"name": "my-mac", "runner": ["self-hosted", "vieww-macos"], "os": "macos"},
}


def legs(ref, ticked):
    rows = [dict(r, hosted=True) for r in HOSTED]
    if ref == "refs/heads/main":
        for key, row in MINE.items():
            if ticked.get(key):
                rows.append(dict(row, hosted=False))
    return rows


def parse_ticked(text):
    out = {}
    for part in text.split():
        key, _, value = part.partition("=")
        out[key] = value.strip().lower() == "true"
    return out


if __name__ == "__main__":
    rows = legs(os.environ.get("REF", ""), parse_ticked(os.environ.get("LEGS", "")))
    print("matrix=" + json.dumps({"include": rows}))
