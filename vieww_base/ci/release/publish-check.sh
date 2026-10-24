#!/usr/bin/env bash
#
# FW-01: can the framework be published to crates.io as it stands?
#
#   ci/release/publish-check.sh            # metadata + package every public crate (no build)
#   ci/release/publish-check.sh --verify   # also build each packaged crate from its .crate
#   ci/release/publish-check.sh --order    # rewrite ci/release/publish-order.txt
#
# Fails when a publishable crate (no `publish = false`) is missing
# description / license / repository, when a dependency lacks a version (a
# path-only dependency cannot be published), or when publish-order.txt is
# stale. Publishing itself is `cargo publish --workspace` (cargo orders it),
# run by a human with a crates.io token; publish-order.txt is the same order
# written down for a manual or partial publish.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
mode="${1:-}"

python3 - "$mode" <<'PY'
import json, subprocess, sys
mode = sys.argv[1]
m = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
    check=True, capture_output=True, text=True).stdout)
public = {p["name"]: p for p in m["packages"] if p["publish"] != []}
bad = []
for name, p in sorted(public.items()):
    for key in ("description", "license", "repository"):
        if not p.get(key):
            bad.append(f"{name}: missing `{key}`")
    for d in p["dependencies"]:
        if d.get("path") and d["req"] == "*" and d["kind"] != "dev":
            bad.append(f"{name}: dependency `{d['name']}` is path-only (needs a version)")
        if d["name"] in {q["name"] for q in m["packages"]} and d["name"] not in public and d["kind"] != "dev":
            bad.append(f"{name}: depends on unpublishable `{d['name']}`")
# Leaf-first order over normal + build dependencies between public crates.
deps = {n: sorted({d["name"] for d in p["dependencies"] if d["name"] in public and d["kind"] != "dev"} - {n})
        for n, p in public.items()}
order, done = [], set()
def visit(n, stack=()):
    if n in done: return
    if n in stack: bad.append(f"dependency cycle through {n}"); return
    for d in deps[n]: visit(d, stack + (n,))
    done.add(n); order.append(n)
for n in sorted(public): visit(n)
text = "# Leaf-first publish order (ci/release/publish-check.sh --order rewrites this).\n" + "\n".join(order) + "\n"
path = "ci/release/publish-order.txt"
if mode == "--order":
    open(path, "w").write(text)
    print(f"wrote {path} ({len(order)} crates)")
else:
    try:
        cur = open(path).read()
    except FileNotFoundError:
        cur = ""
    if cur != text:
        bad.append(f"{path} is stale: run ci/release/publish-check.sh --order")
if bad:
    print("publish-check: NOT publishable:", *bad, sep="\n  ")
    sys.exit(1)
print(f"publish-check: {len(public)} public crates have complete metadata and versioned dependencies")
PY

[[ "$mode" == "--order" ]] && exit 0
mapfile -t crates < <(grep -v '^#' ci/release/publish-order.txt)
pkgs=("${crates[@]/#/-p}")
dirty=()
[[ -z "$(git status --porcelain 2>/dev/null)" ]] || dirty=(--allow-dirty)
if [[ "$mode" == "--verify" ]]; then
	# `cargo publish --workspace` skips publish = false itself and resolves
	# the in-workspace dependencies through a local overlay registry.
	cargo publish --workspace --dry-run --locked "${dirty[@]}"
else
	cargo package "${pkgs[@]}" --locked --no-verify --quiet "${dirty[@]}"
	echo "publish-check: all ${#crates[@]} public crates package"
fi
