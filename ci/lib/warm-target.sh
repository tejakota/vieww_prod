#!/usr/bin/env bash
#
# Point a cargo target directory at a cache that outlives the checkout.
#
#   warm-target.sh AREA LINK
#
# The machine jobs (where = "machine" in a ci.toml) check out a clean tree every run, so that files left by
# another branch are never compiled, and that clean wipes target/. On the
# self-hosted machine the build is the expensive part, so each area keeps
# its target directory under $FEATURE_CACHE/AREA and LINK (relative to the
# current directory) becomes a symlink to it. The second run of a branch
# rebuilds only what changed.
#
# Only for the self-hosted runner: it writes outside the work tree.

set -euo pipefail

area="$1" link="$2"
cache="${FEATURE_CACHE:-$HOME/.cache/vieww-feature-runs}"
cache="${cache/#\~/$HOME}"
dir="$cache/$area"
mkdir -p "$dir"

if [ -L "$link" ]; then
  rm "$link"
elif [ -e "$link" ]; then
  # A real directory (a checkout that kept it): leave it; the run is just cold.
  echo "warm-target: $link is a real directory; building there"
  exit 0
fi
ln -s "$dir" "$link"
echo "warm-target: $link -> $dir ($(du -sh "$dir" 2>/dev/null | cut -f1) cached)"
