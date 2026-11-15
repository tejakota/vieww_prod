# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ72016369b1d · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 288 / **266** / 480 |
| memory after the run, PSS (MB) | **79.4** / 102.8 / 253.4 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **42.35** / 68.51 / – | 226.0 / **24.7** / 101.9 |
| live2k | – / – / – | – / – / – | – / – / – | **65.68** / 70.31 / – | 222.2 / **44.4** / 98.5 |
| storm2k | – / – / – | – / – / – | – / – / – | 114.86 / **67.33** / – | 212.3 / **61.4** / 118.8 |
| scroll20k | – / – / – | – / – / – | – / – / – | **42.84** / 64.40 / – | 223.5 / **24.1** / 105.9 |
| live20k | – / – / – | – / – / – | – / – / – | 64.37 / 67.31 / – | 220.1 / **43.7** / 99.4 |
| storm20k | – / – / – | – / – / – | – / – / – | 91.25 / **66.63** / – | 236.0 / **59.3** / 99.1 |
| thermal | – / – / – | – / – / – | – / – / – | 64.71 / 67.05 / – | 220.5 / **44.4** / 99.2 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 207.16 / **41.60** / – | – / – / – | **0.0** / 1.0 / 98.3 |
| tick2k | 47.32 / **14.08** / – | – / – / – | 31.0 / **0.0** / 99.8 |
| jump2k | 137.46 / **66.17** / – | – / – / – | 224.8 / **15.3** / 93.2 |
| load20k | 116.36 / **36.59** / – | – / – / – | 0.9 / **0.8** / 126.5 |
| tick20k | 32.69 / 33.07 / – | – / – / – | 44.9 / **14.5** / 92.4 |
| jump20k | 128.35 / **55.30** / – | – / – / – | 210.9 / **16.9** / 100.8 |
| idle2k | **0.88** / 13.86 / – | – / – / – | 42.6 / **0.0** / 99.6 |
| idle20k | **6.04** / 12.84 / – | – / – / – | **0.0** / 0.0 / 108.2 |

### RFCN704NGCP · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 141 / 146 / 169 |
| memory after the run, PSS (MB) | **161.2** / 198.0 / 1037.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 70.2 / 71.8 / 69.1 | 16 / 16 / 16 | 3.8 / **0.0** / 0.0 | 16.27 / **14.37** / – | 130.8 / 71.5 / **66.5** |
| live2k | 53.7 / 65.5 / 65.2 | 32 / 16 / 16 | 23.5 / **0.0** / 0.0 | 22.27 / **7.51** / – | 139.1 / **73.6** / 87.9 |
| storm2k | 49.4 / 61.7 / **65.5** | 32 / 16 / 16 | 24.6 / **0.0** / 0.0 | 22.76 / **7.58** / – | 114.7 / 97.8 / 98.3 |
| scroll20k | 70.1 / 71.5 / – | 16 / 16 / – | 4.3 / **0.0** / – | 16.52 / **10.85** / – | 131.1 / **77.8** / 99.6 |
| live20k | 53.1 / **66.2** / – | 32 / **16** / – | 24.7 / **0.0** / – | 22.45 / **6.83** / – | 137.1 / **77.8** / 99.5 |
| storm20k | 51.6 / **63.9** / – | 32 / **16** / – | 17.6 / **0.0** / – | 20.72 / **7.53** / – | 115.0 / 101.3 / 99.4 |
| thermal | 50.1 / **62.3** / – | 32 / **16** / – | 23.8 / **0.0** / – | 22.51 / **7.79** / – | 138.5 / **74.2** / 99.9 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 48.56 / **5.98** / – | – / **2** / 1 | **0.0** / 0.3 / 96.9 |
| tick2k | 47.50 / **18.08** / – | – / – / – | 33.1 / 11.5 / **5.2** |
| jump2k | 103.41 / **64.72** / – | 51 / **59** / – | 132.4 / 98.8 / **0.0** |
| load20k | 52.12 / **9.09** / – | – / – / 56 | 0.8 / **0.0** / 100.1 |
| tick20k | 32.10 / **6.74** / – | – / – / – | **0.0** / 0.0 / 101.3 |
| jump20k | 96.29 / **22.16** / – | **64** / 49 / – | 117.6 / 98.6 / 100.0 |
| idle2k | **0.35** / 2.70 / – | – / – / – | 10.5 / 3.4 / **2.2** |
| idle20k | **0.31** / 2.99 / – | – / – / – | 6.2 / **1.6** / 99.3 |

### Across phones · median presented fps

| phase | LMQ72016369b1d | RFCN704NGCP |
|---|---|---|
| scroll2k | – / – / – | 70.2 / 71.8 / 69.1 |
| live2k | – / – / – | 53.7 / 65.5 / 65.2 |
| storm2k | – / – / – | 49.4 / 61.7 / **65.5** |
| scroll20k | – / – / – | 70.1 / 71.5 / – |
| live20k | – / – / – | 53.1 / **66.2** / – |
| storm20k | – / – / – | 51.6 / **63.9** / – |
| thermal | – / – / – | 50.1 / **62.3** / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
