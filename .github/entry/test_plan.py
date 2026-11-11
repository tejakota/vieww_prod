"""What each choice on the four forms runs, and that the forms match the parts.

    python3 -I .github/entry/test_plan.py
"""
import importlib.util
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
WF = HERE.parent / "workflows"
spec = importlib.util.spec_from_file_location("plan", HERE / "plan.py")
plan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plan)


def run(form, **f):
    out, _ = plan.plan(form, f)
    return out


def fails(form, **f):
    try:
        plan.plan(form, f)
    except SystemExit:
        return True
    return False


def job_ids(file):
    text = (WF / file).read_text().split("\njobs:\n", 1)[1]
    return set(re.findall(r"^  ([A-Za-z0-9_-]+):\s*$", text, re.M)) - plan_exempt


plan_exempt = {"report", "summary", "main-only", "plan", "checklist"}


class Forms(unittest.TestCase):
    def test_every_form_fits_githubs_limit(self):
        for form, spec in plan.FORMS.items():
            self.assertLessEqual(len(spec["fields"]), 25, form)

    def test_nothing_ticked_is_refused(self):
        for form in plan.FORMS:
            self.assertTrue(fails(form), form)


class CloudMachines(unittest.TestCase):
    def test_a_box_runs_its_item_whole(self):
        o = run("cloud-machines", studio=True)
        self.assertEqual((o["studio"], o["studio_only"], o["quick"]), ("true", "", "false"))

    def test_framework_scope_picks_legs(self):
        o = run("cloud-machines", framework=True, framework_scope="gpu (lavapipe)")
        self.assertEqual(o["framework_legs"], "gpu-lavapipe")
        self.assertEqual(run("cloud-machines", framework=True)["framework_legs"].count(","), 9)

    def test_installer_dropdown_sets_publish(self):
        o = run("cloud-machines", wright_installers=True, wright_installers_version="v1.2.0",
                wright_installers_release="publish as release")
        self.assertEqual((o["wright_installers_version"], o["wright_installers_publish"],
                          o["wright_installers_prerelease"]), ("1.2.0", "true", "false"))

    def test_audio_scope_narrows_all_three_parts(self):
        self.assertEqual(run("cloud-machines", audio=True, audio_scope="ci (tests, fuzz)")["audio_only"], "test fuzz")

    def test_release_gate_defaults_to_hosted(self):
        o = run("cloud-machines", release_gate=True)
        self.assertEqual((o["release_gate_legs"], o["release_gate_package"]), ("hosted", "true"))

    def test_advanced_overrides_and_validates(self):
        o = run("cloud-machines", advanced="release_gate:windows-cpu rerun=all")
        self.assertEqual((o["release_gate"], o["release_gate_legs"], o["release_gate_rerun"]), ("true", "windows-cpu", "all"))
        self.assertTrue(fails("cloud-machines", advanced="audio:fuz"))
        self.assertTrue(fails("cloud-machines", advanced="phone_farm:apk"))  # not on this form


class CloudMobile(unittest.TestCase):
    def test_framework_boxes_are_legs(self):
        o = run("cloud-mobile", framework_android_emulator=True, framework_ios_simulator=True)
        self.assertEqual((o["framework_any"], o["framework_legs"]), ("true", "android-emulator,ios-simulator"))

    def test_platform_dropdown(self):
        self.assertEqual(run("cloud-mobile", mobile_apps=True, mobile_apps_platform="ios")["mobile_apps_only"], "ios")
        self.assertEqual(run("cloud-mobile", mobile_apps=True)["mobile_apps_only"], "")


class DeviceMachines(unittest.TestCase):
    def test_hardware_scope_per_os(self):
        o = run("device-machines", hardware_windows=True, hardware_linux=True, hardware_scope="gpu", hardware_launches="3")
        self.assertEqual((o["hardware_any"], o["hardware_linux_steps"], o["hardware_windows_steps"], o["hardware_launches"]),
                         ("true", "env gpu nvidia", "env,gpu,d3d12", "3"))

    def test_release_gate_legs_are_boxes(self):
        o = run("device-machines", release_gate_device=True, release_gate_linux_gpu=False)
        self.assertEqual(o["release_gate_device_legs"], "linux-cpu,windows-gpu")
        self.assertTrue(fails("device-machines", release_gate_device=True, release_gate_linux_cpu=False,
                              release_gate_linux_gpu=False, release_gate_windows_gpu=False))

    def test_feature_dropdowns(self):
        o = run("device-machines", feature=True, feature_areas="all", feature_depth="full")
        self.assertEqual((o["feature_areas"], o["feature_depth"]), ("framework,wright,studio,prototype", "full"))


class DeviceMobile(unittest.TestCase):
    def test_tiers_are_boxes(self):
        o = run("device-mobile", phone_farm=True, phone_mid=False)
        self.assertEqual(o["phone_farm_tiers"], "low,high")
        self.assertEqual(run("device-mobile", phone_farm=True)["phone_farm_tiers"], "low,mid,high")
        self.assertTrue(fails("device-mobile", phone_farm=True, phone_low=False, phone_mid=False, phone_high=False))

    def test_farm_dropdowns(self):
        o = run("device-mobile", phone_farm=True, phone_platform="both", phone_count="5", phone_video=True)
        self.assertEqual((o["phone_farm_platform"], o["phone_farm_count"], o["phone_farm_video"]), ("both", "5", "true"))

    def test_android_phone_is_the_device_leg(self):
        o = run("device-mobile", android_phone=True)
        self.assertEqual((o["framework_any"], o["framework_legs"]), ("true", "android-device"))


class MatchesTheParts(unittest.TestCase):
    """A job renamed in a part without the catalogue following would be a
    step no box can reach (or a box that runs nothing)."""

    def test_steps_are_the_parts_jobs(self):
        files = {
            "quick": ["cloud-machines-quick.yml"], "studio": ["cloud-machines-studio.yml"],
            "wright_sandbox": ["cloud-machines-wright-sandbox.yml"], "wright_models": ["cloud-machines-wright-models.yml"],
            "wright_installers": ["cloud-machines-wright-installers.yml"],
            "audio": ["cloud-machines-audio-ci.yml", "cloud-machines-audio-backends.yml", "cloud-machines-audio-plugins.yml"],
            "supply_chain": ["cloud-machines-supply-chain.yml"], "goldens": ["cloud-machines-goldens.yml"],
            "publish_crates": ["cloud-machines-publish.yml"], "mobile_apps": ["cloud-mobile-apps.yml"],
            "u_apk": ["cloud-mobile-u-apk.yml"], "u_models": ["cloud-mobile-u-models.yml"],
            "mobile_audio": ["cloud-mobile-audio.yml"], "phone_farm": ["device-mobile-phone-farm.yml"],
        }
        for item, fs in files.items():
            want = set().union(*(job_ids(f) for f in fs))
            self.assertEqual(set(plan.ITEMS[item][0]), want, item)

    def test_framework_legs_are_the_parts_jobs(self):
        text = (WF / "shared-framework-checks.yml").read_text()
        legs = set(re.findall(r"',([a-z0-9-]+),'", text))
        mine = set(plan.ITEMS["framework"][0]) | set(plan.ITEMS["framework_mobile"][0])
        self.assertEqual(mine, legs)

if __name__ == "__main__":
    sys.exit(not unittest.main(exit=False).result.wasSuccessful())
