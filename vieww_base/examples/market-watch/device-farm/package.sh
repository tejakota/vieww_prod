#!/usr/bin/env bash
#
# Build the Device Farm test package for the comparison: the runner and the
# Valdi APK, in the Appium-Python shape Device Farm validates (see
# vieww_base/ci/mobile/device-farm/package.sh, whose wheel handling this reuses).
#
#   VALDI_APK=dist/valdi-market-watch.apk examples/market-watch/device-farm/package.sh OUT.zip

set -euo pipefail
out="${1:?usage: package.sh OUT.zip}"
here="$(cd "$(dirname "$0")" && pwd)"
apk="${VALDI_APK:?VALDI_APK must point at the Valdi Market Watch APK}"
[ -f "$apk" ] || { echo "package.sh: no Valdi APK at $apk" >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/tests" "$work/wheelhouse"
cp "$here/run-android.sh" "$work/"
cp "$apk" "$work/valdi-market-watch.apk"

cat > "$work/tests/test_market_watch_package.py" <<'PY'
"""Device Farm requires a collectable pytest file in an Appium Python
package. The comparison does not use it: the custom test spec runs
run-android.sh directly."""


def test_package_is_the_market_watch_comparison():
    assert True
PY

python3 -m pip download --quiet --only-binary=:all: --dest "$work/wheelhouse" "pytest==8.3.3"
: > "$work/requirements.txt"
for wheel in "$work"/wheelhouse/*.whl; do
    name="$(basename "$wheel")"
    case "$name" in
        *-any.whl|*-linux_x86_64.whl) ;;
        *) echo "package.sh: $name is not a pure-Python wheel Device Farm accepts" >&2; exit 1 ;;
    esac
    dist="${name%%-*}"; rest="${name#*-}"; version="${rest%%-*}"
    echo "${dist//_/-}==$version" >> "$work/requirements.txt"
done

mkdir -p "$(dirname "$out")"
out="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
rm -f "$out"
(cd "$work" && zip -qr "$out" .)
echo "package.sh: $(unzip -l "$out" | tail -1 | awk '{print $2}') files -> $out"
