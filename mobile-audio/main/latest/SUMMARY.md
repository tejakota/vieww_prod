# Mobile audio — SUCCESS

- branch: `main`  sha: `866b6ac500d714391e0032125f8650e419b605f1`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40917953837

| job | verdict | failed steps |
|---|---|---|
| Android bridge and APK packaging | PASS |  |
| Focus and session logic (Linux) | PASS |  |
| iOS bridge (macOS) | PASS |  |

## Steps

### Android bridge and APK packaging — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: Run taiki-e/install-action@v2
- PASS: Run actions/cache@v4
- PASS: Type-check for Android
- PASS: APK with the audio classes and the playback service
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Focus and session logic (Linux) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: vieww-audio focus, session, engine, supervisor
- PASS: iOS plist opt-in (vieww-build)
- PASS: AXML editor
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### iOS bridge (macOS) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: Run actions/cache@v4
- PASS: Type-check and lint for iOS
- PASS: Link the iOS example (frameworks and extern statics resolve)
- PASS: Background-audio plist opt-in lints
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: vieww-audio-apk 
