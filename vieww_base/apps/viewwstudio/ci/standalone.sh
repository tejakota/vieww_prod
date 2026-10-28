#!/usr/bin/env bash
# Compile and run the studio's dependency-free modules with `rustc` alone.
#
# # Why this exists
#
# A dozen of the studio's modules — `json`, `task`, `toolchains`, `folding`,
# `picker`, `tokens`, `language`, plus `lsp`, `cargo`, `jobs` and `git` in the
# harness — use nothing but `std`. They were written
# that way on purpose: the build pipeline, the toolchain checklist, the project
# scaffolder and the editor comforts are all logic that has no business needing
# a GPU, a font stack or a window to be checked.
#
# The payoff is that this script runs their whole suite on a machine with no
# network and no crates.io — which is where it was developed, and which is also
# every CI container's first thirty seconds. `cargo test -p viewwstudio` remains
# the real gate; this is the one that still works when the real gate cannot
# fetch a dependency, and it is a strictly faster inner loop besides: the whole
# thing builds and runs in a couple of seconds.
#
#     apps/viewwstudio/ci/standalone.sh
#
# The modules under test are listed in ONE place — `MODULES` below — and each is
# compiled twice: once alone (which proves it really has no crate-internal
# dependency it forgot to declare) and once inside a harness that wires the few
# that do depend on each other together, with a stub for `state::Diagnostic`.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$here/../src"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

# `rustc` is invoked from a directory outside the vieww workspace on purpose:
# inside it, `rust-toolchain.toml` lists `llvm-tools-preview`, and rustup tries
# to re-download the toolchain to satisfy it. That fails with no network and the
# error is about a channel manifest, which reads as nothing to do with this.
cd "$out"

# `scaffold::checkout_root` reads `env!("CARGO_MANIFEST_DIR")`, a compile-time
# macro that reads the *process* environment variable of the same name — cargo
# sets it for every crate it builds, but nothing sets it for a bare `rustc`
# invocation, which is exactly what this script is. Without it, compiling
# `scaffold.rs` here — not through `cargo build` at all — fails immediately with
# "environment variable `CARGO_MANIFEST_DIR` not defined at compile time",
# before a single test runs. Set to the same value cargo would give the real
# crate: `apps/viewwstudio`, two levels above this script.
export CARGO_MANIFEST_DIR="$here/.."

# Modules with no dependency on any other studio module.
#
# `scaffold`, `edit_ops`, `settings`, `customise`, `builds` and `export` used to
# be here; each has since grown a dependency on the crate (themes, the live doc,
# the test harness, Say codegen) and is covered by `cargo test -p viewwstudio`
# instead. A module that stops compiling alone is removed here, not stubbed.
ALONE=(json task toolchains folding picker tokens language)
failed=0

echo "== each module alone =="
for module in "${ALONE[@]}"; do
    printf '%-12s ' "$module"
    if rustc --test --edition 2021 -o "$module.test" "$src/$module.rs" 2>"$module.err"; then
        "./$module.test" --test-threads=4 2>&1 | grep 'test result' || failed=1
    else
        echo "DID NOT COMPILE"
        cat "$module.err"
        failed=1
    fi
done

# `cargo.rs` and `builds.rs` reach for `crate::state::Diagnostic` and for each
# other, so they need a root. The stub below must stay identical to the real
# `state::Diagnostic` — if it drifts, this script passes while the crate does
# not build, which is worse than not having the script.
echo
echo "== the modules that depend on each other =="
cat > harness.rs <<EOF
#[path = "$src/json.rs"]
pub mod json;
#[path = "$src/lsp.rs"]
pub mod lsp;
#[path = "$src/cargo.rs"]
pub mod cargo;
#[path = "$src/task.rs"]
pub mod task;
#[path = "$src/toolchains.rs"]
pub mod toolchains;
#[path = "$src/jobs.rs"]
pub mod jobs;
#[path = "$src/git.rs"]
pub mod git;

pub mod state {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Severity { Error, Warning }
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Diagnostic {
        pub file: String,
        pub severity: Severity,
        pub code: String,
        pub message: String,
        pub help: Option<String>,
        pub line: u32,
        pub column: u32,
        pub end_line: u32,
        pub end_column: u32,
    }
    impl Diagnostic {
        #[must_use]
        pub fn location(&self) -> String { format!("{}:{}", self.line, self.column) }
    }
}
EOF

printf '%-12s ' "harness"
if rustc --test --edition 2021 -o harness.test harness.rs 2>harness.err; then
    ./harness.test --test-threads=4 2>&1 | grep 'test result' || failed=1
else
    echo "DID NOT COMPILE"
    cat harness.err
    failed=1
fi

echo
if [ "$failed" -eq 0 ]; then
    echo "standalone: green"
else
    echo "standalone: FAILED"
fi
exit "$failed"
