#!/usr/bin/env bash
#
# Drive the Wright Flow prototype through its main paths with
# viewwstudio-cli and keep a GIF of each, so a feature run (or a person)
# can see the screens move without opening a window.
#
#   apps/wright/prototypes/wright-flow/walkthrough.sh [OUT_DIR]
#
# Run from vieww_base/ after `cargo build --release -p viewwstudio --bin
# viewwstudio-cli`. OUT_DIR defaults to target/wright-flow-walkthrough.
# Exits non-zero if any walkthrough fails to play (a label that is not on
# screen, a compile error), which is the regression this exists to catch.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
base="$(cd "$here/../../../.." && pwd)"
cli="$base/target/release/viewwstudio-cli"
out="${1:-$base/target/wright-flow-walkthrough}"
screen="$here/src/screens/home.rs"
mkdir -p "$out"

[ -x "$cli" ] || { echo "viewwstudio-cli is not built: cargo build --release -p viewwstudio --bin viewwstudio-cli" >&2; exit 2; }

run() {
  local name="$1" secs="$2"; shift 2
  echo "── $name"
  "$cli" animate "$screen" --width 1280 --height 800 --duration "$secs" --fps 5 "$@" --out "$out/$name.gif"
}

# 1. Start → draft → confirm → run with pause/resume → approval → export.
run run-flow 24s \
  --do 'tap 965,294 @1s' --do 'tap "Stop and transcribe" @2.4s' \
  --do 'tap 258,294 @4.8s' --do 'tap "Add files…" @5.4s' --do 'tap 1019,294 @6.2s' \
  --do 'tap "Build this" @10.4s' --do 'tap "Approve & run" @11.6s' \
  --do 'tap "Pause" @13.2s' --do 'tap "Resume" @14.4s' \
  --do 'tap "Allow" @18.6s' --do 'tap "Export" @22s'

# 2. A delivered session: build story, receipt, export, overview, engine, stop.
run result-and-engine 15s \
  --do 'tap "Weekly Sales Report" @1s' --do 'tap "How it was built" @2.2s' \
  --do 'tap "See the receipt" @3.8s' --do 'tap "Copy the report" @5.4s' --do 'tap "Done" @6.4s' \
  --do 'tap "Export" @7.2s' --do 'tap "Docker" @8.2s' \
  --do 'tap "All sessions" @9.6s' --do 'tap "Engine" @11.2s' \
  --do 'tap "Stop everything" @12.6s' --do 'tap "Start again" @14s'

# 3. Close into Saved and undo, reopen, every Settings tab, light, help.
run sessions-and-settings 21s \
  --do 'tap "All sessions" @0.8s' --do 'tap 802,350 @2s' --do 'tap "Undo" @2.8s' --do 'tap "Word Count CLI" @4s' \
  --do 'tap 1193,31 @5.4s' --do 'tap "Set key" @6.4s' --do 'tap "Save to keychain" @7.2s' \
  --do 'tap "Download now" @8.2s' --do 'tap "Add provider" @9.6s' --do 'tap "Cancel" @10.4s' \
  --do 'tap "Agents & tools" @11s' --do 'tap "Network" @12s' --do 'tap "Choose file…" @12.8s' \
  --do 'tap "Fleet" @13.8s' --do 'tap "Organization" @14.6s' --do 'tap "Appearance" @15.4s' \
  --do 'tap "Ember" @16.2s' --do 'tap "Light" @17s' --do 'tap 1061,147 @18s' \
  --do 'tap 1241,31 @18.8s' --do 'tap "Done" @20.4s'

echo "walkthroughs in $out"
