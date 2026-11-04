#!/usr/bin/env bash
# The plugin compatibility matrix (FW-17): can somebody who is not the ABI's
# author build a plugin against this framework, and does a plugin built
# against an *older* release still run in today's host?
#
#     ci/check/plugin-compat.sh
#
# Three proofs, in order:
#
# 1. PATH leg — `examples/plugin-minimal`, a real plugin `cdylib` that is
#    its own workspace (not a member of ours), compiled against the
#    framework purely by `path` dependency, then clippy'd with `-D
#    warnings`: the contract as shipped is buildable and clean from the
#    outside.
# 2. HOST leg — that binary is loaded and fully exercised by
#    `load_plugin`, the example host built from *this* tree: the documented
#    lifecycle (version gate → init → commands → invoke → shutdown) runs
#    against the current host, end to end, through a real `dlopen`.
# 3. PUBLISHED leg — a copy of the same example with its dependency
#    rewritten to the *last published* `vieww-plugin` (crates.io), built
#    with that release, then loaded by the same current-host runner. This
#    is what "additive-only within a major" means, demonstrated rather than
#    asserted; it is the leg that goes red the day an ABI change breaks a
#    plugin somebody already shipped.
#
# Until FW-01's first publish reaches crates.io there is nothing to test
# against, and the published leg **skips loudly** — a warning with the
# reason in the run log — rather than quietly testing nothing. The path and
# host legs always run; they are the parts this machine can prove alone.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# Some rustc installs (this repository's own sandbox among them) link libstd
# dynamically, so the runner binary — and any `cdylib` it dlopens — need the
# toolchain's lib dir on the loader path. CI's static-std toolchains have
# the same directory without needing it, so exporting unconditionally is
# harmless there. Derived from the sysroot, silently skipped when rustc is
# somewhere exotic. (Same stanza as feature-a11y.sh.)
sysroot="$(rustc --print sysroot 2>/dev/null || true)"
if [ -n "$sysroot" ]; then
    std_lib="$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib"
    if [ -d "$std_lib" ]; then
        export LD_LIBRARY_PATH="$std_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/vieww-plugin-compat.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# Where this platform's cargo puts the example's cdylib.
find_cdylib() {
    local dir="$1"
    for name in libvieww_plugin_minimal.so libvieww_plugin_minimal.dylib \
                vieww_plugin_minimal.dll; do
        if [ -f "$dir/$name" ]; then
            printf '%s\n' "$dir/$name"
            return 0
        fi
    done
    return 1
}

echo "== 1/3 plugin-minimal, compiled against the framework by path =="
cargo build --manifest-path examples/plugin-minimal/Cargo.toml \
    --target-dir "$scratch/path"
cargo clippy --manifest-path examples/plugin-minimal/Cargo.toml \
    --target-dir "$scratch/path" -- -D warnings
path_cdylib="$(find_cdylib "$scratch/path/debug")"
echo "   built: $path_cdylib"

echo "== 2/3 that binary, driven by the current host (load_plugin) =="
cargo build -p vieww-plugin --example load_plugin
"$root/target/debug/examples/load_plugin" "$path_cdylib"
# Exit status is the contract: 0 only if every command invoked cleanly and
# the shutdown sequence ran. `set -e` fails the leg otherwise.

echo "== 3/3 the same example, compiled against the last published vieww-plugin =="
published="$(curl -fsSL --max-time 30 \
    -A "vieww-ci (plugin-compat leg)" \
    https://crates.io/api/v1/crates/vieww-plugin 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["crate"]["max_version"])' \
    2>/dev/null || true)"

if [ -z "$published" ]; then
    # The loud skip: visible as a warning annotation in the run, and as this
    # text in the log. Not a failure — there is no release to be compatible
    # with yet, and pretending otherwise would test nothing quietly.
    echo "::warning::vieww-plugin is not on crates.io yet (FW-01's first publish). The published-compat half of this leg has nothing to test against and is skipped; the path-dep and current-host halves above ran for real."
    echo "   skipped: no published vieww-plugin release to compile against"
    exit 0
fi
echo "   last published: vieww-plugin $published"

mkdir -p "$scratch/published"
cp examples/plugin-minimal/Cargo.toml "$scratch/published/"
cp -r examples/plugin-minimal/src "$scratch/published/"
python3 - "$published" "$scratch/published/Cargo.toml" <<'PY'
import re, sys
version, manifest = sys.argv[1], sys.argv[2]
text = open(manifest, encoding="utf-8").read()
rewritten, count = re.subn(
    r'vieww-plugin = \{ path = "[^"]*" \}',
    f'vieww-plugin = "={version}"',
    text,
)
assert count == 1, f"expected exactly one path dependency to rewrite, found {count}"
open(manifest, "w", encoding="utf-8").write(rewritten)
PY

cargo build --manifest-path "$scratch/published/Cargo.toml" \
    --target-dir "$scratch/published-target"
published_cdylib="$(find_cdylib "$scratch/published-target/debug")"
echo "   built against $published: $published_cdylib"

# The whole point: a plugin binary from an older release, loaded and run by
# the host built from this tree. Any incompatibility within major 1 is a
# broken promise and this goes red.
"$root/target/debug/examples/load_plugin" "$published_cdylib"

echo "plugin compatibility matrix: green (path, current host, published $published)"
