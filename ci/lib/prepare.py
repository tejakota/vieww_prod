"""The engine's first step after checkout: settle what this job does.

    python3 ci/lib/prepare.py        (env: CI_JOB, CI_INPUTS, CI_MATRIX, ...)

CI_JOB is the job's entry from the plan (ci/projects plan): its file,
`if`, needs, and the setup, cache and artifact definitions copied from
ci.toml. Their `${{ }}` expressions are evaluated here, on the job's own
machine after checkout, so `runner.temp`, `hashFiles()` and the job file's
`env` mean what they meant in the old workflows. Writes step outputs the
engine's action steps read (`go`, `java`, `cache_key`, `up0_path`, ...).

`go` is the job's `if:` with GitHub's rule: without a status function it
also needs every job in `needs` to have succeeded.
"""
import json
import os
import sys
import tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import expr  # noqa: E402
import run  # noqa: E402

UPLOADS, DOWNLOADS = 4, 2


def main():
    job = json.loads(os.environ["CI_JOB"])
    with open(job["file"], "rb") as f:
        spec = tomllib.load(f)
    out = {}

    needs = run.Needs(job["project"])
    status = "success"
    for n in job.get("needs", []):
        r = needs(n)["result"]
        if r != "success":
            status = "cancelled" if r == "cancelled" else "failure"
            print(f"needs: {n} → {r}")
    ctx = {"inputs": run._json_env("CI_INPUTS"), "matrix": run._json_env("CI_MATRIX"),
           "vars": run._json_env("CI_VARS"), "github": run.github_context(), "runner": run.runner_context(),
           "secrets": run.secrets_context(), "needs": needs, "steps": {}, "job": {"status": status},
           "strategy": {}, "__status__": status}
    env = {}
    for k, v in (spec.get("env") or {}).items():
        env[k] = expr.to_str(expr.interpolate(v if isinstance(v, str) else expr.to_str(v), {**ctx, "env": dict(env)}))
    ctx["env"] = env

    def ev(v):
        if isinstance(v, bool):
            return "true" if v else "false"
        return expr.to_str(expr.interpolate(expr.to_str(v), ctx))

    go = expr.condition(job.get("if"), ctx)
    if not go:
        why = job.get("if") or "a job it needs did not succeed"
        print(f"::notice::{job['title']}: skipped ({why})")
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
            f.write(f"- {job['title']}: skipped — `{why}`\n")
    out["go"] = "true" if go else "false"

    # Setup actions; a definition's `if` is evaluated here too.
    tools = []
    for s in job.get("setup", []):
        if s.get("if") and not expr.truthy(expr.evaluate(s["if"], {**ctx, "__status__": "success"})):
            continue
        w = {k: ev(v) for k, v in (s.get("with") or {}).items()}
        u = s["uses"]
        if u == "java":
            out["java"], out["java_dist"] = w.get("java-version", "17"), w.get("distribution", "temurin")
        elif u == "node":
            out["node"] = w.get("node-version", "20")
        elif u == "gradle":
            out["gradle"] = w.get("gradle-version", "")
        elif u == "tools":
            tools += [t.strip() for t in w["tool"].split(",") if t.strip()]
        elif u == "rust-cache":
            out["rust_cache"], out["rust_cache_key"] = w.get("workspaces", ""), w.get("key", "")
        elif u == "cache":
            out["cache_path"], out["cache_key"], out["cache_restore"] = w["path"], w["key"], w.get("restore-keys", "")
        elif u == "aws":
            out["aws"] = w.get("aws-region", "")
        elif u == "emulator":
            out["emulator"] = "true"
            for k in ("api-level", "arch", "target", "emulator-options"):
                out["emu_" + k.replace("-", "_")] = w.get(k, "")
        elif u == "attest":
            out["attest"] = w["subject-path"]
    out["tools"] = ",".join(dict.fromkeys(tools))
    out["has_final"] = "true" if any(s.get("part") == "final" for s in spec.get("step", [])) else "false"

    for i, u in enumerate(job.get("uploads", [])[:UPLOADS]):
        out[f"up{i}_name"] = ev(u.get("name", "artifact"))
        out[f"up{i}_path"] = ev(u["path"])
        out[f"up{i}_ifnf"] = ev(u.get("if-no-files-found", "warn"))
        out[f"up{i}_days"] = ev(u.get("retention-days", ""))
        out[f"up{i}_hidden"] = ev(u.get("include-hidden-files", False))
    for i, d in enumerate(job.get("downloads", [])[:DOWNLOADS]):
        out[f"dl{i}"] = "true"
        out[f"dl{i}_name"] = ev(d.get("name", ""))
        out[f"dl{i}_pattern"] = ev(d.get("pattern", ""))
        out[f"dl{i}_path"] = ev(d.get("path", ""))
        out[f"dl{i}_merge"] = ev(d.get("merge-multiple", False))
        out[f"dl{i}_soft"] = ev(d.get("continue-on-error", False))

    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
        for k, v in out.items():
            if "\n" in v:
                f.write(f"{k}<<__CI_EOF__\n{v}\n__CI_EOF__\n")
            else:
                f.write(f"{k}={v}\n")
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
