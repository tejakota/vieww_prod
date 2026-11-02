## Wright @VERSION@

Describe the outcome; agents build, check and deliver it.

### Downloads

| Platform | Installer | Portable |
|---|---|---|
| Windows 10/11 (x64) | `Wright-Setup-@VERSION@-x64.exe` — per-user, no admin prompt | `wright-@VERSION@-windows-x64.zip` |
| macOS 11+ (Apple silicon and Intel) | `Wright-@VERSION@-macos-universal.dmg` | `Wright-@VERSION@-macos-universal.app.zip` |
| Linux (x64, glibc 2.35+) | `wright_@VERSION@_amd64.deb` | `wright-@VERSION@-linux-x64.tar.gz` |

`SHA256SUMS.txt` lists a checksum for every file, and
`wright-v@VERSION@.manifest.json` is the signed update manifest — every
asset's SHA-256 and Ed25519 signature, the whole manifest signed with the
release key. `wright update check` verifies it against the key you pin in
config.kdl (`update url=... key=...`); it makes one plain GET of that one
file and nothing else.

### What works where

| | Linux | macOS | Windows |
|---|---|---|---|
| The window (splash, sessions, engine, settings, voice, files) | yes | yes | yes |
| Plans, the run store, history, schedules | yes | yes | yes |
| Local model servers over `http://` (Ollama, LM Studio, vLLM, the mock) | yes | yes | yes |
| Cloud model providers over `https://` | yes (OS OpenSSL) | yes (system curl) | yes (system curl) |
| Built-in models (Qwen3-1.7B, Whisper base), no key needed | yes | yes | yes |
| Sandboxed tools: shell, git, run-tests | yes (Landlock) | **refuse** | **refuse** |
| Keys in the OS keychain | `secret-tool` | Keychain | environment variables only (`wright secret env <ref>` names them) |

On macOS and Windows, HTTPS goes through the system's own curl, and the
sandbox refuses loudly rather than run unprotected (W-5b in
`docs/SPIKES.md`). The built-in models download on first use (1.9 GB in
all, once) — or ahead of time from Settings → Models & keys.

### First launch

- **macOS:** releases built with the signing workflow (WR-04) carry a
  Developer ID signature and a notarization ticket — they open like any
  signed app. A build from an unsigned draft run opens the old way:
  right-click Wright → Open the first time, or run
  `xattr -dr com.apple.quarantine /Applications/Wright.app`.
- **Windows:** signed releases carry an Authenticode signature, so no
  SmartScreen prompt. An unsigned draft build still trips it (More info →
  Run anyway) — check the manifest's signature before trusting any file
  that claims to be Wright.
- **Linux:** `sudo apt install ./wright_@VERSION@_amd64.deb`, then open Wright
  from your applications menu, or run `wright`.

### Updates

`wright update check` reads the channel you configured (one plain GET of
`wright-latest.manifest.json`, verified against your pinned release key)
and `wright update apply` downloads the asset for this machine, checks its
SHA-256 AND its Ed25519 signature, and swaps the binary in place with an
atomic rename — then asks you to restart. Wright never auto-executes a
downloaded installer. Update checks can be turned off:
`update check=false` in config.kdl.

Everything Wright remembers lives in `~/.wright`; uninstalling never touches
it or your projects.
