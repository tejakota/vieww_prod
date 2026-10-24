#!/usr/bin/env bash
#
# Build this host's release artifacts and make them publishable.
#
#   ci/vieww artifacts [package.sh args]   # default: --installers
#   ci/vieww sums DIR [DIR...]             # merge per-OS SHA256SUMS into ./SHA256SUMS
#
# Writes into target/package/:
#   viewwstudio-<os>-<arch>.*   from packaging/package.sh --installers
#   SHA256SUMS                  one line per artifact, `sha256sum -c` format
#   MANIFEST.txt                version, source digest, git rev, toolchain,
#                               host, signing status — checklist rows G8.*
#
# The download page (packaging/site/index.html) links a single `SHA256SUMS`,
# so after building on all three OSes, merge them with `ci/vieww sums`.
#
# On Windows run from Git Bash, as packaging/package.sh requires.

set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

if command -v sha256sum >/dev/null; then SHA256=(sha256sum); else SHA256=(shasum -a 256); fi

if [[ "${1:-}" == --merge ]]; then
	shift
	(($# > 0)) || { echo "usage: ci/vieww sums DIR [DIR...]" >&2; exit 2; }
	cat "${@/%//SHA256SUMS}" | sort -k2 | uniq >SHA256SUMS
	dupes=$(awk '{print $2}' SHA256SUMS | sort | uniq -d)
	[[ -z "$dupes" ]] || { echo "conflicting checksums for: $dupes" >&2; exit 1; }
	echo "wrote $(pwd)/SHA256SUMS ($(wc -l <SHA256SUMS | tr -d ' ') artifacts)"
	exit 0
fi

cd "$root"
bash ci/check/release-clean-check.sh "$root"

args=("$@")
((${#args[@]} > 0)) || args=(--installers)
# FW-23: one timestamp for everything that honours it (dpkg-deb, tar/zip
# writers, the SBOM) — the commit's, so two builds of one commit agree.
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || date +%s)}"
export SOURCE_DATE_EPOCH
bash packaging/package.sh "${args[@]}"

out="$root/target/package"
cd "$out"
shopt -s nullglob
files=(viewwstudio-*.deb viewwstudio-*.AppImage viewwstudio-*.tar.gz viewwstudio-*.zip viewwstudio-*.msi viewwstudio-*.dmg)
((${#files[@]} > 0)) || { echo "artifacts: package.sh produced no artifacts in $out" >&2; exit 1; }
# FW-23 / G8.7: the SBOM of what the installers ship (the studio binary's
# normal-dependency closure, from Cargo.lock), listed in SHA256SUMS with them.
python3 "$root/ci/release/sbom.py" --manifest "$root/Cargo.toml" --bin viewwstudio -o viewwstudio.cdx.json
files+=(viewwstudio.cdx.json)
"${SHA256[@]}" "${files[@]}" | sed 's/ \*/  /' >SHA256SUMS

case "$(uname -s)" in
Darwin) os=macos ;;
MINGW* | MSYS* | CYGWIN*) os=windows ;;
*) os=linux ;;
esac
# What packaging/sign.sh actually achieved, per artifact (FW-06) — read back
# from the files, not assumed from which secrets were set.
if [[ "$os" == linux ]]; then
	signing="unsigned packages (Linux); integrity via SHA256SUMS + Sigstore provenance on release"
elif bash "$root/packaging/sign.sh" verify "${files[@]}" >signing.txt 2>&1; then
	signing="signed: $(grep -c '^signed' signing.txt) artifact(s)"
else
	signing="UNSIGNED: $(grep -E '^UNSIGNED' signing.txt | awk '{print $2}' | tr '\n' ' ')"
fi
{
	echo "version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
	echo "git_rev=$(git -C "$root" rev-parse HEAD 2>/dev/null || echo none)"
	echo "git_dirty=$(git -C "$root" status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
	echo "built=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "host_os=$os"
	echo "host=$(uname -a)"
	echo "rustc=$(rustc --version)"
	echo "cargo=$(cargo --version)"
	echo "toolchain_file=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")"
	echo "signing=$signing"
	echo "source_date_epoch=$SOURCE_DATE_EPOCH"
	echo "reproducible=partial (framework binaries: checked nightly by ci/release/repro-check.sh; installers: SOURCE_DATE_EPOCH honoured, bundled toolchain paths not normalised)"
	echo "sbom=viewwstudio.cdx.json (CycloneDX 1.5)"
	echo "# artifacts"
	cat SHA256SUMS
} >MANIFEST.txt

cat MANIFEST.txt
echo "==> $out/SHA256SUMS"
