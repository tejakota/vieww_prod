#!/usr/bin/env bash
#
# The Linux half of the accessibility suite — what a desktop screen reader
# can be shown to see, on a machine that has one.
#
#   ci/check/a11y-linux.sh --check     # what this machine can run, and what is missing
#   ci/check/a11y-linux.sh --dry-run   # the assertion engine over the committed fixture — runs anywhere
#   ci/check/a11y-linux.sh --live      # the real AT-SPI bus, on a desktop session (self-hosted)
#
# # What each half settles
#
# **--dry-run** regenerates the fixture from the real tree — `cargo test -p
# vieww-platform-winit` builds `examples/shared/screen.rs` (the demo every
# entry point runs), hands it to the same `tree_update` the platform uses,
# and writes the JSON — then asserts two things: that the regenerated tree
# still matches the committed one structurally (roles, names, ids, shape;
# bounds are excluded, they move with the machine's fonts), and that
# `a11y_atspi.py`'s assertion engine passes over it and *fails* over a
# deliberately broken tree (`--self-test`). That proves the harness itself,
# and vieww's side of the AccessKit seam, on any machine with cargo.
#
# **--live** is the other half: the AT-SPI bus, read with pyatspi, from the
# demo running on a real desktop session. This is the leg that exercises
# AccessKit's Unix adapter — code this repository does not own — so it runs
# where a desktop exists: a self-hosted runner or an operator's machine. The
# first live run also records the tree it saw (`--record`), which is what
# confirms (or corrects) the AT-SPI role-name column `a11y_atspi.py` carries.
#
# # What no half of this settles
#
# Ears. Nothing here proves Orca *spoke*, exactly as the Android and iOS
# suites record for TalkBack and VoiceOver: a green tree is "the reader can
# find and name every control", and a manual session beside it is the other
# half of the contract PENDING §3.6 writes down.

set -euo pipefail

mode="${1:---check}"
here="$(cd "$(dirname "$0")" && pwd)"
fixture="$here/testdata/a11y-atspi-tree.json"

report() { printf 'a11y-linux: %s\n' "$*"; }
fail() { printf 'a11y-linux: FAIL — %s\n' "$*" >&2; exit 1; }

case "$mode" in
--check)
        report "checking what this machine can run"
        command -v cargo >/dev/null 2>&1 \
                && report "cargo present — the dry-run can run (--dry-run)" \
                || report "cargo not found — the dry-run cannot run here"
        if python3 -c "import pyatspi" >/dev/null 2>&1; then
                report "pyatspi importable — the live leg can run (--live)"
        else
                report "pyatspi not importable — the live leg needs it (Debian: python3-pyatspi)"
        fi
        if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
                report "a display is present — the live leg has a desktop session to read"
        else
                report "no DISPLAY/WAYLAND_DISPLAY — the live leg has no session to read here"
        fi
        report "the dry-run half is what CI runs; the live half is a self-hosted leg"
        exit 0
        ;;

--dry-run)
        [ -f "$fixture" ] || fail "no committed fixture at ci/check/testdata/a11y-atspi-tree.json"

        # Regenerate the fixture from the live code. The test itself carries
        # the real assertions (labels, focusability, connectivity — see
        # src/a11y/atspi_fixture.rs); this step is what keeps the committed
        # JSON from rotting silently behind a code change. `--lib` because the
        # name also matches `tests/wait_loop.rs`'s scenario runner, which
        # answers an unknown scenario by panicking.
        report "regenerating the fixture from the real tree (cargo test a11y::atspi_fixture)"
        regen="$(mktemp)"
        trap 'rm -f "$regen"' EXIT
        # The lean profile the repository's own CI runs use: no debug info, no
        # incremental, symbols stripped. Not just thrift — the fixture test
        # links the whole platform crate, and the fat default profile is
        # gigabytes a small disk (this repository's own sandbox among them)
        # cannot pay for a dump that reads stderr, not stack traces.
        VIEWW_ATSPI_FIXTURE="$regen" \
        CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
                cargo test -p vieww-platform-winit --lib a11y::atspi_fixture -- --nocapture \
                || fail "the fixture test itself failed — the tree is broken before the harness sees it"

        # Structural comparison: everything except bounds, which move with the
        # machine's font metrics. The accessibility contract is roles, names,
        # ids and shape — that is what a drifted fixture must be caught on.
        report "comparing the regenerated tree against the committed fixture (structure, not bounds)"
        strip_bounds() { python3 -c '
import json, sys
doc = json.load(open(sys.argv[1]))
for node in doc["nodes"]:
    node.pop("bounds", None)
json.dump(doc, sys.stdout, indent=2, sort_keys=True)
' "$1"; }
        if ! diff <(strip_bounds "$regen") <(strip_bounds "$fixture") > /tmp/a11y-fixture-drift.txt; then
                cat /tmp/a11y-fixture-drift.txt >&2
                fail "the committed AT-SPI fixture drifted from the real tree — regenerate it \
(cargo test with VIEWW_ATSPI_FIXTURE=ci/check/testdata/a11y-atspi-tree.json) and review the diff"
        fi
        report "fixture matches the real tree structurally"

        # The engine over the committed fixture, and over a broken one.
        python3 "$here/a11y_atspi.py" --tree "$fixture" \
                || fail "the assertion engine failed over the committed fixture"
        python3 "$here/a11y_atspi.py" --self-test \
                || fail "the assertion engine's self-test failed — it can no longer be trusted to fire"
        report "dry-run done: the engine passes over the real tree and fires on broken ones"
        exit 0
        ;;

--live)
        command -v python3 >/dev/null 2>&1 || fail "python3 not found"
        python3 -c "import pyatspi" >/dev/null 2>&1 \
                || fail "pyatspi is not importable (Debian: python3-pyatspi)"
        [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] \
                || fail "no desktop session (DISPLAY/WAYLAND_DISPLAY unset) — this is the self-hosted leg"
        command -v cargo >/dev/null 2>&1 || fail "cargo not found — the demo cannot be run"

        report "running the demo (examples/desktop.rs) in the background"
        cargo run -p vieww-platform-winit --example desktop &
        demo=$!
        trap 'kill "$demo" 2>/dev/null || true' EXIT
        # The tree publishes on the first laid-out frame; a cold build can
        # take minutes, a warm one seconds, so poll rather than sleep a guess.
        # Each attempt's output is kept: on success it is the report, and on
        # timeout the last one is the reason nothing answered.
        report "waiting for the app to appear on the bus (name: desktop)"
        last="$(mktemp)"
        trap 'rm -f "$last"; kill "$demo" 2>/dev/null || true' EXIT
        for _ in $(seq 1 120); do
                if python3 "$here/a11y_atspi.py" --live desktop >"$last" 2>&1; then
                        cat "$last"
                        report "live leg done: the AT-SPI tree is asserted. Record it with --record"
                        report "on the next run — that recording is what confirms the python's role table"
                        exit 0
                fi
                sleep 2
        done
        cat "$last" >&2
        fail "the demo never answered on the AT-SPI bus (4 min) — see the output above"
        ;;

*)
        printf 'usage: %s [--check|--dry-run|--live]\n' "$0" >&2
        exit 2
        ;;
esac
