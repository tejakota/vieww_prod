#!/usr/bin/env bash
#
# Platform code signing (FW-06), applied when the credentials exist and stated
# plainly when they do not. Called by packaging/package.sh; usable by hand.
#
#   packaging/sign.sh mac-app  "<path>/vieww Studio.app"   # every Mach-O, then the bundle
#   packaging/sign.sh mac-dmg  <file.dmg>                   # sign, notarise, staple
#   packaging/sign.sh win-tree <dir>                        # every .exe/.dll before wix
#   packaging/sign.sh win      <file.msi|exe>...            # Authenticode + RFC 3161 timestamp
#   packaging/sign.sh apk      <file.apk|aab>...            # apksigner / jarsigner
#   packaging/sign.sh verify   <file>...                    # check what is signed
#
# Credentials (environment; in CI, repository secrets):
#
#   macOS    APPLE_DEVELOPER_ID_P12 (base64 .p12), APPLE_P12_PASSWORD,
#            APPLE_SIGNING_IDENTITY (optional; default: the p12's first
#            "Developer ID Application"), APPLE_NOTARY_KEY (base64 .p8),
#            APPLE_NOTARY_KEY_ID, APPLE_NOTARY_ISSUER
#   Windows  WINDOWS_CERT_PFX (base64 .pfx), WINDOWS_CERT_PASSWORD,
#            WINDOWS_TIMESTAMP_URL (default http://timestamp.digicert.com)
#   Android  ANDROID_KEYSTORE (base64), ANDROID_KEYSTORE_PASSWORD,
#            ANDROID_KEY_ALIAS, ANDROID_KEY_PASSWORD (default: the store's)
#
# Missing credentials are not an error: the step prints "unsigned: <why>"
# and exits 0, so the open-source pipeline still produces installers. A
# signing step that RUNS and fails is an error.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cmd="${1:-}"
shift || true

work="$(mktemp -d)"
cleanup() {
	if [[ -n "${keychain:-}" ]]; then security delete-keychain "$keychain" >/dev/null 2>&1 || true; fi
	rm -rf "$work"
}
trap cleanup EXIT

unsigned() {
	echo "    unsigned: $*"
	exit 0
}
b64d() { if base64 --help 2>&1 | grep -q -- '--decode'; then base64 --decode; else base64 -D; fi; }

# ── macOS ───────────────────────────────────────────────────────────────────

mac_identity() {
	[[ -n "${APPLE_DEVELOPER_ID_P12:-}" && -n "${APPLE_P12_PASSWORD:-}" ]] ||
		unsigned "macOS: APPLE_DEVELOPER_ID_P12 / APPLE_P12_PASSWORD not set (Developer ID, \$99/yr)"
	keychain="$work/sign.keychain-db"
	local kpass
	kpass="$(openssl rand -hex 16)"
	printf '%s' "$APPLE_DEVELOPER_ID_P12" | b64d >"$work/id.p12"
	security create-keychain -p "$kpass" "$keychain"
	security set-keychain-settings -lut 3600 "$keychain"
	security unlock-keychain -p "$kpass" "$keychain"
	security import "$work/id.p12" -k "$keychain" -P "$APPLE_P12_PASSWORD" -T /usr/bin/codesign
	security set-key-partition-list -S apple-tool:,apple: -s -k "$kpass" "$keychain" >/dev/null
	# shellcheck disable=SC2046 # the list is word-split on purpose
	security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
	identity="${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning "$keychain" |
		sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1)}"
	[[ -n "$identity" ]] || { echo "sign: no Developer ID Application identity in the p12" >&2; exit 1; }
}

mac_sign_one() {
	codesign --force --timestamp --options runtime \
		--entitlements "$here/macos.entitlements" \
		--keychain "$keychain" --sign "$identity" "$1"
}

mac_app() {
	local app="${1:?mac-app needs the .app}"
	mac_identity
	# Inside out: every Mach-O in the bundle (the bundled rustc, cargo, linker
	# and dylibs are executables too), then the bundle itself. --deep is not
	# used: Apple documents it as unsuitable for signing, only for verifying.
	local n=0
	while IFS= read -r -d '' f; do
		if file -b "$f" | grep -q 'Mach-O'; then
			mac_sign_one "$f"
			n=$((n + 1))
		fi
	done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.so' \) -print0)
	mac_sign_one "$app"
	codesign --verify --strict --deep --verbose=2 "$app"
	echo "    signed: $app ($n nested Mach-O files) as $identity"
}

mac_dmg() {
	local dmg="${1:?mac-dmg needs the .dmg}"
	mac_identity
	codesign --force --timestamp --keychain "$keychain" --sign "$identity" "$dmg"
	if [[ -z "${APPLE_NOTARY_KEY:-}" || -z "${APPLE_NOTARY_KEY_ID:-}" || -z "${APPLE_NOTARY_ISSUER:-}" ]]; then
		echo "    signed, NOT notarised: APPLE_NOTARY_KEY / _KEY_ID / _ISSUER not set (Gatekeeper will warn)"
		return 0
	fi
	printf '%s' "$APPLE_NOTARY_KEY" | b64d >"$work/notary.p8"
	xcrun notarytool submit "$dmg" --key "$work/notary.p8" \
		--key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER" --wait --timeout 1h |
		tee "$work/notary.txt"
	grep -q 'status: Accepted' "$work/notary.txt" || {
		echo "sign: notarisation was not accepted" >&2
		exit 1
	}
	xcrun stapler staple "$dmg"
	spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
	echo "    signed, notarised and stapled: $dmg"
}

# ── Windows ─────────────────────────────────────────────────────────────────

win_tool() {
	[[ -n "${WINDOWS_CERT_PFX:-}" && -n "${WINDOWS_CERT_PASSWORD:-}" ]] ||
		unsigned "Windows: WINDOWS_CERT_PFX / WINDOWS_CERT_PASSWORD not set (OV/EV Authenticode certificate)"
	printf '%s' "$WINDOWS_CERT_PFX" | b64d >"$work/cert.pfx"
	signtool="$(command -v signtool.exe || command -v signtool ||
		find "/c/Program Files (x86)/Windows Kits/10/bin" -name signtool.exe -path '*x64*' 2>/dev/null | sort | tail -1)"
	[[ -n "$signtool" ]] || { echo "sign: signtool not found (Windows SDK)" >&2; exit 1; }
}

win_sign() {
	"$signtool" sign /fd sha256 /f "$(cygpath -w "$work/cert.pfx" 2>/dev/null || echo "$work/cert.pfx")" \
		/p "$WINDOWS_CERT_PASSWORD" /tr "${WINDOWS_TIMESTAMP_URL:-http://timestamp.digicert.com}" /td sha256 "$@"
	"$signtool" verify /pa "$@" >/dev/null
}

win_tree() {
	local dir="${1:?win-tree needs a directory}"
	win_tool
	local files=()
	while IFS= read -r -d '' f; do files+=("$f"); done < <(find "$dir" -type f \( -iname '*.exe' -o -iname '*.dll' \) -print0)
	((${#files[@]})) || return 0
	# signtool takes many files per call; batches keep the command line short.
	local i
	for ((i = 0; i < ${#files[@]}; i += 50)); do win_sign "${files[@]:i:50}"; done
	echo "    signed: ${#files[@]} executable(s) under $dir"
}

win_files() {
	(($#)) || { echo "sign: win needs files" >&2; exit 2; }
	win_tool
	win_sign "$@"
	echo "    signed: $*"
}

# ── Android ─────────────────────────────────────────────────────────────────

apk_files() {
	(($#)) || { echo "sign: apk needs files" >&2; exit 2; }
	[[ -n "${ANDROID_KEYSTORE:-}" && -n "${ANDROID_KEYSTORE_PASSWORD:-}" && -n "${ANDROID_KEY_ALIAS:-}" ]] ||
		unsigned "Android: ANDROID_KEYSTORE / _PASSWORD / ANDROID_KEY_ALIAS not set (release upload key)"
	printf '%s' "$ANDROID_KEYSTORE" | b64d >"$work/release.jks"
	local keypass="${ANDROID_KEY_PASSWORD:-$ANDROID_KEYSTORE_PASSWORD}" f
	for f in "$@"; do
		case "$f" in
		*.aab)
			jarsigner -keystore "$work/release.jks" -storepass "$ANDROID_KEYSTORE_PASSWORD" \
				-keypass "$keypass" -sigalg SHA256withRSA -digestalg SHA-256 "$f" "$ANDROID_KEY_ALIAS"
			jarsigner -verify "$f" >/dev/null
			;;
		*)
			apksigner sign --ks "$work/release.jks" --ks-pass "pass:$ANDROID_KEYSTORE_PASSWORD" \
				--ks-key-alias "$ANDROID_KEY_ALIAS" --key-pass "pass:$keypass" "$f"
			apksigner verify --verbose "$f" >/dev/null
			;;
		esac
		echo "    signed: $f"
	done
}

# ── verification (no credentials needed) ────────────────────────────────────

verify() {
	local f rc=0
	for f in "$@"; do
		case "$f" in
		*.app | *.dmg)
			if codesign --verify --strict --verbose=1 "$f" 2>/dev/null; then echo "signed    $f"; else echo "UNSIGNED  $f"; rc=1; fi
			;;
		*.msi | *.exe | *.dll)
			if command -v osslsigncode >/dev/null && osslsigncode verify -in "$f" >/dev/null 2>&1; then
				echo "signed    $f"
			elif command -v signtool >/dev/null && signtool verify /pa "$f" >/dev/null 2>&1; then
				echo "signed    $f"
			else
				echo "UNSIGNED  $f"
				rc=1
			fi
			;;
		*.apk)
			if apksigner verify "$f" >/dev/null 2>&1 && ! apksigner verify --print-certs "$f" | grep -q 'CN=Android Debug'; then
				echo "signed    $f"
			else
				echo "UNSIGNED  $f (or debug key)"
				rc=1
			fi
			;;
		*) echo "n/a       $f" ;;
		esac
	done
	return $rc
}

case "$cmd" in
mac-app) mac_app "$@" ;;
mac-dmg) mac_dmg "$@" ;;
win-tree) win_tree "$@" ;;
win) win_files "$@" ;;
apk) apk_files "$@" ;;
verify) verify "$@" ;;
*)
	sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
	;;
esac
