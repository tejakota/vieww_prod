"""Turn an entry point's Run workflow form into what each part runs.

    python3 .github/entry/plan.py cloud|device

Reads the form from $INPUTS (the entry's `toJSON(inputs)`), writes one
output per part to $GITHUB_OUTPUT and the selection to the run's summary.

The form: a checkbox per item, plus `steps`, which narrows or tunes items:

    audio:fuzz test; release_gate:windows-cpu rerun=all; wright_installers:version=1.2.0

Items are separated by `;`. After `item:` come step names (space-separated)
and `key=value` options. An item named in `steps` runs even if its box is
not ticked. Anything unknown fails the plan, naming what is valid, so a typo
never turns into a run that silently checks nothing.
"""
import json
import os
import sys

# ── the catalogue ────────────────────────────────────────────────────────
# item: (group, steps, options {name: default})
CLOUD = {
    "quick": ("machines", ["quick", "workflows", "wright-sandbox"], {}),
    "framework": ("machines", ["gate", "coverage", "cross", "gpu-lavapipe", "macos", "windows", "web",
                               "audio-fuzz", "a11y-gallery", "plugin-compat"], {}),
    "studio": ("machines", ["suite"], {}),
    "wright_sandbox": ("machines", ["linux-escape-suite", "macos-seatbelt-suite", "windows-fail-closed",
                                    "windows-doctor-fix"], {"profile": "debug"}),
    "wright_models": ("machines", ["reference", "platforms"], {}),
    "wright_installers": ("machines", ["build", "release", "update-channel-tests"],
                          {"version": "1.0.0", "publish": "false", "prerelease": "true"}),
    "audio": ("machines", ["test", "fuzz", "linux-servers", "linux-alsa-loopback", "windows-loopback",
                           "macos-loopback", "desktop", "targets", "plugins"], {}),
    "release_gate": ("machines", ["windows-cpu", "macos-cpu", "macos-gpu", "linux-export"],
                     {"scope": "gate", "rerun": "failed", "package": "true"}),
    "supply_chain": ("machines", ["advisories", "sbom", "repro"], {}),
    "goldens": ("machines", ["shots"], {"update": "false"}),
    "publish_crates": ("machines", ["check", "verify", "scaffold-external", "publish"], {}),
    "mobile_apps": ("mobile", ["android", "ios"], {}),
    "u_apk": ("mobile", ["apk"], {}),
    "u_models": ("mobile", ["model"], {}),
    "mobile_audio": ("mobile", ["logic", "android", "ios"], {}),
    "framework_mobile": ("mobile", ["android", "ios-simulator", "android-emulator"], {}),
}

DEVICE = {
    "feature": ("machines", [], {"areas": "auto", "depth": "quick"}),
    "hardware_linux": ("machines", "env gpu nvidia gallery baseline size stress desktop audio wright-docker".split(),
                       {"launches": "5"}),
    "hardware_windows": ("machines", "env gpu d3d12 gallery baseline size pipelines stress desktop remote audio".split(),
                         {"launches": "5"}),
    "hardware_macos": ("machines", "env gpu moltenvk gallery baseline size stress desktop audio".split(),
                       {"launches": "5"}),
    "release_gate_device": ("machines", ["linux-cpu", "linux-gpu", "windows-gpu"],
                            {"scope": "gate", "rerun": "failed", "package": "true"}),
    "phone_farm": ("mobile", ["apk", "phones", "ipa", "iphones"],
                   {"platform": "android", "profile": "dist", "devices": "", "count": "3", "video": "false"}),
    "android_phone": ("mobile", [], {}),
}

CHOICES = {
    "depth": {"quick", "full"}, "scope": {"gate", "checks", "screenshots"}, "rerun": {"failed", "all"},
    "profile": None, "platform": {"android", "ios", "both"},
    "publish": {"true", "false"}, "prerelease": {"true", "false"}, "package": {"true", "false"},
    "update": {"true", "false"}, "video": {"true", "false"},
}
# `profile` is per item: the sandbox takes debug|release, the farm dist|release.
PROFILE = {"wright_sandbox": {"debug", "release"}, "phone_farm": {"dist", "release"}}


def fail(msg):
    print(f"::error::{msg}", file=sys.stderr)
    sys.exit(1)


def parse_steps(text, catalogue):
    picked = {}
    for chunk in (c.strip() for c in (text or "").replace("\n", ";").split(";")):
        if not chunk:
            continue
        item, _, rest = chunk.partition(":")
        item = item.strip()
        if item not in catalogue:
            fail(f"steps: unknown item '{item}'. Items: {', '.join(catalogue)}")
        _, steps, options = catalogue[item]
        sel, opts = picked.setdefault(item, ([], {}))
        for word in rest.split():
            if "=" in word:
                k, v = word.split("=", 1)
                if k not in options:
                    fail(f"steps: '{item}' has no option '{k}'. Options: {', '.join(options) or 'none'}")
                allowed = PROFILE.get(item) if k == "profile" else CHOICES.get(k)
                if allowed and v not in allowed:
                    fail(f"steps: {item} {k}={v}: use one of {', '.join(sorted(allowed))}")
                opts[k] = v
            else:
                if word not in steps:
                    fail(f"steps: '{item}' has no step '{word}'. Steps: {', '.join(steps) or 'none (options only)'}")
                sel.append(word)
    return picked


def main():
    side = sys.argv[1]
    catalogue = CLOUD if side == "cloud" else DEVICE
    form = json.loads(os.environ.get("INPUTS") or "{}")
    picked = parse_steps(form.get("steps", ""), catalogue)

    out, rows = {}, []
    for item, (group, steps, options) in catalogue.items():
        on = bool(form.get(item)) or item in picked
        sel, opts = picked.get(item, ([], {}))
        out[item] = "true" if on else "false"
        out[f"{item}_only"] = " ".join(sel)
        for k, d in options.items():
            out[f"{item}_{k}"] = opts.get(k, d)
        if on:
            detail = " ".join(sel) or "all steps"
            if opts:
                detail += " · " + " ".join(f"{k}={v}" for k, v in opts.items())
            rows.append((group, item, detail))

    # Parts several items share.
    if side == "cloud":
        legs = []
        for item in ("framework", "framework_mobile"):
            if out[item] == "true":
                legs += picked.get(item, ([], {}))[0] or catalogue[item][1]
        out["framework_any"] = "true" if legs else "false"
        out["framework_legs"] = ",".join(legs)
        out["release_gate_legs"] = ",".join(picked.get("release_gate", ([], {}))[0]) or "hosted"
    else:
        out["framework_any"] = out["android_phone"]
        out["framework_legs"] = "android-device"
        out["release_gate_device_legs"] = ",".join(catalogue["release_gate_device"][1]
                                                   if not picked.get("release_gate_device", ([], {}))[0]
                                                   else picked["release_gate_device"][0])
        hw = [i for i in ("hardware_linux", "hardware_windows", "hardware_macos") if out[i] == "true"]
        out["hardware_any"] = "true" if hw else "false"
        for i in ("hardware_linux", "hardware_windows", "hardware_macos"):
            words = picked.get(i, ([], {}))[0] or catalogue[i][1]
            out[f"{i}_steps"] = (",".join if i == "hardware_windows" else " ".join)(words)
        out["hardware_launches"] = next((out[f"{i}_launches"] for i in hw if i in picked), "5")

    out["save_results"] = "true" if form.get("save_results") else "false"
    if not rows:
        fail("Nothing is ticked. Tick at least one item (or name one in `steps`).")

    with open(os.environ["GITHUB_OUTPUT"], "a") as f:
        for k, v in out.items():
            f.write(f"{k}={v}\n")
    title = "Cloud VM" if side == "cloud" else "Device"
    with open(os.environ.get("GITHUB_STEP_SUMMARY", os.devnull), "a") as f:
        f.write(f"### {title} · what runs\n\n| group | item | steps |\n|---|---|---|\n")
        for g, i, d in sorted(rows):
            f.write(f"| {g.title()} | `{i}` | {d} |\n")
        f.write(f"\nResults saved to `ci-results`: {out['save_results']}\n")


if __name__ == "__main__":
    main()
