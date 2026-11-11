"""The CI machinery: expressions, the step runner, the planner.

    python3 -m unittest discover -s ci/tests -v
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, "ci", "lib"))
import expr  # noqa: E402


class Expressions(unittest.TestCase):
    ctx = {"matrix": {"os": "windows", "hosted": True, "mode": "cpu", "export_only": False, "id": "w"},
           "inputs": {"scope": "gate", "only": "", "publish": False, "legs": "a,b"},
           "env": {"SCOPE": "gate"}, "github": {"event_name": "workflow_dispatch", "ref": "refs/tags/v1.2.0"},
           "steps": {"plan": {"outputs": {"x": "1"}}}, "__status__": "success"}

    def ev(self, text):
        return expr.evaluate(text, self.ctx)

    def test_operators_return_operands(self):
        self.assertEqual(self.ev("matrix.hosted && '1' || ''"), "1")
        self.assertEqual(self.ev("inputs.publish && 'yes' || 'no'"), "no")
        self.assertEqual(self.ev("github.event_name == 'push' && 'gate' || inputs.scope"), "gate")

    def test_loose_equality(self):
        self.assertTrue(self.ev("matrix.os == 'WINDOWS'"))
        self.assertTrue(self.ev("'1' == 1"))
        self.assertFalse(self.ev("inputs.publish == 'false'"))  # boolean vs string, as on GitHub
        self.assertTrue(self.ev("!matrix.export_only"))

    def test_functions(self):
        self.assertTrue(self.ev("startsWith(github.ref, 'refs/tags/v')"))
        self.assertTrue(self.ev("contains(format(',{0},', inputs.legs), ',b,')"))
        self.assertEqual(self.ev("format('{0}-{{x}}', matrix.id)"), "w-{x}")
        self.assertEqual(self.ev("fromJSON('[1,2]')[1]"), 2)
        self.assertEqual(self.ev("steps.plan.outputs.x"), "1")
        self.assertIsNone(self.ev("steps.nope.outputs.x"))

    def test_conditions_imply_success(self):
        failed = dict(self.ctx, __status__="failure")
        self.assertFalse(expr.condition("matrix.hosted", failed))
        self.assertTrue(expr.condition("always() && matrix.hosted", failed))
        self.assertTrue(expr.condition("failure()", failed))
        self.assertTrue(expr.condition(None, self.ctx))

    def test_interpolate(self):
        self.assertEqual(expr.interpolate("screens-${{ matrix.id }}/${{ inputs.publish }}", self.ctx), "screens-w/false")


JOB = r"""
name = "test job"
[defaults]
shell = "bash"
[env]
A = "${{ inputs.who }}"
[[step]]
id = "one"
name = "writes output and env"
run = '''
echo "x=42" >> "$GITHUB_OUTPUT"
echo "B=from-env" >> "$GITHUB_ENV"
echo "$A" > out.txt
'''
[[step]]
name = "reads them"
env = { X = "${{ steps.one.outputs.x }}" }
run = '''
echo "$X $B ${{ matrix.n }}" >> out.txt
'''
[[step]]
name = "skipped"
if = "inputs.who == 'nobody'"
run = "echo skipped >> out.txt"
[[step]]
name = "fails softly"
continue-on-error = true
run = "exit 3"
[[step]]
name = "fails"
run = "exit 4"
[[step]]
name = "not after a failure"
run = "echo nope >> out.txt"
[[step]]
name = "always"
if = "always()"
run = "echo always ${{ job.status }} >> out.txt"
[[step]]
name = "final part"
part = "final"
if = "failure()"
run = "echo final >> out.txt"
"""


class Runner(unittest.TestCase):
    def test_steps_like_github(self):
        with tempfile.TemporaryDirectory() as d:
            job = os.path.join(d, "job.toml")
            with open(job, "w") as f:
                f.write(JOB)
            env = dict(os.environ, GITHUB_WORKSPACE=d, RUNNER_TEMP=d, CI_INPUTS=json.dumps({"who": "me"}),
                       CI_MATRIX=json.dumps({"n": 7}), CI_STEP_TABLE="0", GITHUB_ENV=os.path.join(d, "env"))
            env.pop("GITHUB_STEP_SUMMARY", None)
            r = subprocess.run([sys.executable, os.path.join(ROOT, "ci/lib/run.py"), job], env=dict(env, CI_PART="main"),
                               capture_output=True, text=True)
            self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
            r2 = subprocess.run([sys.executable, os.path.join(ROOT, "ci/lib/run.py"), job], env=dict(env, CI_PART="final"),
                                capture_output=True, text=True)
            self.assertEqual(r2.returncode, 1)
            with open(os.path.join(d, "out.txt")) as f:
                got = f.read()
            self.assertEqual(got.split("\n"),
                             ["me", "42 from-env 7", "always failure", "final", ""])
            with open(os.path.join(d, "env")) as f:
                self.assertIn("B<<", f.read())


def plan(workflow, form, ref="refs/heads/main"):
    r = subprocess.run([sys.executable, os.path.join(ROOT, "ci/projects"), "plan", workflow, "--dry"],
                       env=dict(os.environ, FORM=json.dumps(form), GITHUB_REF=ref, GITHUB_STEP_SUMMARY=os.devnull), capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


class Planner(unittest.TestCase):
    def test_check_passes(self):
        r = subprocess.run([sys.executable, os.path.join(ROOT, "ci/projects"), "check"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_quick_check(self):
        rc, out = plan("check", {"project": "All projects", "depth": "quick", "run_on": "GitHub's machines"})
        self.assertEqual(rc, 0, out)
        self.assertIn("vieww · framework-gate", out)
        self.assertNotIn("framework-coverage", out)  # thorough only
        self.assertNotIn("self-hosted", out)

    def test_machine_check(self):
        rc, out = plan("check", {"project": "wright", "depth": "quick", "run_on": "My Linux machine"})
        self.assertEqual(rc, 0, out)
        self.assertIn("wright · machine", out)
        self.assertNotIn("sandbox-quick", out)

    def test_build_platforms(self):
        rc, out = plan("build", {"project": "wright", "linux": False, "windows": True, "macos": False,
                                 "android": False, "ios": False, "web": False})
        self.assertEqual(rc, 0, out)
        self.assertIn("installers (windows-x64)", out)
        self.assertNotIn("linux-x64", out)

    def test_devices(self):
        rc, out = plan("devices", {"linux": False, "windows": True, "mac": False, "suite": "wright docker suite (Linux)",
                                   "usb_phone": False, "farm": "off"})
        self.assertNotEqual(rc, 0)  # Windows has no docker suite, nothing else picked
        rc, out = plan("devices", {"linux": True, "windows": False, "mac": False, "suite": "everything",
                                   "usb_phone": False, "farm": "Android", "farm_low": True})
        self.assertEqual(rc, 0, out)
        self.assertIn("U · farm-phones", out)
        self.assertNotIn("farm-iphones", out)
        rc, out = plan("devices", {"linux": True, "farm": "off"}, ref="refs/heads/topic")
        self.assertNotEqual(rc, 0)
        self.assertIn("main only", out)

    def test_release_publish_waits(self):
        rc, out = plan("release", {"mode": "publish", "include_machines": False, "version": "2.0.0"})
        self.assertEqual(rc, 0, out)
        self.assertIn("waits for approval", out)
        self.assertIn("wright · installers-release", out)
        self.assertIn("viewwsite · deploy", out)
        rc, out = plan("release", {"mode": "rehearse"})
        self.assertNotIn("deploy", out)
        self.assertNotIn("approval", out)

    def test_every_project_in_dropdown(self):
        with open(os.path.join(ROOT, ".github/workflows/1-check.yml")) as f:
            text = f.read()
        for name in ("vieww", "wright", "vavlt", "viewwsite", "U"):
            self.assertIn(f"          - {name}\n", text)


if __name__ == "__main__":
    unittest.main()
