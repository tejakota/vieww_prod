#!/usr/bin/env bash
#
# Check a built app declares what background audio needs (AU-04).
#
#   ci/mobile/lint-background-audio.sh --plist path/to/App.app/Info.plist
#   ci/mobile/lint-background-audio.sh --apk path/to/app.apk
#
# iOS: UIBackgroundModes must contain "audio", or iOS suspends the app when
# the screen locks. Android: the manifest must declare
# dev.vieww.audio.PlaybackService as a foreground service of type
# mediaPlayback with FOREGROUND_SERVICE and FOREGROUND_SERVICE_MEDIA_PLAYBACK,
# and classes.dex must contain the class, or the process is frozen seconds
# after the screen goes off. Exits non-zero naming each missing piece.
#
# The audio code itself (focus, interruptions, lock-screen controls) is in
# vieww-audio's session.rs and the platform crate's audio_android.rs /
# audio_ios.rs; this only checks the packaging.

set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

case "${1:-}" in
    --plist)
        plist="${2:?usage: --plist Info.plist}"
        python3 -I - "$plist" <<'PY'
import plistlib, sys
path = sys.argv[1]
with open(path, "rb") as f:
    info = plistlib.load(f)
modes = info.get("UIBackgroundModes", [])
if "audio" not in modes:
    print("lint-background-audio: %s: UIBackgroundModes has no 'audio' (has %r)" % (path, modes))
    sys.exit(1)
print("lint-background-audio: %s: UIBackgroundModes includes audio" % path)
PY
        ;;
    --apk)
        apk="${2:?usage: --apk app.apk}"
        python3 -I "$here/apk_add_code.py" --check-background-audio "$apk"
        echo "lint-background-audio: $apk: playback service, type and permissions declared"
        ;;
    *)
        sed -n '3,6p' "$0" >&2
        exit 2
        ;;
esac
