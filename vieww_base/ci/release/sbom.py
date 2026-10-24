#!/usr/bin/env python3
"""Write a CycloneDX 1.5 SBOM for a Cargo workspace (FW-23, checklist G8.7).

    python3 ci/release/sbom.py [--manifest Cargo.toml] [--bin PKG] -o sbom.cdx.json

Built from `cargo metadata --locked`, so it lists exactly what Cargo.lock
resolves — every crate with its version, license, source and the lockfile's
sha256 checksum — and needs nothing but Python and Cargo (no cargo-cyclonedx
install on the release machines). With `--bin PKG` only PKG's normal
(non-dev, non-build) dependency closure is listed, which is what ships in that
binary; without it, the whole workspace graph.

Deterministic: no timestamp unless SOURCE_DATE_EPOCH is set, components
sorted, serial number derived from the content — the same lockfile gives the
same bytes.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
import uuid


def metadata(manifest):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", manifest],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def lock_checksums(manifest):
    import tomllib

    lock = os.path.join(os.path.dirname(os.path.abspath(manifest)), "Cargo.lock")
    try:
        with open(lock, "rb") as f:
            data = tomllib.load(f)
    except FileNotFoundError:
        return {}
    return {
        (p["name"], p["version"]): p["checksum"]
        for p in data.get("package", [])
        if "checksum" in p
    }


def closure(meta, root_name):
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    roots = [p["id"] for p in meta["packages"] if p["name"] == root_name and p["id"] in meta["workspace_members"]]
    if not roots:
        sys.exit(f"sbom: no workspace package named {root_name}")
    seen, stack = set(), list(roots)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            kinds = {k.get("kind") for k in dep["dep_kinds"]}
            if None in kinds:  # a normal dependency (not dev, not build)
                stack.append(dep["pkg"])
    return [pkgs[i] for i in seen], roots[0]


def purl(p):
    src = p.get("source") or ""
    if src.startswith("registry+"):
        return f"pkg:cargo/{p['name']}@{p['version']}"
    if src.startswith("git+"):
        return f"pkg:cargo/{p['name']}@{p['version']}?vcs_url={src[4:]}"
    return f"pkg:cargo/{p['name']}@{p['version']}?local=true"


def component(p, sums):
    c = {
        "type": "library",
        "bom-ref": purl(p),
        "name": p["name"],
        "version": p["version"],
        "purl": purl(p),
    }
    if p.get("description"):
        c["description"] = " ".join(p["description"].split())
    if p.get("license"):
        c["licenses"] = [{"expression": p["license"]}]
    s = sums.get((p["name"], p["version"]))
    if s:
        c["hashes"] = [{"alg": "SHA-256", "content": s}]
    refs = []
    if p.get("repository"):
        refs.append({"type": "vcs", "url": p["repository"]})
    if p.get("source"):
        refs.append({"type": "distribution", "url": p["source"]})
    if refs:
        c["externalReferences"] = refs
    return c


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default="Cargo.toml")
    ap.add_argument("--bin", help="only this package's shipped dependency closure")
    ap.add_argument("-o", "--out", required=True)
    a = ap.parse_args()

    meta = metadata(a.manifest)
    sums = lock_checksums(a.manifest)
    if a.bin:
        pkgs, root_id = closure(meta, a.bin)
    else:
        pkgs, root_id = meta["packages"], None
    by_id = {p["id"]: p for p in meta["packages"]}
    root = by_id[root_id] if root_id else None
    comps = sorted(
        (component(p, sums) for p in pkgs if p["id"] != root_id),
        key=lambda c: (c["name"], c["version"]),
    )
    ids = {p["id"] for p in pkgs}
    deps = []
    for n in sorted(meta["resolve"]["nodes"], key=lambda n: n["id"]):
        if n["id"] not in ids:
            continue
        on = sorted({purl(by_id[d["pkg"]]) for d in n["deps"] if d["pkg"] in ids})
        deps.append({"ref": purl(by_id[n["id"]]), "dependsOn": on})

    body = {"components": comps, "dependencies": deps}
    digest = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).digest()
    bom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.UUID(bytes=digest[:16], version=5)}",
        "version": 1,
        "metadata": {
            "tools": {"components": [{"type": "application", "name": "vieww ci/release/sbom.py"}]},
        },
        **body,
    }
    if root:
        bom["metadata"]["component"] = component(root, sums) | {"type": "application"}
    epoch = os.environ.get("SOURCE_DATE_EPOCH")
    if epoch:
        bom["metadata"]["timestamp"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(int(epoch)))
    with open(a.out, "w", encoding="utf-8") as f:
        json.dump(bom, f, indent=2, sort_keys=False)
        f.write("\n")
    print(f"sbom: {len(comps)} components -> {a.out}")


if __name__ == "__main__":
    main()
