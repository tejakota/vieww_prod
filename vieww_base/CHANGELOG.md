# Changelog

All notable changes to the vieww framework, Studio, audio stack and wright.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow the policy in [docs/release/RELEASING.md](docs/release/RELEASING.md)
(pre-1.0: a minor bump may break, a patch never does).

The release job (`.github/workflows/release-gate.yml`, `release`) copies the
section whose heading matches the tag into the GitHub Release notes, so every
tag needs a `## [x.y.z]` section here before it is pushed.

## [Unreleased]

### Fixed
- **The iOS example builds everywhere again**: its audio-check call is iOS-only (Framework checks on ad768f4 failed clippy and the cross type-check on it).
- **Device Farm audio pass, media keys**: falls back from `cmd media_session dispatch` to `media dispatch` to `input keyevent KEYCODE_MEDIA_PAUSE/PLAY` — Samsung's Android 12 answers the first with its usage text (Galaxy A73, run 40753842110).
- **GPU path on Qualcomm Adreno phones**: built-in shaders are now baked (and translated at run time) as one SPIR-V module per entry point. Since post.wgsl gained its D3D12 entry points, the whole-file module failed pipeline creation on Adreno ("Failed to link shaders", `VK_ERROR_UNKNOWN`): every GPU pass on the Adreno 506, 650 and 840 in Device Farm run 40720367260. A test now holds every baked module to exactly one `OpEntryPoint`.
- **Device Farm audio pass**: reads the media-session state in Android 13+'s `state=PLAYING(3)` form, sends media keys with `media dispatch` on Android 9, and logs each line once; the headphones receiver is registered on the application context so it no longer leaks with the activity.

### Added
- **Device Farm: audio pass** (AU-02, AU-03, AU-04): on every phone, the demo plays a tone and checks focus loss/duck/resume (Android, through a real competing focus request) or the session category, now-playing entry and interruption handling (iOS); the runners then check media-session pause/play, the foreground service, and that audio keeps playing with the screen off (Android, 60 s) or in the background (iOS, 30 s). The Device Farm APK and .ipa are built with background audio on.
- **Mobile audio: focus and interruptions** (AU-02, AU-03): `vieww_audio::focus` and `vieww_audio::session` — one focus/interruption state machine for both platforms; the engine pauses on the exact sample, ducks smoothly and resumes only what it paused. Android requests `AudioFocusRequest` focus (refused during a call: `PlayError::FocusDenied`); iOS sets the `AVAudioSession` category and follows interruptions, route changes and media-services resets. The device stream is not reopened during an interruption.
- **Mobile audio: background playback and lock-screen controls** (AU-04): `NowPlaying` and `RemoteCommand`, `MediaPlayer::set_metadata` / `poll_remote`; Android `MediaSession` + a `mediaPlayback` foreground service (`apk-add-code.sh --background-audio`), iOS `MPNowPlayingInfoCenter` / `MPRemoteCommandCenter` and an opt-in `UIBackgroundModes` (`background-audio = true` under `[package.metadata.vieww]`). `ci/mobile/lint-background-audio.sh` checks a built APK or plist.
- **CI: Mobile audio workflow**: the logic tests, an Android APK built and linted with the audio classes and service, and the iOS bridge type-checked and linked on macOS; started by hand.
- **Studio: undo for file operations** (ST-11): `Undo File Operation` / `Redo File Operation` reverse Explorer creates, renames and deletes without ever overwriting.
- **Studio: Copy Diagnostics** (ST-17): a bug-report block (version, toolchains, problems, session) that never includes code.
- **Studio: interface language** (ST-16): Spanish catalog for command, view and menu titles; `language` setting and `VIEWWSTUDIO_LANG`.
- **CI: Studio workflow** (ST-12): the Studio suite and screenshots on Linux, macOS and Windows, started by hand.
- **CI, every workflow started by hand** (FW-02): `pr-quick` (checks --quick,
  actionlint, wright sandbox escape suite), the release gate restored to the
  root, self-hosted hardware runs, audio CI; results of every run land on the
  `ci-results` branch.
- **Golden screenshots** (FW-14): 227 committed captures; `goldens` fails a PR
  on any byte difference and uploads the diff report.
- **Supply chain** (FW-23): cargo-deny + cargo-audit for both
  workspaces, CycloneDX SBOMs (`ci/release/sbom.py`, shipped beside the
  installers), a reproducibility check, `SOURCE_DATE_EPOCH` in
  packaging.
- **Publishing** (FW-01): `repository`/`homepage` on all 52 public crates,
  `ci/release/publish-check.sh` + `publish-order.txt`, a `publish` workflow
  (package check, dry-run verify, scaffold-outside-the-checkout build, and
  `cargo publish --workspace` when run on a tag with a token configured).
- **Releases** (FW-03, FW-06): `release-gate` run on a tag drafts a GitHub Release with
  installers, merged SHA256SUMS, SBOM, checklist and Sigstore build provenance.
- **Live scores** (AU-17): `vieww_music::live::LiveScore` plays a score
  through a running stream, sample-for-sample the offline mix.
- **Scene IR** (FW-25): `vieww_paint::ir` encodes and decodes a `Scene`
  as versioned bytes, for tools, fuzzers and bug reports.
- **Parity certificate** (FW-26): GPU suites write per-comparison parity
  rows; CI turns them into `parity-certificate-<leg>.json`.
- **Audio** (AU-20, AU-22, AU-24, AU-25): five new fuzz targets and a
  stable mutation suite; peak-hold and waterfall helpers; web autoplay
  resume, state events and Media Session; ReplayGain/R128 tags read,
  written and used by `MediaPlayer`; ABC export; `docs/guide/audio.md`.
- **wright** (WR-16, WR-19): kill-switch reach tested and shown in
  `wright doctor`; interviews measured, with a strict-format hint after
  repeated invalid drafts.
- **Notation and analysis** (AU-16, AU-22): MusicXML 4.0 export and
  import (`vieww_music::notation::musicxml`), YIN pitch detection and
  chroma (`vieww_audio::analysis`), key and melakarta estimation
  (`vieww_music::western::key`).
- **wright fork** (WR-12): `wright fork <run> --at N` branches a new
  project from a run at one event.
- **wright export targets** (WR-15): `--docker`, `--systemd`,
  `--gh-actions`, generated from the blueprint.
- **Loudness normalization** (AU-25): `MediaPlayer::set_normalization`
  (track mode to a target LUFS with a −1 dBTP peak cap, album mode with
  `album_gain_db`). Media wider than stereo now folds down with the
  standard coefficients (AU-23).
- **Audio patents** (AU-07): `vieww-audio/PATENTS.md` states each
  format's patent status; the AAC decoder is behind the default-on `aac`
  feature so distributors can leave it out.
- **Audio that plays** (AU-01, AU-11): `vieww_audio::Engine`, `EnginePlayer`
  (registered by `vieww-platform-winit` on desktop and Android, device opened on
  first play) and `MediaPlayer` (load / play / pause / seek / position /
  volume fades / looping / ended events).

### Security
- wright sandbox (WR-01, WR-02, WR-10, WR-16): PATH parents no longer expose
  `$HOME`; own PID namespace; seccomp-BPF filter; 0700 scratch; macOS
  `secret set` no longer leaks the value in argv; symlink-race-free writes;
  private wright home.
- wright prompts (WR-08): every inlined input is fenced and marked PRIVATE
  or UNTRUSTED from the Rule-of-Two taint set, with a data rule in every
  system prompt; an end-to-end test pins it on the bytes sent to the model.
- wright native Windows workers (WR-05): every worker runs in a Job object;
  a worker `.exe` also runs in an AppContainer whose only writable folder
  is its workspace copy, with internet access only.
- wright `http-post` bodies are capped at 1 MiB and its answers read to
  64 KiB; `http-read` checks the bytes as well as the content-type (WR-20).
- wright HTTP tools (WR-07): DNS-rebinding closed by pinning the SSRF-checked
  addresses into the connection.
- `clipboard-win` 5.4.1 (yanked) → 5.4.3; `tokio` → 1.54.1 (RUSTSEC-2026-0384,
  RUSTSEC-2026-0386).

### Fixed
- `vieww new` / Studio export outside a checkout wrote `vieww = "0.0.1"`, a
  version that never existed (checklist B16).
- wright: two processes creating the receipt signing key at once failed one
  of them.
- D3D12 scene renderer runs on WARP: atlas uploads honour the 256-byte
  row pitch, staging buffers outlive their copies, target states are
  tracked in one order, and the post shader is split so WARP runs it.
- The audio-plugin `Driver` heap-allocated on the audio thread every block
  once a block held a few hundred events (the standard stable sort's
  scratch); it now sorts in place. Clips and step sequencers gain
  `events_into` for allocation-free dispatch (AU-18).
- `84-particles-retarget` captured a different picture on every run.

## [0.1.0] - unreleased

Not yet tagged. The first tag cuts from a commit whose release gate is green
on every leg; see docs/release/RELEASING.md.
