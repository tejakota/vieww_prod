#!/usr/bin/env bash
#
# Can the widget gallery say anything? — the accessibility gate over every
# feature example, with no display at all.
#
#   ci/check/feature-a11y.sh            # build + audit every feature example
#   ci/check/feature-a11y.sh 25 47      # only these (numbers as in the dirs)
#
# # Why the gallery is audited, not just looked at
#
# `VIEWW_SHOT` proves an example draws; this proves it *says* something. Every
# example under `examples/features/` is run with `VIEWW_AUDIT=1` (the harness
# mode `examples/features/harness/src/audit_mode.rs` documents): the semantics
# tree is walked for unlabelled controls and undersized targets, and every
# painted glyph run is judged for WCAG AA contrast against the backdrop it was
# actually painted over. Any error fails the run — a gallery that ships an
# unlabelled button teaches every reader of the example to ship one too.
#
# The same fixed clock as the shot mode, so the audit is a gate rather than a
# flake: no display, no input, the same tree on every machine.
#
# # The two examples that are not in it, and why
#
# `54-image-mipmaps-atlas` and `55-accessibility-audit` do not use the
# harness — the first is a pure-CPU terminal demo with no widgets to audit,
# the second *is* the audit, run as a demonstration. Both are excluded by
# name, here, rather than by a glob nobody can read: an exclusion that is
# not written down is an exclusion nobody can audit.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# Some rustc installs (this repository's own sandbox among them) link libstd
# dynamically, so the example binaries need the toolchain's lib dir on the
# loader path. CI's static-std toolchains have the same directory without
# needing it, so exporting unconditionally is harmless there and the gate
# runs in both places. Derived from the sysroot rather than hard-coded, and
# silently skipped when rustc is somewhere exotic.
sysroot="$(rustc --print sysroot 2>/dev/null || true)"
if [ -n "$sysroot" ]; then
    std_lib="$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib"
    if [ -d "$std_lib" ]; then
        export LD_LIBRARY_PATH="$std_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
fi

# The examples that use the harness: every directory under examples/features
# except the harness itself and the two named above.
excluded="54-image-mipmaps-atlas 55-accessibility-audit"

all_examples() {
    ls examples/features | grep -v '^harness$' | while read -r dir; do
        case " $excluded " in
            *" $dir "*) continue ;;
        esac
        printf '%s\n' "$dir"
    done
}

if [ "$#" -gt 0 ]; then
    wanted=""
    for number in "$@"; do
        match="$(all_examples | grep "^${number}-" || true)"
        [ -n "$match" ] || { echo "feature-a11y: no example numbered $number" >&2; exit 2; }
        wanted="$wanted $match"
    done
    examples="$wanted"
else
    examples="$(all_examples)"
fi

packages() {
    for dir in $examples; do
        printf ' -p feature-%s' "${dir#*-}"
    done
}

echo "==> building the audited examples"
# Symbols stripped, the same economy the sandbox runs under: an audit gate
# reads stderr, not stack traces, and 90 example binaries with symbols are
# gigabytes nobody debugs from.
CARGO_PROFILE_DEV_STRIP=symbols cargo build $(packages)

failed=()
for dir in $examples; do
    name="feature-${dir#*-}"
    echo "==> auditing $name ($dir)"
    if out="$(VIEWW_AUDIT=1 ./target/debug/$name 2>&1)"; then
        printf '%s\n' "$out" | grep '^audit:' || true
    else
        printf '%s\n' "$out" | sed 's/^/    /' >&2
        failed+=("$name")
    fi
done

if [ "${#failed[@]}" -gt 0 ]; then
    echo "feature-a11y: ${#failed[@]} example(s) failed the accessibility audit:" >&2
    printf '  %s\n' "${failed[@]}" >&2
    echo "  (run ci/check/feature-a11y.sh <number> to see one example's findings)" >&2
    exit 1
fi
echo "feature-a11y: every audited example passed (tree + text contrast)"
