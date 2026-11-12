# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201bd50691 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — none

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|

### RZCY80L23GJ · Android 16 · SM6475 · 5.3 GB · 120.00001 Hz

Runs — none

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|

### RFGL126LSST · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — none

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|

### Across phones · median presented fps

| phase | LMQ7201bd50691 | RZCY80L23GJ | RFGL126LSST |
|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – |
| live2k | – / – / – | – / – / – | – / – / – |
| storm2k | – / – / – | – / – / – | – / – / – |
| scroll20k | – / – / – | – / – / – | – / – / – |
| live20k | – / – / – | – / – / – | – / – / – |
| storm20k | – / – / – | – / – / – | – / – / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
