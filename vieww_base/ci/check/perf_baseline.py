"""Fetch a leg's last published performance figures from ci-results (FW-08).

    python3 -I ci/check/perf_baseline.py FOLDER ARTIFACT OUT.json

FOLDER is the workflow's folder on the `ci-results` branch (e.g.
`framework-checks`, `hardware-self-hosted-linux-and-windows`); ARTIFACT
the uploaded artifact holding the leg's `perf-measured.json` (from
`perf-budget.sh --write-measured`). Writes the newest copy under
FOLDER/main/latest/ to OUT.json. No copy (a first run, a leg skipped last
time, no network) leaves OUT.json absent, which perf_budget.py treats as
"no previous run". Always exits 0: a missing baseline is not an error.
"""

import subprocess
import sys


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True)


def main(argv):
    if len(argv) != 3:
        print(__doc__)
        return 2
    folder, artifact, out = argv
    git("fetch", "-q", "--depth=1", "origin", "ci-results")
    tree = git("ls-tree", "-r", "--name-only", "FETCH_HEAD", f"{folder}/main/latest/")
    if tree.returncode != 0:
        print(f"perf-baseline: no ci-results/{folder} to read ({tree.stderr.strip()[:200]})")
        return 0
    hits = [p for p in tree.stdout.splitlines()
            if p.endswith("/perf-measured.json") and f"/{artifact}/" in p]
    if not hits:
        print(f"perf-baseline: no previous {artifact}/perf-measured.json under ci-results/{folder}")
        return 0
    blob = git("show", f"FETCH_HEAD:{hits[0]}")
    if blob.returncode == 0:
        with open(out, "w") as f:
            f.write(blob.stdout)
        print(f"perf-baseline: last run's figures from ci-results/{hits[0]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
