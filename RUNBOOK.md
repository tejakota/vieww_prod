# Runner runbook — what CI cannot fix from inside the repo

The workflows repair what they can themselves (Git Bash for `.sh` steps, a
venv for the models' Python, a kernel-verification in the ALSA VM job, the
collector's redirect-following log fetch). The rest is machine state only a
person can set, and this is that list. Every entry names the runs that
failed for it and the check that says it is fixed.

## 1 · The release-signing secret (1 · Check, 3 · Build)

`wright-runtime`'s update channel signs manifests with a key CI must be
given. An empty value is now treated as "not set", so the test no longer
fails on the missing secret — but the channel cannot sign anything until
the key exists. Make one and set it once:

```sh
# locally: make a 32-byte key, 64 hex characters
openssl rand -hex 32
# then on GitHub (Settings ▸ Secrets and variables ▸ Actions):
gh secret set WRIGHT_SECRET_WRIGHT_RELEASE_SIGNING --body "<the 64 hex chars>"
```

The release-gate and publish jobs use it; `wright secret set
keychain:wright/release-signing` is the local-machine equivalent.

## 2 · The self-hosted Windows runner (2 · Devices, hardware)

The 2026-11-06 run found no `rustc` on PATH and skipped every check. The
job now probes `%USERPROFILE%\.cargo\bin` and `C:\Users\Teja\.cargo\bin`
itself, but the deeper issue is how the runner is started: registered as a
service it runs as another account, in session 0, and the GPU/D3D12/desktop
steps need an interactive desktop anyway.

- Stop the service; start the runner with `run.cmd` from a logged-in
  desktop session.
- Install Rust for that account (`rustup`, the version
  `vieww_base/rust-toolchain.toml` pins) so `rustc` sits in that user's
  `.cargo\bin`.

Fixed when: a `2 · Devices` Windows run reports `rustc: 1.98.1 (pinned …)`
in its summary header instead of `NOT RESOLVED`.

## 3 · The self-hosted macOS runner (2 · Devices)

The 2026-11-12 hardware-macos leg died at `mkdir: /Users/runner: Permission
denied` — setup-python trying to write a tool cache that does not belong to
the runner user. The engine now installs Python through Homebrew on macOS
and tolerates a failing setup-python elsewhere, so this should not recur;
if a mac leg still cannot find `python3.12`, `brew install python@3.12` as
the runner user is the fix.

## 4 · WSL is no longer required on Windows runners

`ci/lib/run.py` now runs bash steps through Git for Windows' bash (the
runner's own `shell: bash` binary); the old code picked up
`System32\bash.exe`, the WSL launcher, which fails with "Windows Subsystem
for Linux has no installed distributions" when no distro is installed —
GitHub's hosted Windows runners ship none. **No action needed**; if you
want WSL-based POSIX sandboxing on the fail-closed suite's terms, that is
an opt-in machine decision, not a requirement.

## 5 · The Device Farm budget (phone-lm-q720)

The October 30 run measured `auto_frame_time_p50` 25.6% over the recorded
budget. The paint pipeline (M1–M5) landed after that run; re-run
`1 · Check` (project U, thorough) and let the numbers speak. If the phone
still misses, re-record the budget as a reviewed commit:
`ci/check/perf-budget.sh --record` (the budgets live in
`ci/check/perf-budgets.json`), or investigate the renderer — the budget
file documents the discipline.

## 6 · Re-verifying everything

The failing workflows were re-dispatched after these fixes landed. The
`ci-results` branch's `LATEST.md` is the scoreboard; `errors.txt` next to
each summary now carries the real error excerpts again (the collector had
been publishing empty ones since the logs endpoint began redirecting).
