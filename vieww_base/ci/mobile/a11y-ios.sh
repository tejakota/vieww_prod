#!/usr/bin/env bash
#
# The iOS half of the accessibility suite — the counterpart of
# `a11y-android.sh`, and like it, honest about what a given machine can
# and cannot settle.
#
#   ci/mobile/a11y-ios.sh --check        # what this machine can run, and what is missing
#   ci/mobile/a11y-ios.sh --sim          # booted simulator: install, launch, read the tree
#   ci/mobile/a11y-ios.sh --xcuitest     # build and run the XCUITest bundle (needs a project, see below)
#
# Needs macOS with Xcode for anything beyond --check. This script never
# builds the app: `ci/mobile/ios-app.sh --sim` does that, and this script
# consumes its bundle.
#
# # What this can and cannot settle
#
# **The tree, not the ears.** `ios_accessibility.rs` publishes the
# semantics tree as `UIAccessibilityElement`s, and a UI test reads the same
# tree VoiceOver consumes — so a run can prove "the button is present,
# labelled, traited, enabled", which is the half of the contract PENDING
# §3.6 records as missing. What no automated run here proves is that
# VoiceOver *spoke*: VoiceOver itself cannot be driven from XCUITest, and
# the manual session recorded beside the TalkBack record (PENDING §3.6's
# own wording) remains the other half.
#
# **The XCUITest half runs from a generated project.** The repository carries
# `ci/mobile/xcuitest/project.yml` — the spec — and `xcodegen` writes the
# .xcodeproj from it on demand (the generated project is not committed; see
# that file's header for why the app target has no sources and copies the
# cargo-built bundle instead). `--xcuitest` below installs the Chinese
# (Pinyin) keyboard the CJK typing test needs, generates the project, and
# runs `xcodebuild test` against the booted simulator.

set -uo pipefail

mode="${1:---check}"

have() { command -v "$1" >/dev/null 2>&1; }

report() { printf 'a11y-ios: %s\n' "$*"; }

fail() { printf 'a11y-ios: FAIL — %s\n' "$*" >&2; exit 1; }

case "$mode" in
--check)
        report "checking what this machine can run"
        if ! have xcrun; then
                report "xcrun not found — no Apple toolchain; the Rust bridge still type-checks:"
                report "  cargo check --target aarch64-apple-ios -p vieww-platform-winit"
                exit 0
        fi
        report "xcrun present — the simctl half can run: ios-app.sh --sim, then this script --sim"
        if have xcodebuild; then
                report "xcodebuild present — the XCUITest half can run (--xcuitest)"
                if have xcodegen; then
                        report "xcodegen present — the project generates from ci/mobile/xcuitest/project.yml"
                else
                        report "xcodegen not found — install it (brew install xcodegen) for --xcuitest"
                fi
        else
                report "xcodebuild not found — the XCUITest half cannot run here at all"
        fi
        report "record: no VoiceOver session has ever been heard on vieww (PENDING 3.6)"
        exit 0
        ;;
--sim)
        have xcrun || fail "--sim needs macOS with Xcode (xcrun)"
        booted="$(xcrun simctl list devices booted | grep -c "Booted" || true)"
        [ "$booted" -ge 1 ] || fail "no booted simulator: open Simulator, or xcrun simctl boot '<device>'"
        # The bundle ios-app.sh --sim produced and installed. The launch is the
        # test's setup; what the tree holds afterwards is what the XCUITest
        # bundle would assert in detail.
        report "launching the installed vieww demo on the booted simulator"
        xcrun simctl launch booted dev.vieww.demo ||
                fail "dev.vieww.demo not installed — run ci/mobile/ios-app.sh --sim first"
        sleep 3
        report "the app is up; what the tree holds is asserted by the XCUITest set"
        report "  (ci/mobile/xcuitest/ViewwA11yTests.swift) — see --xcuitest and the header"
        report "simctl half done; this run alone does not move PENDING 3.6 without a"
        report "  recorded session or a green XCUITest run beside it"
        exit 0
        ;;
--xcuitest)
        have xcrun || fail "--xcuitest needs macOS with Xcode (xcrun)"
        have xcodebuild || fail "--xcuitest needs xcodebuild (full Xcode, not just the command-line tools)"
        have xcodegen || fail "--xcuitest needs xcodegen — brew install xcodegen"
        device="${VIEWW_SIM_DEVICE:-booted}"
        if [ "$device" = "booted" ]; then
                xcrun simctl list devices booted | grep -q Booted \
                        || fail "no booted simulator: open Simulator, or xcrun simctl boot '<device>'"
        fi
        # The app bundle the suite launches. Built, not rebuilt: the project's
        # app target copies this directory in place, so the test drives the
        # exact bytes ios-app.sh shipped.
        app="$(cd "$(dirname "$0")" && pwd)/../../../target/ios/vieww.app"
        [ -d "$app" ] || fail "no app bundle at target/ios/vieww.app — run ci/mobile/ios-app.sh --sim --bundle-only first"
        report "app bundle found: $app"

        # The Chinese (Simplified, Pinyin) keyboard, for the CJK typing test.
        # A UI test cannot install keyboards on its own host — this is the one
        # thing the suite needs done to the simulator before it starts. The
        # domain is the one the Settings app reads on the simulator; English
        # stays first so the launch state is unchanged for the other tests.
        # A restart is what makes the setting land: the keyboard cache is read
        # once, at springboard start, not per keystroke.
        report "installing the Chinese (Pinyin) keyboard on the simulator"
        xcrun simctl spawn "$device" defaults write com.apple.Preferences AppleKeyboards \
                -array 'en_US@sw=QWERTY;hw=Automatic' 'zh_Hans@sw=Pinyin;hw=Automatic' \
                || fail "could not write the keyboard defaults on the simulator"
        xcrun simctl shutdown "$device" || true
        xcrun simctl boot "$device"
        xcrun simctl bootstatus "$device" -b

        here="$(cd "$(dirname "$0")/xcuitest" && pwd)"
        report "generating the Xcode project from project.yml"
        (cd "$here" && xcodegen generate) \
                || fail "xcodegen could not generate the project from ci/mobile/xcuitest/project.yml"

        report "running the XCUITest suite (this builds the test bundle; the app is the prebuilt bundle)"
        xcodebuild test \
                -project "$here/ViewwA11y.xcodeproj" \
                -scheme ViewwA11y \
                -destination "id=$device" \
                || fail "xcodebuild test failed — a red suite is a finding about the bridge, not a flake (see the suite's header)"
        report "xcuitest half done: the tree VoiceOver reads is asserted. Ears on a manual"
        report "session remain the other half of PENDING 3.6 — a green suite is tree-level evidence"
        exit 0
        ;;
*)
        printf 'usage: %s [--check|--sim|--xcuitest]\n' "$0" >&2
        exit 2
        ;;
esac
