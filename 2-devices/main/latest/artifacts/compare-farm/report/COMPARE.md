# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ720340f6afa · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 277 / 265 / 476 |
| memory after the run, PSS (MB) | **79.7** / 102.8 / 246.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **42.30** / 68.75 / – | 215.8 / **25.8** / 102.5 |
| live2k | – / – / – | – / – / – | – / – / – | **64.03** / 70.85 / – | 223.3 / **43.6** / 96.6 |
| storm2k | – / – / – | – / – / – | – / – / – | 107.66 / **67.62** / – | 211.5 / **61.3** / 119.5 |
| scroll20k | – / – / – | – / – / – | – / – / – | **41.99** / 63.92 / – | 218.2 / **21.2** / 107.0 |
| live20k | – / – / – | – / – / – | – / – / – | 65.31 / 68.68 / – | 223.1 / **44.9** / 99.1 |
| storm20k | – / – / – | – / – / – | – / – / – | 117.83 / **66.61** / – | 216.5 / **59.3** / 99.5 |
| thermal | – / – / – | – / – / – | – / – / – | 65.07 / 66.95 / – | 218.7 / **44.6** / 98.5 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 196.29 / **41.03** / – | – / – / – | **0.0** / 1.0 / 99.4 |
| tick2k | 34.94 / **29.30** / – | – / – / – | 46.6 / **12.7** / 105.8 |
| jump2k | 134.91 / **62.05** / – | – / – / – | 180.5 / **37.5** / 139.5 |
| load20k | 113.64 / **37.09** / – | – / – / – | **0.0** / 0.9 / 126.3 |
| tick20k | 54.57 / **30.56** / – | – / – / – | **0.0** / 0.0 / 103.8 |
| jump20k | 125.73 / **47.99** / – | – / – / – | 274.5 / **0.0** / 181.8 |
| idle2k | **0.87** / 15.28 / – | – / – / – | **0.0** / 0.0 / 103.8 |
| idle20k | **1.28** / 11.96 / – | – / – / – | 11.7 / **4.5** / 117.7 |

### RFCN20FXGFW · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | **131** / 141 / 167 |
| memory after the run, PSS (MB) | **161.4** / 199.2 / 1066.9 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 70.3 / 72.3 / 68.9 | 16 / 16 / 16 | 4.5 / **0.0** / 0.0 | 15.82 / **12.71** / – | 131.0 / 57.6 / 56.8 |
| live2k | 54.7 / 66.4 / 66.0 | 32 / 16 / 16 | 21.7 / **0.0** / 0.0 | 21.55 / **7.44** / – | 139.4 / **72.4** / 91.8 |
| storm2k | 54.3 / 63.6 / 65.8 | 32 / 16 / 16 | 16.0 / **0.0** / 0.0 | 20.91 / **7.49** / – | 113.9 / 100.1 / 99.5 |
| scroll20k | 69.7 / 71.6 / – | 16 / 16 / – | 4.1 / **0.0** / – | 16.68 / **10.17** / – | 138.3 / **64.4** / 99.5 |
| live20k | 54.3 / **65.9** / – | 32 / **16** / – | 23.2 / **0.0** / – | 21.73 / **6.79** / – | 138.9 / **73.8** / 99.5 |
| storm20k | 50.0 / **61.0** / – | 32 / **16** / – | 21.9 / **0.0** / – | 22.40 / **7.47** / – | 115.1 / 95.8 / 99.4 |
| thermal | 50.8 / **62.0** / – | 32 / **16** / – | 22.5 / **0.0** / – | 21.64 / **7.63** / – | 141.6 / **72.0** / 100.0 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 48.01 / **6.07** / – | – / **2** / 1 | **0.0** / 0.3 / 100.2 |
| tick2k | 32.26 / **19.19** / – | – / – / – | 35.7 / **0.0** / 7.2 |
| jump2k | 62.72 / **18.48** / – | 57 / 57 / – | 126.9 / 51.1 / **0.0** |
| load20k | 48.85 / **8.79** / – | – / – / 44 | **0.0** / 0.0 / 100.2 |
| tick20k | 25.97 / **9.04** / – | – / – / – | 46.7 / **7.5** / 100.0 |
| jump20k | 66.66 / **10.10** / – | **56** / 49 / – | 135.1 / **53.3** / 69.8 |
| idle2k | **0.34** / 3.49 / – | – / – / – | 6.8 / 3.4 / **1.0** |
| idle20k | **0.55** / 3.56 / – | – / – / – | 4.3 / **1.4** / 102.0 |

### Across phones · median presented fps

| phase | LMQ720340f6afa | RFCN20FXGFW |
|---|---|---|
| scroll2k | – / – / – | 70.3 / 72.3 / 68.9 |
| live2k | – / – / – | 54.7 / 66.4 / 66.0 |
| storm2k | – / – / – | 54.3 / 63.6 / 65.8 |
| scroll20k | – / – / – | 69.7 / 71.6 / – |
| live20k | – / – / – | 54.3 / **65.9** / – |
| storm20k | – / – / – | 50.0 / **61.0** / – |
| thermal | – / – / – | 50.8 / **62.0** / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
