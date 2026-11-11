#!/usr/bin/env bash
#
# A Valdi checkout at a pinned commit, with the comparison's Market Watch app
# dropped in.
#
#   compare/valdi/checkout.sh DEST [REF]
#
# Copies apps/market_watch (the Valdi app) into the checkout and gives it
# helloworld's launch theme and icon. Valdi itself is untouched: the app is
# built with Valdi's own release flags (release-flags.mjs), as it would ship.

set -euo pipefail
dest="${1:?usage: checkout.sh DEST [REF]}"
# The commit this comparison was written and type-checked against.
ref="${2:-e90a983749b384ba403c3bcdd40f7d0432404694}"
here="$(cd "$(dirname "$0")" && pwd)"

if [ ! -d "$dest/.git" ]; then
    git init -q "$dest"
    git -C "$dest" remote add origin https://github.com/Snapchat/valdi.git
fi
git -C "$dest" fetch -q --depth 1 origin "$ref"
git -C "$dest" checkout -q --force FETCH_HEAD
git -C "$dest" clean -qfdx -e .bazelrc.local
echo "valdi at $(git -C "$dest" rev-parse HEAD)"

cp -R "$here/apps/market_watch" "$dest/apps/"
cp -R "$dest/apps/helloworld/app_assets" "$dest/apps/market_watch/"
echo "market_watch added"
