"""Generate the four entry workflows from plan.py's FORMS.

    python3 .github/entry/entries.py      (from the repository root)

Run it after changing a form in plan.py; CI's `quick` lint checks the
files are current (entries.py --check).
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import plan  # noqa: E402

WF = Path(".github/workflows")
O = "needs.plan.outputs."
READ = {"contents": "read"}
REPORT = {"contents": "write", "actions": "read"}


def v(key, cmp=None):
    return f"${{{{ {O}{key} == 'true' }}}}" if cmp == "bool" else f"${{{{ {O}{key} }}}}"


def call(job, name, on, file, with_=None, perms=READ):
    return {"job": job, "name": name, "on": on, "file": file, "with": with_ or {}, "perms": perms}


PARTS = {
    "cloud-machines": [
        call("quick", "quick checks", "quick", "cloud-machines-quick.yml", {"only": v("quick_only")}),
        call("framework", "framework", "framework_any", "shared-framework-checks.yml",
             {"legs": v("framework_legs"), "device": "false"}, REPORT),
        call("studio", "studio", "studio", "cloud-machines-studio.yml", {"only": v("studio_only")}),
        call("wright_sandbox", "wright sandbox", "wright_sandbox", "cloud-machines-wright-sandbox.yml",
             {"only": v("wright_sandbox_only"), "profile": v("wright_sandbox_profile")}),
        call("wright_models", "wright models", "wright_models", "cloud-machines-wright-models.yml", {"only": v("wright_models_only")}),
        call("wright_installers", "wright installers", "wright_installers", "cloud-machines-wright-installers.yml",
             {"only": v("wright_installers_only"), "version": v("wright_installers_version"),
              "publish": v("wright_installers_publish", "bool"), "prerelease": v("wright_installers_prerelease", "bool")},
             {"contents": "write"}),
        call("audio_ci", "audio CI", "audio", "cloud-machines-audio-ci.yml", {"only": v("audio_only")}),
        call("audio_backends", "audio backends", "audio", "cloud-machines-audio-backends.yml", {"only": v("audio_only")}),
        call("audio_plugins", "audio plugins", "audio", "cloud-machines-audio-plugins.yml", {"only": v("audio_only")}),
        call("release_gate", "release gate (hosted)", "release_gate", "shared-release-gate.yml",
             {"legs": v("release_gate_legs"), "scope": v("release_gate_scope"), "rerun": v("release_gate_rerun"),
              "package": v("release_gate_package", "bool")},
             {"contents": "write", "id-token": "write", "attestations": "write"}),
        call("supply_chain", "supply chain", "supply_chain", "cloud-machines-supply-chain.yml", {"only": v("supply_chain_only")}),
        call("goldens", "goldens", "goldens", "cloud-machines-goldens.yml",
             {"only": v("goldens_only"), "update": v("goldens_update", "bool")}),
        call("publish_crates", "crates", "publish_crates", "cloud-machines-publish.yml", {"only": v("publish_crates_only")}),
    ],
    "cloud-mobile": [
        call("mobile_apps", "mobile apps", "mobile_apps", "cloud-mobile-apps.yml", {"only": v("mobile_apps_only")}),
        call("u_apk", "U APK", "u_apk", "cloud-mobile-u-apk.yml", {"only": v("u_apk_only")}),
        call("u_models", "U models", "u_models", "cloud-mobile-u-models.yml", {"only": v("u_models_only")}),
        call("mobile_audio", "mobile audio", "mobile_audio", "cloud-mobile-audio.yml", {"only": v("mobile_audio_only")}),
        call("framework", "framework (mobile)", "framework_any", "shared-framework-checks.yml",
             {"legs": v("framework_legs"), "device": "false"}, REPORT),
    ],
    "device-machines": [
        call("feature", "feature run", "feature", "device-machines-feature.yml",
             {"areas": v("feature_areas"), "depth": v("feature_depth")}),
        call("hardware", "hardware", "hardware_any", "device-machines-hardware.yml",
             {"linux": v("hardware_linux", "bool"), "windows": v("hardware_windows", "bool"), "macos": v("hardware_macos", "bool"),
              "linux_steps": v("hardware_linux_steps"), "windows_steps": v("hardware_windows_steps"),
              "macos_steps": v("hardware_macos_steps"), "launches": v("hardware_launches")},
             {"contents": "write"}),
        call("release_gate", "release gate (your machines)", "release_gate_device", "shared-release-gate.yml",
             {"legs": v("release_gate_device_legs"), "scope": v("release_gate_device_scope"),
              "rerun": v("release_gate_device_rerun"), "package": v("release_gate_device_package", "bool")},
             {"contents": "write", "id-token": "write", "attestations": "write"}),
    ],
    "device-mobile": [
        call("phone_farm", "phone farm", "phone_farm", "device-mobile-phone-farm.yml",
             {"only": v("phone_farm_only"), "platform": v("phone_farm_platform"), "profile": v("phone_farm_profile"),
              "devices": v("phone_farm_devices"), "count": v("phone_farm_count"), "video": v("phone_farm_video", "bool"),
              "tiers": v("phone_farm_tiers")},
             {"contents": "write", "id-token": "write"}),
        call("android_phone", "your Android phone", "framework_any", "shared-framework-checks.yml",
             {"legs": "android-device", "device": "true"}, REPORT),
    ],
}


def q(s):
    return json.dumps(str(s), ensure_ascii=False)


def render(form):
    spec = plan.FORMS[form]
    title = spec["title"]
    others = ", ".join(plan.FORMS[f]["title"] for f in plan.FORMS if f != form)
    L = [
        "# GENERATED by .github/entry/entries.py from .github/entry/plan.py: edit those, not this.",
        "#",
        f"# {title}: {spec['what']}.",
        f"# The other forms: {others}. Nothing in this repository runs any other",
        "# way: not on a push, a pull request, a schedule or another run.",
        "#",
        "# Tick what to run; the `└` fields under an item choose its steps and",
        "# options. The run's summary starts with exactly what was chosen.",
        "",
        f"name: {q(title)}",
        "",
        "on:",
        "  workflow_dispatch:",
        "    inputs:",
    ]
    for f in spec["fields"]:
        L += [f"      {f['name']}:", f"        description: {q(f['description'])}", f"        type: {f['type']}"]
        if f["type"] == "choice":
            L.append("        options:")
            L += [f"          - {q(o)}" for o in f["options"]]
            L.append(f"        default: {q(f['default'])}")
        elif f["type"] == "boolean":
            L.append(f"        default: {'true' if f['default'] else 'false'}")
        else:
            L.append(f"        default: {q(f['default'])}")
    parts = PARTS[form]
    keys = {p["on"] for p in parts} | {"save_results"}
    for p in parts:
        for val in p["with"].values():
            if O in val:
                keys.add(val.split(O, 1)[1].split()[0])
    L += ["", "permissions:", "  contents: read", "", "jobs:",
          "  plan:", "    name: plan", "    runs-on: ubuntu-latest", "    outputs:"]
    L += [f"      {k}: ${{{{ steps.plan.outputs.{k} }}}}" for k in sorted(keys)]
    L += ["    steps:", "      - uses: actions/checkout@v4", "        with:", "          persist-credentials: false",
          "          sparse-checkout: .github/entry", "      - id: plan", "        env:",
          "          INPUTS: ${{ toJSON(inputs) }}", f"        run: python3 .github/entry/plan.py {form}", ""]
    for p in parts:
        L += [f"  {p['job']}:", f"    name: {q(p['name'])}", "    needs: plan",
              f"    if: {O}{p['on']} == 'true'", f"    uses: ./.github/workflows/{p['file']}"]
        if p["with"]:
            L.append("    with:")
            L += [f"      {k}: {val}" for k, val in p["with"].items()]
        L.append("    secrets: inherit")
        if p["perms"] != READ:
            L.append("    permissions:")
            L += [f"      {k}: {val}" for k, val in p["perms"].items()]
        L.append("")
    L += ["  results:", "    name: save results", f"    needs: [plan, {', '.join(p['job'] for p in parts)}]",
          f"    if: always() && needs.plan.result == 'success' && {O}save_results == 'true'",
          "    uses: ./.github/workflows/shared-results.yml", "    with:", "      run_id: ${{ github.run_id }}",
          "      workflow: ${{ github.workflow }}",
          "      conclusion: ${{ contains(needs.*.result, 'failure') && 'failure' || (contains(needs.*.result, 'cancelled') && 'cancelled' || 'success') }}",
          "    permissions:", "      contents: write", "      actions: read", ""]
    return "\n".join(L)


def main():
    check = "--check" in sys.argv
    stale = []
    for form in plan.FORMS:
        path = WF / f"entry-{form}.yml"
        text = render(form)
        if len([f for f in plan.FORMS[form]["fields"]]) > 25:
            sys.exit(f"{path}: more than 25 fields (GitHub's limit)")
        if check:
            if not path.exists() or path.read_text() != text:
                stale.append(str(path))
        else:
            path.write_text(text)
    if stale:
        sys.exit("stale (run python3 .github/entry/entries.py): " + ", ".join(stale))


if __name__ == "__main__":
    main()
