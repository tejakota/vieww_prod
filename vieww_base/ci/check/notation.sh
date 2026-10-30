#!/usr/bin/env bash
#
# Notation interchange, checked by the tools that read it (AU-16).
#
#     ci/check/notation.sh
#
# Exports every demo piece (vieww-music's notation_export example), then:
#   * validates each MusicXML file against the official MusicXML 4.0 XSD
#     (w3c/musicxml at the pinned tag, fetched with git) with xmllint —
#     what MuseScore, Finale, Dorico and Sibelius import;
#   * engraves each LilyPond file with `lilypond` and fails on any error
#     (warnings about bar checks or unknown input are errors here too).
# Needs git, xmllint (libxml2-utils) and lilypond.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
tag="${VIEWW_MUSICXML_TAG:-v4.0}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for tool in git xmllint lilypond; do
    command -v "$tool" >/dev/null || { echo "notation: $tool is missing" >&2; exit 2; }
done
git clone -q --depth 1 --branch "$tag" https://github.com/w3c/musicxml "$work/schema-repo"
# The XSD imports xml.xsd and xlink.xsd from musicxml.org; the repo has
# both beside it, so point the imports there (no network during checks).
sed -i -e 's|http://www.musicxml.org/xsd/xml.xsd|xml.xsd|' \
    -e 's|http://www.musicxml.org/xsd/xlink.xsd|xlink.xsd|' "$work/schema-repo/schema/musicxml.xsd"
cargo run -q -p vieww-music --example notation_export -- "$work/out"
fail=0
for f in "$work"/out/*.musicxml; do
    if xmllint --noout --nonet --schema "$work/schema-repo/schema/musicxml.xsd" "$f" 2>"$work/xsd.log"; then
        echo "notation: $(basename "$f") is valid MusicXML ($tag)"
    else
        cat "$work/xsd.log" >&2
        fail=1
    fi
done
for f in "$work"/out/*.ly; do
    if (cd "$work/out" && lilypond -s -dno-point-and-click -o "${f%.ly}" "$f") >"$work/ly.log" 2>&1 &&
        ! grep -qiE "error|warning" "$work/ly.log"; then
        echo "notation: $(basename "$f") engraves ($(basename "${f%.ly}").pdf)"
    else
        cat "$work/ly.log" >&2
        fail=1
    fi
done
exit "$fail"
