"""Run one job file's steps, the way GitHub would run them.

    python3 ci/lib/run.py vieww_base/ci/jobs/gate.toml

With CI_PART set (main, emulator or final) only that part's steps run;
a step's `part` says which (main by default). The engine runs the parts
around the actions that sit between them (the Android emulator, build
provenance), and state carries over: the job's status, `steps.*`,
GITHUB_ENV and GITHUB_PATH.

A job file is what a job's `steps:` used to be, minus the `uses:` steps
(the engine does checkout, caches, toolchains and artifacts from the
job's entry in the project's ci.toml). Each step keeps GitHub's meaning:

    name, id, if, run, shell, working-directory, env,
    continue-on-error, timeout-minutes

plus the file's `[defaults]` (shell, working-directory) and `[env]`.
`${{ ... }}` is evaluated with the run's real values: the engine passes
the job's inputs (CI_INPUTS), matrix (CI_MATRIX), vars (CI_VARS) and the
secrets its groups allow; `steps.<id>.outputs`, GITHUB_ENV, GITHUB_PATH
and `needs.<job>.result` work as they do on GitHub.

Each step prints as a collapsible group, and the run ends with a table of
outcomes in the job summary.
"""
import json
import os
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import expr  # noqa: E402

WINDOWS = os.name == "nt"


def _which_bash():
    """The bash that runs GitHub-flavoured shell steps.

    On Windows, a bare `bash` on PATH resolves to
    `C:\\Windows\\System32\\bash.exe`, the *WSL launcher* — with no
    distro installed (GitHub's hosted Windows runners ship none) every
    step dies with "Windows Subsystem for Linux has no installed
    distributions" before the script's first line runs (run 2026-11-13,
    3-build: `Rust toolchain`, `Allow long paths (Windows)`). The
    runner's own `shell: bash` uses Git's bash; resolve the same binary
    explicitly, skipping anything that smells like WSL.
    """
    if not WINDOWS:
        return "bash"
    for cand in (
        os.environ.get("CI_BASH"),
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
    ):
        if cand and os.path.isfile(cand):
            return cand
    for p in os.environ.get("PATH", "").split(os.pathsep):
        if not p:
            continue
        cand = os.path.join(p, "bash.exe")
        low = cand.lower()
        if ("system32" in low or "sysnative" in low or "wsl" in low
                or not os.path.isfile(cand)):
            continue
        return cand
    return "bash"  # let the OS say it cannot find it, in its own words


BASH = _which_bash()


def _json_env(name):
    raw = os.environ.get(name) or "{}"
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def read_kv_file(path):
    """GITHUB_ENV / GITHUB_OUTPUT: `k=v` lines and `k<<DELIM` heredocs."""
    out = {}
    if not os.path.exists(path):
        return out
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.read().splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        i += 1
        if "<<" in line and ("=" not in line or line.index("<<") < line.index("=")):
            key, delim = line.split("<<", 1)
            buf = []
            while i < len(lines) and lines[i] != delim:
                buf.append(lines[i])
                i += 1
            i += 1
            out[key] = "\n".join(buf)
        elif "=" in line:
            k, v = line.split("=", 1)
            out[k] = v
    return out


class Needs:
    """`needs.<job>.result` from the run's jobs, looked up on first use."""

    def __init__(self, project):
        self.project, self.jobs = project, None

    def _load(self):
        if self.jobs is not None:
            return
        self.jobs = []
        token = os.environ.get("CI_GITHUB_TOKEN")
        api = os.environ.get("GITHUB_API_URL", "https://api.github.com")
        repo, run = os.environ.get("GITHUB_REPOSITORY"), os.environ.get("GITHUB_RUN_ID")
        if not (token and repo and run):
            return
        page = 1
        while True:
            req = urllib.request.Request(
                f"{api}/repos/{repo}/actions/runs/{run}/jobs?per_page=100&page={page}",
                headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"})
            try:
                with urllib.request.urlopen(req, timeout=30) as r:
                    data = json.load(r)
            except Exception as e:  # noqa: BLE001
                print(f"::warning::could not read this run's jobs for needs.*: {e}")
                return
            self.jobs += data.get("jobs", [])
            if len(data.get("jobs", [])) < 100:
                return
            page += 1

    def __call__(self, job):
        self._load()
        tag = f"{self.project} · {job}"
        hits = [j for j in self.jobs if j.get("name", "").split(" / ")[-1].split(" (")[0] == tag]
        results = [j.get("conclusion") or "pending" for j in hits]
        if not results:
            result = "skipped"
        elif "failure" in results or "timed_out" in results:
            result = "failure"
        elif "cancelled" in results:
            result = "cancelled"
        elif all(r == "skipped" for r in results):
            result = "skipped"
        else:
            result = "success"
        return {"result": result, "outputs": {}}


def github_context():
    e = os.environ
    g = {k[len("GITHUB_"):].lower(): v for k, v in e.items() if k.startswith("GITHUB_")}
    g["token"] = e.get("CI_GITHUB_TOKEN", "")
    try:
        with open(e.get("GITHUB_EVENT_PATH", ""), encoding="utf-8") as f:
            g["event"] = json.load(f)
    except (OSError, json.JSONDecodeError):
        g["event"] = {}
    return g


def runner_context():
    e = os.environ
    return {"os": e.get("RUNNER_OS") or {"Linux": "Linux", "Darwin": "macOS", "Windows": "Windows"}.get(platform.system(), platform.system()),
            "arch": e.get("RUNNER_ARCH", platform.machine().upper()),
            "temp": e.get("RUNNER_TEMP", tempfile.gettempdir()),
            "tool_cache": e.get("RUNNER_TOOL_CACHE", ""),
            "name": e.get("RUNNER_NAME", ""),
            "debug": e.get("RUNNER_DEBUG", ""),
            "environment": e.get("RUNNER_ENVIRONMENT", "")}


def secrets_context():
    names = [n for n in (os.environ.get("CI_SECRET_NAMES") or "").split(",") if n]
    allowed = {n: os.environ.get(n, "") for n in names}
    allowed["GITHUB_TOKEN"] = os.environ.get("CI_GITHUB_TOKEN", "")
    return allowed


def shell_command(shell, path):
    """GitHub's shell templates."""
    if shell is None:
        shell = "pwsh" if WINDOWS else "bash-default"
    if shell == "bash-default":
        return [BASH, "-e", path]
    if shell == "bash":
        return [BASH, "--noprofile", "--norc", "-eo", "pipefail", path]
    if shell == "sh":
        if WINDOWS:
            return [BASH, "--noprofile", "--norc", "-e", path]
        return ["sh", "-e", path]
    if shell in ("pwsh", "powershell"):
        exe = "pwsh" if shell == "pwsh" else "powershell"
        return [exe, "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", f". '{path}'"]
    if shell == "python":
        return [sys.executable if not os.environ.get("CI_PYTHON") else os.environ["CI_PYTHON"], path]
    if shell == "cmd":
        return [os.environ.get("ComSpec", "cmd.exe"), "/D", "/E:ON", "/V:OFF", "/S", "/C", f'CALL "{path}"']
    parts = shlex.split(shell)
    return [p.replace("{0}", path) for p in parts] if "{0}" in shell else parts + [path]


def script_body(shell, text):
    if shell in ("pwsh", "powershell") or (shell is None and WINDOWS):
        return ("$ErrorActionPreference = 'stop'\n" + text +
                "\nif ((Test-Path -LiteralPath variable:\\LASTEXITCODE)) { exit $LASTEXITCODE }\n"), ".ps1"
    if shell == "python":
        return text, ".py"
    if shell == "cmd":
        return text, ".cmd"
    return text, ".sh"


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: run.py JOB_FILE")
    job_file = sys.argv[1]
    with open(job_file, "rb") as f:
        job = tomllib.load(f)

    workspace = os.environ.get("GITHUB_WORKSPACE") or os.getcwd()
    project = os.environ.get("CI_PROJECT", "")
    part = os.environ.get("CI_PART", "")
    state_file = os.path.join(os.environ.get("RUNNER_TEMP") or tempfile.gettempdir(), "ci-run-state.json")
    state = {}
    if part and part != "main" and os.path.exists(state_file):
        with open(state_file, encoding="utf-8") as f:
            state = json.load(f)
    status = state.get("status", "success")
    steps_ctx = state.get("steps", {})
    base_env = dict(os.environ)
    added_env, added_path = state.get("env", {}), state.get("path", [])
    defaults = job.get("defaults", {})
    rows = []

    def ctx(env):
        return {"inputs": _json_env("CI_INPUTS"), "matrix": _json_env("CI_MATRIX"), "vars": _json_env("CI_VARS"),
                "github": github_context(), "runner": runner_context(), "secrets": secrets_context(),
                "steps": steps_ctx, "needs": Needs(project), "env": env,
                "job": {"status": status}, "strategy": {}, "__status__": status}

    # job-level env (the old workflow's env, then the job's)
    job_env = {}
    for k, v in (job.get("env") or {}).items():
        job_env[k] = expr.to_str(expr.interpolate(v if isinstance(v, str) else expr.to_str(v), ctx({**job_env})))

    tmp = tempfile.mkdtemp(prefix="ci-steps-", dir=os.environ.get("RUNNER_TEMP"))
    for n, step in enumerate(job.get("step", []), 1):
        if part and step.get("part", "main") != part:
            continue
        name = step.get("name") or (step.get("run", "").strip().splitlines() or [f"step {n}"])[0][:80]
        cur_env = {**job_env, **added_env}
        c = ctx(cur_env)
        try:
            go = expr.condition(step.get("if"), c)
        except expr.ExprError as e:
            print(f"::error::{name}: {e}")
            go, status = False, "failure"
        if not go:
            rows.append((name, "skipped"))
            if step.get("id"):
                steps_ctx[step["id"]] = {"outputs": {}, "outcome": "skipped", "conclusion": "skipped"}
            continue

        step_env = {}
        for k, v in (step.get("env") or {}).items():
            step_env[k] = expr.to_str(expr.interpolate(v if isinstance(v, str) else expr.to_str(v), ctx(cur_env)))
        env = {**base_env, **cur_env, **step_env}
        if added_path:
            env["PATH"] = os.pathsep.join(reversed(added_path)) + os.pathsep + base_env.get("PATH", "")
        files = {k: os.path.join(tmp, f"{n}.{k.lower()}") for k in ("GITHUB_ENV", "GITHUB_PATH", "GITHUB_OUTPUT", "GITHUB_STATE")}
        for k, p in files.items():
            open(p, "w").close()
            env[k] = p

        shell = step.get("shell", defaults.get("shell"))
        wd = expr.interpolate(step.get("working-directory", defaults.get("working-directory", ".")), c)
        cwd = os.path.join(workspace, wd)
        body, ext = script_body(shell, expr.interpolate(step["run"], ctx(cur_env)))
        path = os.path.join(tmp, f"{n}{ext}")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(body)
        timeout = step.get("timeout-minutes")
        print(f"::group::{name}", flush=True)
        t0 = time.time()
        try:
            rc = subprocess.run(shell_command(shell, path), cwd=cwd, env=env,
                                timeout=float(timeout) * 60 if timeout else None).returncode
        except subprocess.TimeoutExpired:
            print(f"::error::{name}: timed out after {timeout} minutes")
            rc = 124
        except FileNotFoundError as e:
            print(f"::error::{name}: {e}")
            rc = 127
        print("::endgroup::", flush=True)
        took = time.time() - t0

        outputs = read_kv_file(files["GITHUB_OUTPUT"])
        added_env.update(read_kv_file(files["GITHUB_ENV"]))
        with open(files["GITHUB_PATH"], encoding="utf-8") as f:
            added_path += [line.strip() for line in f if line.strip()]
        outcome = "success" if rc == 0 else "failure"
        conclusion = outcome
        if rc != 0:
            soft = step.get("continue-on-error", False)
            if isinstance(soft, str):
                soft = expr.truthy(expr.evaluate(soft, ctx(cur_env)))
            if soft:
                conclusion = "success"
                print(f"::warning::{name} failed (exit {rc}); continuing as the step allows")
            else:
                print(f"::error::{name} failed (exit {rc})")
                status = "failure"
        if step.get("id"):
            steps_ctx[step["id"]] = {"outputs": outputs, "outcome": outcome, "conclusion": conclusion}
        rows.append((name, f"{outcome} · {took:.0f}s" + (" (allowed)" if outcome != conclusion else "")))

    if part:
        with open(state_file, "w", encoding="utf-8") as f:
            json.dump({"status": status, "steps": steps_ctx, "env": added_env, "path": added_path}, f)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary and rows and os.environ.get("CI_STEP_TABLE", "1") == "1":
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"\n<details><summary>{project} · {job.get('name', os.path.basename(job_file))}: steps{' (' + part + ')' if part and part != 'main' else ''}</summary>\n\n")
            f.write("| step | outcome |\n|---|---|\n")
            for name, r in rows:
                f.write(f"| {name.replace('|', '/')} | {r} |\n")
            f.write("\n</details>\n")
    # Pass what the steps exported on, for the engine's later steps.
    if os.environ.get("GITHUB_ENV"):
        with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as f:
            for k, v in added_env.items():
                f.write(f"{k}<<__CI_EOF__\n{v}\n__CI_EOF__\n")
    sys.exit(0 if status == "success" else 1)


if __name__ == "__main__":
    main()
