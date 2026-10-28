# AWS Device Farm

The on-device suite (`ci/mobile/device-suite.sh`) on real phones in Device
Farm, from the manual-only workflow `.github/workflows/device-farm.yml`
(pending placement: `.github/workflows-pending/`).

| File | Runs where | Does |
|---|---|---|
| `../device_farm.py` | GitHub runner (or your shell) | picks phones, uploads, schedules, waits, downloads, writes `SUMMARY.md` / `summary.json` |
| `package.sh` | GitHub runner | builds the test package: an Appium-Python-shaped zip Device Farm will accept, carrying the scripts below |
| `testspec-android.yml` | Device Farm | the custom test spec: runs `run-android.sh` |
| `run-android.sh` | Device Farm Android host, per phone | cold start x3 per path (`am start -W`), the device suite on the CPU and GPU paths, `dumpsys meminfo`, screenshot, logcat → `result.txt` |

## Secrets the workflow reads

- `DEVICE_FARM_PROJECT_ARN` — the project's ARN (Device Farm console → project → settings).
- `AWS_ROLE_ARN` (GitHub OIDC, preferred) **or** `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY`.
  The identity needs Device Farm access to the project (`AWSDeviceFarmFullAccess` is the simple choice).

Device Farm lives in `us-west-2` only; the workflow pins it.

## By hand

```console
export DEVICE_FARM_PROJECT_ARN=arn:aws:devicefarm:us-west-2:...:project:...
python3 ci/mobile/device_farm.py --apk vieww-devicefarm.apk --out df-out            # low / mid / high
python3 ci/mobile/device_farm.py --apk vieww-devicefarm.apk --out df-out --devices "Pixel 8,Galaxy A14"
```

The APK must carry a `classes.dex` (`ci/mobile/apk-add-code.sh`), as for any
device cloud; the workflow's `apk` job builds it that way.

## iPhones

```console
python3 ci/mobile/device_farm.py --platform ios --ipa dist/vieww.ipa --out df-ios
```

`testspec-ios.yml` runs `run-ios.sh` on Device Farm's macOS host: the app is
launched with `xcrun devicectl … --console` (CPU path, then `VIEWW_GPU=1`), its
tap targets are read from `__VIEWW_TAPS_PT__` and tapped through Appium's
XCUITest driver by `tap_ios.py` (stdlib only), and the suite's verdict and
frame report are read from the console — with the copy the app writes into its
own `Documents/` as a second channel. Start-up is the app's own trace: the
first presented frame, from `main`. **Not yet run on a real Device Farm
iPhone**; the first run is what verifies these scripts.

The .ipa is built by the workflow's `ipa` job (`ci/mobile/ios-app.sh --ipa`,
MoltenVK bundled), signed with your identity when the `IOS_*` secrets exist.

## What it does not cover

Energy (Device Farm exposes no power data) and TalkBack/VoiceOver by ear (done
by hand on the Redmi).

## The audio pass (AU-02, AU-03, AU-04)

After the rendering passes, each phone runs an audio pass. The demo is
launched with `debug.vieww.audio=1` (Android) or `VIEWW_AUDIO=1` (iOS) and
plays a looping tone through `MediaPlayer`, with lock-screen metadata.
`crates/vieww-platform-winit/src/audio_check.rs` checks on the phone and
prints `__VIEWW_AUDIO__` lines.

- **Android, in the app:** a competing transient focus request pauses
  playback and freezes its position, and releasing it resumes. A "may duck"
  request ducks, and releasing it unducks.
- **Android, from `run-android.sh`:** pause and play through
  `cmd media_session dispatch` (the lock-screen and headset path). `dumpsys`
  must show the session PLAYING and `dev.vieww.audio.PlaybackService` in the
  foreground. With the screen off for 60 s, at least 50 s of audio callbacks
  must arrive.
- **iOS, in the app:** the session category is `.playback`, and the lock
  screen has a now-playing entry. The interruption notification a call
  posts pauses playback, and its end resumes it.
- **iOS, from `run-ios.sh`:** after Home is pressed (through Appium), at
  least 25 s of audio callbacks must arrive in 30 s. This needs
  `UIBackgroundModes` = `audio`, which the workflow's .ipa build sets with
  `VIEWW_BACKGROUND_AUDIO=1`.

The results are the `audio_*` keys in `result.txt`, and the "Audio" section
of the run's SUMMARY.md. A failed audio check fails the phone's job.

What a device cloud cannot do: place a real call, or unplug headphones. The
competing focus request and the posted notification stand in for them,
through the same OS paths.
