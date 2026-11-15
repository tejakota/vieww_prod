# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201bd5075a · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 270 / **256** / 463 |
| memory after the run, PSS (MB) | **79.3** / 112.4 / 252.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **44.51** / 61.65 / – | 220.7 / **30.3** / 102.1 |
| live2k | – / – / – | – / – / – | – / – / – | 87.16 / **62.30** / – | 232.1 / **57.4** / 97.8 |
| storm2k | – / – / – | – / – / – | – / – / – | 155.85 / **61.19** / – | 195.9 / **75.1** / 123.6 |
| scroll20k | – / – / – | – / – / – | – / – / – | **45.19** / 59.52 / – | 225.8 / **26.1** / 103.9 |
| live20k | – / – / – | – / – / – | – / – / – | 91.23 / **61.24** / – | 233.5 / **56.7** / 98.4 |
| storm20k | – / – / – | – / – / – | – / – / – | 139.01 / **61.51** / – | 221.7 / **75.6** / 98.6 |
| thermal | – / – / – | – / – / – | – / – / – | 89.62 / **60.92** / – | 231.7 / **55.4** / 98.7 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 182.60 / **44.28** / – | – / – / – | **0.0** / 0.7 / 97.2 |
| tick2k | 81.53 / **15.85** / – | – / – / – | **0.0** / 9.9 / 75.5 |
| jump2k | 156.62 / **59.34** / – | – / – / – | 206.6 / **26.5** / 98.2 |
| load20k | 145.02 / **43.28** / – | – / – / – | **0.0** / 1.7 / 123.5 |
| tick20k | 71.63 / **28.02** / – | – / – / – | 219.5 / **42.7** / 66.9 |
| jump20k | 132.18 / **48.00** / – | – / – / – | 187.0 / **9.4** / 151.5 |
| idle2k | **1.26** / 12.64 / – | – / – / – | 18.8 / **0.0** / 99.4 |
| idle20k | **0.90** / 15.79 / – | – / – / – | 21.0 / **4.9** / 126.7 |

### RFGL1225WVM · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: FAIL (no frames presented in thermal: the app is not drawing), valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 81 / **76** / 129 |
| memory after the run, PSS (MB) | 111.6 / **21.0** / 1037.9 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 62.3 / – / **118.9** | 20 / – / **16** | 100.0 / – / **6.5** | 23.29 / – / – | 74.2 / **0.0** / 31.6 |
| live2k | 62.4 / – / **124.1** | 16 / – / **8** | 100.0 / – / **0.7** | 14.21 / – / – | 82.3 / **0.0** / 93.6 |
| storm2k | 61.3 / – / **114.0** | 16 / – / 16 | 100.0 / – / **9.5** | 15.93 / – / – | 89.3 / **0.0** / 105.7 |
| scroll20k | 65.1 / – / – | 16 / – / – | 100.0 / – / – | 13.56 / – / – | 70.0 / **0.0** / 99.8 |
| live20k | 62.6 / – / – | 16 / – / – | 100.0 / – / – | 14.23 / – / – | 80.4 / **0.0** / 99.5 |
| storm20k | 58.8 / – / – | 16 / – / – | 100.0 / – / – | 17.43 / – / – | 86.8 / **0.0** / 99.7 |
| thermal | **60.0** / – / 0.8 | **16** / – / 1000 | 99.9 / – / **8.7** | 14.22 / – / – | 80.2 / **0.0** / 93.1 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 12.39 / – / – | – / – / 1 | 0.3 / **0.0** / 26.8 |
| tick2k | 19.90 / – / – | – / – / – | 12.6 / **0.0** / 0.0 |
| jump2k | 30.15 / – / – | 20 / – / – | 59.9 / **0.0** / 0.0 |
| load20k | 20.74 / – / – | – / – / 19 | **0.0** / 0.0 / 99.9 |
| tick20k | 13.87 / – / – | – / – / – | 4.9 / **0.0** / 98.5 |
| jump20k | 35.58 / – / – | 36 / – / – | 55.7 / **0.0** / 97.1 |
| idle2k | 0.26 / – / – | – / – / – | 1.5 / **0.0** / 0.0 |
| idle20k | 0.22 / – / – | – / – / – | 3.8 / **2.4** / 101.0 |

### Across phones · median presented fps

| phase | LMQ7201bd5075a | RFGL1225WVM |
|---|---|---|
| scroll2k | – / – / – | 62.3 / – / **118.9** |
| live2k | – / – / – | 62.4 / – / **124.1** |
| storm2k | – / – / – | 61.3 / – / **114.0** |
| scroll20k | – / – / – | 65.1 / – / – |
| live20k | – / – / – | 62.6 / – / – |
| storm20k | – / – / – | 58.8 / – / – |
| thermal | – / – / – | **60.0** / – / 0.8 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
