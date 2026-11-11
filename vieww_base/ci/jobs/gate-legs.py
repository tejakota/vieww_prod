"""The release gate's legs: which machines sign off, and what each runs.

Moved from the release gate's plan job. `ci/projects plan` runs this to
expand the `gate` job of vieww_base/ci.toml into one job per leg: it reads
`matrix=` from stdout. Inputs, as environment variables: EVENT, REF,
LEGS (hosted | all | a comma list of leg ids), RERUN (failed | all) and
SCOPE_IN (gate | checks | screenshots).
"""
import json, os

# ── LEGS ────────────────────────────────────────────────────────
# One entry per machine the sign-off needs.
#
#   hosted     false = a self-hosted runner with these labels, so it
#              is opt-in (`legs: all`, or named).
#   headless   true  = no display and no GPU: the desktop suite and
#              the GPU stages record SKIPPED rather than pretending.
#   rows       what `rerun: failed` executes. "" = nothing
#              outstanding, so the leg is skipped on a rerun.
#              "*" = this leg has never passed anything; run it whole.
LEGS = [
    # Linux, on the machine with a real GPU and a real display. The
    # only leg that can answer G1.4, G1.5, G1.10 and G1.14 with
    # hardware rather than a skip.
    {
        "id": "linux-cpu",
        "os": "linux",
        "runner": ["self-hosted", "linux", "vieww-gate"],
        "mode": "cpu",
        "hosted": False,
        "headless": False,
        "rows": "G0.5,G1.1,G1.3,G3.1",
    },
    {
        "id": "linux-gpu",
        "os": "linux",
        "runner": ["self-hosted", "linux", "vieww-gate", "gpu"],
        "mode": "gpu",
        "hosted": False,
        "headless": False,
        "vk_device": "nvidia",
        "rows": "*",
    },
    # Outstanding as of the 2026-09-18 run: the studio's tests
    # spawning `/bin/sh` (G1.1, and G1.3 through the same suite) and
    # the MSI (G3.1, with G8.1 behind it). Everything else on this
    # machine has passed.
    {
        "id": "windows-cpu",
        "os": "windows",
        "runner": "windows-latest",
        "mode": "cpu",
        "hosted": True,
        "headless": True,
        "rows": "G1.1,G1.3,G3.1",
    },
    # Windows on real hardware: D3D12 and the desktop suite, which
    # the hosted image cannot run. Opt-in like the Linux legs.
    {
        "id": "windows-gpu",
        "os": "windows",
        "runner": ["self-hosted", "windows", "vieww-gate", "gpu"],
        "mode": "gpu",
        "hosted": False,
        "headless": False,
        "rows": "*",
    },
    {
        "id": "macos-cpu",
        "os": "macos",
        "runner": "macos-latest",
        "mode": "cpu",
        "hosted": True,
        "headless": True,
        "rows": "",
    },
    # Apple's paravirtualized GPU through MoltenVK — the only test of
    # the Vulkan parity suite and census on that path. Its pixels are
    # evidence; its timings are not.
    {
        "id": "macos-gpu",
        "os": "macos",
        "runner": "macos-latest",
        "mode": "gpu",
        "hosted": True,
        "headless": True,
        "rows": "",
    },
    # The hosted Ubuntu image carries the Android SDK, NDK and JDK a
    # laptop may not, so the export route is proved here. G2.7-G2.11
    # passed on 2026-09-18, so the list is empty and reruns skip it;
    # `rerun: all` and the tag still run it, and `full_rows` is what
    # it runs then.
    {
        "id": "linux-export",
        "os": "linux",
        "runner": "ubuntu-latest",
        "mode": "cpu",
        "hosted": True,
        "headless": True,
        "export_only": True,
        "rows": "",
        "full_rows": "G2.7,G2.8,G2.9,G2.10,G2.11",
    },
]

event = os.environ["EVENT"]
ref = os.environ["REF"]
tag = ref.startswith("refs/tags/")
wanted = (os.environ.get("LEGS") or "").strip().lower()
rerun = (os.environ.get("RERUN") or "").strip() or ("failed" if event == "push" and not tag else "all")
# A tag is the certification: everything, on every machine that is
# available, whatever the lists say.
if tag:
    rerun = "all"

if not wanted:
    wanted = "hosted"

# `scope: screenshots` takes ten pictures and runs no gate row at
# all — minutes rather than hours, for the question "does this
# machine draw the product correctly", which is what sent somebody
# looking at a macOS build with no text on its buttons. Only the CPU
# legs draw them (a GPU leg's pictures would be the same ones), so
# the rest are not started.
scope = (os.environ.get("SCOPE_IN") or "").strip() or "gate"
if os.environ["EVENT"] == "push":
    scope = "gate"

chosen, skipped = [], []
for leg in LEGS:
    if scope == "screenshots" and (leg["mode"] != "cpu" or leg.get("export_only")):
        skipped.append((leg["id"], "draws no screenshots"))
        continue
    if wanted == "none":
        skipped.append((leg["id"], "legs: none"))
        continue
    if wanted == "hosted" and not leg["hosted"]:
        skipped.append((leg["id"], "self-hosted; pass legs: all or name it"))
        continue
    if wanted not in ("hosted", "all"):
        named = [x.strip() for x in wanted.split(",") if x.strip()]
        if leg["id"] not in named:
            skipped.append((leg["id"], "not named in legs"))
            continue

    rows = leg.get("rows", "*")
    only = ""
    if scope == "screenshots":
        chosen.append({**leg, "only": "", "export_only": False,
                       "vk_device": "", "headless": "1" if leg.get("headless") else ""})
        continue
    # An outstanding list is also what a leg runs on a rerun, so
    # `linux-cpu`'s list below is this laptop's, not a CI machine's.
    if rerun == "failed":
        if rows == "":
            skipped.append((leg["id"], "nothing outstanding (rows is empty)"))
            continue
        if rows != "*":
            only = rows
    # An export-only leg never runs anything but its own rows, even
    # in a full run: the rest of the gate is another leg's job, and a
    # full pass here would build the whole workspace twice.
    if leg.get("export_only"):
        rows_full = leg.get("full_rows") or leg.get("rows")
        if rows_full not in ("", "*"):
            only = rows_full

    entry = dict(leg)
    entry["only"] = only
    entry["export_only"] = bool(leg.get("export_only"))
    entry["vk_device"] = leg.get("vk_device", "")
    entry["headless"] = "1" if leg.get("headless") else ""
    chosen.append(entry)

matrix = {"include": chosen}
print("matrix=" + json.dumps(matrix))
print("any=" + ("true" if chosen else "false"))

summary = ["## Plan", "",
           f"event `{event}`, ref `{ref}`, scope `{scope}`, legs `{wanted}`, rerun `{rerun}`", ""]
for leg in chosen:
    rows = leg["only"] or ("screenshots only" if scope == "screenshots" else "the whole gate")
    summary.append(f"- **{leg['id']}** — {rows}")
for name, why in skipped:
    summary.append(f"- ~~{name}~~ — {why}")
with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as out:
    out.write("\n".join(summary) + "\n")
