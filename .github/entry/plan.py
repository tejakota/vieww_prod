"""The four Run workflow forms, and what each choice on them runs.

    python3 .github/entry/plan.py <form>      form: cloud-machines | cloud-mobile |
                                                    device-machines | device-mobile

At run time it reads the form ($INPUTS, the entry's `toJSON(inputs)`) and
writes one output per part to $GITHUB_OUTPUT, with the selection on the
run's summary. The same tables generate the entry files (entries.py), so a
form and what it runs cannot drift apart.

Everything on a form is a checkbox or a dropdown. The one text field,
`advanced`, is optional and only for what the boxes do not offer, in the
form `item:step step key=value; item2:...` (an unknown item, step or option
fails the run, naming the valid ones).
"""
import json
import os
import sys

# ── items: what can run, its steps (job or leg ids) and options ──────────
ITEMS = {
    # Cloud VM · Machines
    "quick": (["quick", "workflows", "wright-sandbox"], {}),
    "framework": (["gate", "coverage", "cross", "gpu-lavapipe", "macos", "windows", "web",
                   "audio-fuzz", "a11y-gallery", "plugin-compat"], {}),
    "studio": (["suite"], {}),
    "wright_sandbox": (["linux-escape-suite", "macos-seatbelt-suite", "windows-fail-closed",
                        "windows-doctor-fix"], {"profile": "debug"}),
    "wright_models": (["reference", "platforms"], {}),
    "wright_installers": (["build", "release", "update-channel-tests"],
                          {"version": "1.0.0", "publish": "false", "prerelease": "true"}),
    "audio": (["test", "fuzz", "linux-servers", "linux-alsa-loopback", "windows-loopback",
               "macos-loopback", "desktop", "targets", "plugins"], {}),
    "release_gate": (["windows-cpu", "macos-cpu", "macos-gpu", "linux-export"],
                     {"scope": "gate", "rerun": "failed", "package": "true"}),
    "supply_chain": (["advisories", "sbom", "repro"], {}),
    "goldens": (["shots"], {"update": "false"}),
    "publish_crates": (["check", "verify", "scaffold-external", "publish"], {}),
    # Cloud VM · Mobile
    "mobile_apps": (["android", "ios"], {}),
    "u_apk": (["apk"], {}),
    "u_models": (["model"], {}),
    "mobile_audio": (["logic", "android", "ios"], {}),
    "framework_mobile": (["android", "ios-simulator", "android-emulator"], {}),
    # Device · Machines
    "feature": ([], {"areas": "auto", "depth": "quick"}),
    "hardware_linux": ("env gpu nvidia gallery baseline size stress desktop audio wright-docker".split(), {"launches": "5"}),
    "hardware_windows": ("env gpu d3d12 gallery baseline size pipelines stress desktop remote audio".split(), {"launches": "5"}),
    "hardware_macos": ("env gpu moltenvk gallery baseline size stress desktop audio".split(), {"launches": "5"}),
    "release_gate_device": (["linux-cpu", "linux-gpu", "windows-gpu"],
                            {"scope": "gate", "rerun": "failed", "package": "true"}),
    # Device · Mobile
    "phone_farm": (["apk", "phones", "ipa", "iphones"],
                   {"platform": "android", "profile": "dist", "devices": "", "count": "3", "video": "false",
                    "tiers": "low,mid,high"}),
    "android_phone": ([], {}),
}

ALLOWED = {
    ("wright_sandbox", "profile"): {"debug", "release"},
    ("phone_farm", "profile"): {"dist", "release"},
    ("phone_farm", "platform"): {"android", "ios", "both"},
    ("feature", "depth"): {"quick", "full"},
}
for item in ("release_gate", "release_gate_device"):
    ALLOWED[(item, "scope")] = {"gate", "checks", "screenshots"}
    ALLOWED[(item, "rerun")] = {"failed", "all"}
for key in [("wright_installers", "publish"), ("wright_installers", "prerelease"), ("goldens", "update"),
            ("release_gate", "package"), ("release_gate_device", "package"), ("phone_farm", "video")]:
    ALLOWED[key] = {"true", "false"}

# Presets a dropdown stands for.
FRAMEWORK_SCOPE = {
    "all": ITEMS["framework"][0],
    "core (gate, coverage, cross)": ["gate", "coverage", "cross"],
    "gpu (lavapipe)": ["gpu-lavapipe"],
    "platforms (macOS, Windows, web)": ["macos", "windows", "web"],
    "extras (audio fuzz, a11y, plugins)": ["audio-fuzz", "a11y-gallery", "plugin-compat"],
}
AUDIO_SCOPE = {
    "all": [],
    "ci (tests, fuzz)": ["test", "fuzz"],
    "backends (servers, loopback, targets)": ["linux-servers", "linux-alsa-loopback", "windows-loopback",
                                              "macos-loopback", "desktop", "targets"],
    "plugins (CLAP, VST3, LV2)": ["plugins"],
}
INSTALLER_RELEASE = {
    "build only": {"publish": "false", "prerelease": "true"},
    "publish as pre-release": {"publish": "true", "prerelease": "true"},
    "publish as release": {"publish": "true", "prerelease": "false"},
}
HARDWARE_SCOPE = {  # per OS; "all" = every step
    "all": {"linux": None, "windows": None, "macos": None},
    "gpu": {"linux": "env gpu nvidia", "windows": "env gpu d3d12", "macos": "env gpu moltenvk"},
    "visual (gallery, baseline)": {o: "env gallery baseline" for o in ("linux", "windows", "macos")},
    "performance (size, stress)": {"linux": "env size stress", "windows": "env size pipelines stress",
                                   "macos": "env size stress"},
    "desktop (windows, audio)": {"linux": "env desktop audio", "windows": "env desktop remote audio",
                                 "macos": "env desktop audio"},
}
FEATURE_AREAS = {"auto (from the branch's changes)": "auto", "framework": "framework", "wright": "wright",
                 "studio": "studio", "prototype": "prototype", "all": "framework,wright,studio,prototype"}


def B(name, desc, default=False):
    return {"name": name, "type": "boolean", "description": desc, "default": default}


def C(name, desc, options, default=None):
    return {"name": name, "type": "choice", "description": desc, "options": list(options),
            "default": default or list(options)[0]}


def S(name, desc, default=""):
    return {"name": name, "type": "string", "description": desc, "default": default}


ADVANCED = S("advanced", "Optional, for what the boxes don't offer: item:step key=value; … (see .github/workflows/README.md)")
SAVE = B("save_results", "Save this run's results to the ci-results branch", True)

FORMS = {
    "cloud-machines": {
        "title": "Cloud VM · Machines",
        "what": "GitHub's hosted Linux, macOS and Windows machines",
        "items": ["quick", "framework", "studio", "wright_sandbox", "wright_models", "wright_installers",
                  "audio", "release_gate", "supply_chain", "goldens", "publish_crates"],
        "fields": [
            B("quick", "Quick checks: fmt, clippy, workflow lint, Wright sandbox smoke"),
            B("framework", "Framework checks"),
            C("framework_scope", "  └ which framework checks", FRAMEWORK_SCOPE),
            B("studio", "Studio suite (Linux, macOS, Windows)"),
            B("wright_sandbox", "Wright sandbox suites"),
            C("wright_sandbox_profile", "  └ build profile", ["debug", "release"]),
            B("wright_models", "Wright built-in models"),
            B("wright_installers", "Wright desktop installers"),
            S("wright_installers_version", "  └ version to stamp (no leading v)", "1.0.0"),
            C("wright_installers_release", "  └ build only, or publish", INSTALLER_RELEASE),
            B("audio", "Audio"),
            C("audio_scope", "  └ which audio", AUDIO_SCOPE),
            B("release_gate", "Release gate, hosted legs (Windows, macOS, Linux export)"),
            C("release_gate_scope", "  └ scope", ["gate", "checks", "screenshots"]),
            C("release_gate_rerun", "  └ rerun", ["failed", "all"]),
            B("release_gate_package", "  └ build installers", True),
            B("supply_chain", "Supply chain: advisories, SBOM, reproducible build"),
            B("goldens", "Golden screenshots"),
            B("goldens_update", "  └ capture a new baseline instead of comparing"),
            B("publish_crates", "Crates: check, verify (publishes only on a v* tag)"),
            ADVANCED, SAVE,
        ],
    },
    "cloud-mobile": {
        "title": "Cloud VM · Mobile",
        "what": "GitHub's hosted machines, building and testing for phones",
        "items": ["mobile_apps", "u_apk", "u_models", "mobile_audio", "framework_mobile"],
        "fields": [
            B("mobile_apps", "Build the mobile apps"),
            C("mobile_apps_platform", "  └ platform", ["both", "android", "ios"]),
            B("u_apk", "U APK"),
            B("u_models", "U model runtime"),
            B("mobile_audio", "Mobile audio"),
            C("mobile_audio_scope", "  └ which", ["all", "logic", "android", "ios"]),
            B("framework_android", "Framework: Android build"),
            B("framework_android_emulator", "Framework: Android emulator"),
            B("framework_ios_simulator", "Framework: iOS simulator"),
            ADVANCED, SAVE,
        ],
    },
    "device-machines": {
        "title": "Device · Machines",
        "what": "your own Linux, Windows and macOS machines",
        "items": ["feature", "hardware_linux", "hardware_windows", "hardware_macos", "release_gate_device"],
        "fields": [
            B("feature", "Feature run: the checks for what this branch changed"),
            C("feature_areas", "  └ areas", FEATURE_AREAS),
            C("feature_depth", "  └ depth", ["quick", "full"]),
            B("hardware_linux", "Hardware: Linux"),
            B("hardware_windows", "Hardware: Windows"),
            B("hardware_macos", "Hardware: macOS"),
            C("hardware_scope", "  └ which hardware steps", HARDWARE_SCOPE),
            C("hardware_launches", "  └ launches per timing", ["5", "3", "1", "10"]),
            B("release_gate_device", "Release gate on your machines"),
            B("release_gate_linux_cpu", "  └ Linux CPU", True),
            B("release_gate_linux_gpu", "  └ Linux GPU", True),
            B("release_gate_windows_gpu", "  └ Windows GPU", True),
            C("release_gate_device_scope", "  └ scope", ["gate", "checks", "screenshots"]),
            C("release_gate_device_rerun", "  └ rerun", ["failed", "all"]),
            B("release_gate_device_package", "  └ build installers", True),
            ADVANCED, SAVE,
        ],
    },
    "device-mobile": {
        "title": "Device · Mobile",
        "what": "real phones: the phone farm and your own Android phone",
        "items": ["phone_farm", "android_phone"],
        "fields": [
            B("phone_farm", "Phone farm: run on real phones"),
            B("phone_low", "  └ low-end phone", True),
            B("phone_mid", "  └ mid-range phone", True),
            B("phone_high", "  └ high-end phone", True),
            C("phone_platform", "  └ platform", ["android", "ios", "both"]),
            C("phone_count", "  └ phones spread from cheapest to most capable (before tiers)", ["3", "5", "8"]),
            C("phone_profile", "  └ APK build profile", ["dist", "release"]),
            B("phone_video", "  └ record video of each phone"),
            B("android_phone", "Your Android phone (USB): the device suite"),
            ADVANCED, SAVE,
        ],
    },
}


def fail(msg):
    print(f"::error::{msg}", file=sys.stderr)
    sys.exit(1)


def from_form(form, f):
    """item -> [on, steps, opts] from the boxes and dropdowns."""
    p = {i: [bool(f.get(i)), [], {}] for i in FORMS[form]["items"]}
    if form == "cloud-machines":
        p["framework"][1] = FRAMEWORK_SCOPE.get(f.get("framework_scope", "all"), [])
        p["wright_sandbox"][2]["profile"] = f.get("wright_sandbox_profile", "debug")
        p["wright_installers"][2]["version"] = (f.get("wright_installers_version") or "1.0.0").lstrip("v")
        p["wright_installers"][2].update(INSTALLER_RELEASE.get(f.get("wright_installers_release", "build only"), {}))
        p["audio"][1] = list(AUDIO_SCOPE.get(f.get("audio_scope", "all"), []))
        g = p["release_gate"][2]
        g["scope"], g["rerun"] = f.get("release_gate_scope", "gate"), f.get("release_gate_rerun", "failed")
        g["package"] = "true" if f.get("release_gate_package", True) else "false"
        p["goldens"][2]["update"] = "true" if f.get("goldens_update") else "false"
    elif form == "cloud-mobile":
        plat = f.get("mobile_apps_platform", "both")
        p["mobile_apps"][1] = [] if plat == "both" else [plat]
        sc = f.get("mobile_audio_scope", "all")
        p["mobile_audio"][1] = [] if sc == "all" else [sc]
        legs = [leg for box, leg in [("framework_android", "android"), ("framework_android_emulator", "android-emulator"),
                                     ("framework_ios_simulator", "ios-simulator")] if f.get(box)]
        p["framework_mobile"] = [bool(legs), legs, {}]
    elif form == "device-machines":
        p["feature"][2] = {"areas": FEATURE_AREAS.get(f.get("feature_areas", ""), "auto"), "depth": f.get("feature_depth", "quick")}
        scope = HARDWARE_SCOPE.get(f.get("hardware_scope", "all"), HARDWARE_SCOPE["all"])
        for os_ in ("linux", "windows", "macos"):
            words = scope[os_]
            p[f"hardware_{os_}"][1] = words.split() if words else []
            p[f"hardware_{os_}"][2]["launches"] = f.get("hardware_launches", "5")
        legs = [leg for box, leg in [("release_gate_linux_cpu", "linux-cpu"), ("release_gate_linux_gpu", "linux-gpu"),
                                     ("release_gate_windows_gpu", "windows-gpu")] if f.get(box, True)]
        g = p["release_gate_device"]
        g[1] = legs
        g[2] = {"scope": f.get("release_gate_device_scope", "gate"), "rerun": f.get("release_gate_device_rerun", "failed"),
                "package": "true" if f.get("release_gate_device_package", True) else "false"}
        if g[0] and not legs:
            fail("Release gate on your machines is ticked, but none of its legs are.")
    elif form == "device-mobile":
        tiers = [t for t in ("low", "mid", "high") if f.get(f"phone_{t}", True)]
        if p["phone_farm"][0] and not tiers:
            fail("Phone farm is ticked, but no phone tier is: tick low-end, mid-range and/or high-end.")
        p["phone_farm"][2] = {"platform": f.get("phone_platform", "android"), "count": f.get("phone_count", "3"),
                              "profile": f.get("phone_profile", "dist"), "video": "true" if f.get("phone_video") else "false",
                              "tiers": ",".join(tiers) or "low,mid,high"}
    return p


def apply_advanced(form, text, p):
    valid = FORMS[form]["items"]
    for chunk in (c.strip() for c in (text or "").replace("\n", ";").split(";")):
        if not chunk:
            continue
        item, _, rest = chunk.partition(":")
        item = item.strip()
        if item not in valid:
            fail(f"advanced: '{item}' is not on this form. Items: {', '.join(valid)}")
        steps, options = ITEMS[item]
        on, sel, opts = p[item]
        words_given = []
        for word in rest.split():
            if "=" in word:
                k, v = word.split("=", 1)
                if k not in options:
                    fail(f"advanced: '{item}' has no option '{k}'. Options: {', '.join(options) or 'none'}")
                allowed = ALLOWED.get((item, k))
                if allowed and v not in allowed:
                    fail(f"advanced: {item} {k}={v}: use one of {', '.join(sorted(allowed))}")
                opts[k] = v
            else:
                if word not in steps:
                    fail(f"advanced: '{item}' has no step '{word}'. Steps: {', '.join(steps) or 'none'}")
                words_given.append(word)
        p[item] = [True, words_given or sel, opts]
    return p


def plan(form, f):
    p = apply_advanced(form, f.get("advanced", ""), from_form(form, f))
    out, rows = {}, []
    for item in FORMS[form]["items"]:
        on, sel, opts = p[item]
        out[item] = "true" if on else "false"
        out[f"{item}_only"] = " ".join(sel)
        for k, d in ITEMS[item][1].items():
            out[f"{item}_{k}"] = opts.get(k, d)
        if on:
            detail = " ".join(sel) or "all steps"
            if opts:
                detail += " · " + " ".join(f"{k}={v}" for k, v in opts.items())
            rows.append((item, detail))
    if form == "cloud-machines":
        out["framework_any"] = out["framework"]
        out["framework_legs"] = ",".join(p["framework"][1] or ITEMS["framework"][0])
        out["release_gate_legs"] = ",".join(p["release_gate"][1]) or "hosted"
    elif form == "cloud-mobile":
        out["framework_any"] = out["framework_mobile"]
        out["framework_legs"] = ",".join(p["framework_mobile"][1])
    elif form == "device-machines":
        out["release_gate_device_legs"] = ",".join(p["release_gate_device"][1] or ITEMS["release_gate_device"][0])
        hw = [f"hardware_{o}" for o in ("linux", "windows", "macos") if out[f"hardware_{o}"] == "true"]
        out["hardware_any"] = "true" if hw else "false"
        for o in ("linux", "windows", "macos"):
            words = p[f"hardware_{o}"][1] or ITEMS[f"hardware_{o}"][0]
            out[f"hardware_{o}_steps"] = (",".join if o == "windows" else " ".join)(words)
        out["hardware_launches"] = next((out[f"{i}_launches"] for i in hw), "5")
    elif form == "device-mobile":
        out["framework_any"] = out["android_phone"]
        out["framework_legs"] = "android-device"
    out["save_results"] = "true" if f.get("save_results", True) else "false"
    if not rows:
        fail("Nothing is ticked. Tick at least one item.")
    return out, rows


def main():
    form = sys.argv[1]
    if form not in FORMS:
        fail(f"unknown form {form!r}: {', '.join(FORMS)}")
    out, rows = plan(form, json.loads(os.environ.get("INPUTS") or "{}"))
    with open(os.environ["GITHUB_OUTPUT"], "a") as fh:
        for k, v in out.items():
            fh.write(f"{k}={v}\n")
    with open(os.environ.get("GITHUB_STEP_SUMMARY", os.devnull), "a") as fh:
        fh.write(f"### {FORMS[form]['title']} · what runs\n\n| item | steps |\n|---|---|\n")
        for i, d in rows:
            fh.write(f"| `{i}` | {d} |\n")
        fh.write(f"\nResults saved to `ci-results`: {out['save_results']}\n")


if __name__ == "__main__":
    main()
