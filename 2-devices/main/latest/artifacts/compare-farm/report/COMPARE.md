# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ720340f6afa · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 270 / 264 / 470 |
| memory after the run, PSS (MB) | **79.1** / 106.1 / 252.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **43.65** / 68.41 / – | 127.7 / **27.0** / 103.3 |
| live2k | – / – / – | – / – / – | – / – / – | 69.61 / 70.50 / – | 139.5 / **44.9** / 96.5 |
| storm2k | – / – / – | – / – / – | – / – / – | 99.55 / **67.03** / – | 200.3 / **62.8** / 122.2 |
| scroll20k | – / – / – | – / – / – | – / – / – | **43.69** / 64.45 / – | 126.5 / **23.9** / 104.8 |
| live20k | – / – / – | – / – / – | – / – / – | 67.86 / 68.32 / – | 140.5 / **44.3** / 98.6 |
| storm20k | – / – / – | – / – / – | – / – / – | 105.56 / **66.75** / – | 210.6 / **61.9** / 98.6 |
| thermal | – / – / – | – / – / – | – / – / – | 68.58 / 66.80 / – | 137.1 / **44.9** / 98.8 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 209.85 / **43.44** / – | – / – / – | **0.3** / 1.0 / 98.2 |
| tick2k | 45.07 / **32.18** / – | – / – / – | **0.0** / 10.8 / 110.4 |
| jump2k | 131.91 / **66.89** / – | – / – / – | 113.8 / **26.8** / 108.1 |
| load20k | 110.99 / **36.77** / – | – / – / – | **0.0** / 0.8 / 129.0 |
| tick20k | 31.20 / **28.45** / – | – / – / – | 41.9 / **0.0** / 107.1 |
| jump20k | 133.58 / **34.09** / – | – / – / – | 132.2 / **14.7** / 117.1 |
| idle2k | **1.01** / 14.05 / – | – / – / – | 15.6 / **0.0** / 99.9 |
| idle20k | **0.55** / 14.25 / – | – / – / – | 34.5 / **4.4** / 119.3 |

### RFCN704NGCP · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 140 / 138 / 167 |
| memory after the run, PSS (MB) | **157.2** / 198.7 / 1037.7 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 70.4 / 72.1 / 65.3 | 16 / 16 / 16 | 4.1 / **0.0** / 0.0 | 16.84 / **13.17** / – | 94.0 / 69.4 / 72.8 |
| live2k | 53.1 / 65.5 / 65.5 | 32 / 16 / 16 | 25.4 / **0.0** / 0.0 | 21.91 / **7.56** / – | 101.3 / **74.9** / 93.5 |
| storm2k | 55.6 / 62.9 / 65.7 | 32 / 16 / 16 | 12.1 / **0.0** / 0.0 | 18.86 / **7.71** / – | 97.8 / 99.2 / 100.0 |
| scroll20k | 69.6 / **73.3** / – | 32 / **16** / – | 5.6 / **0.0** / – | 17.32 / **9.83** / – | 95.3 / **78.1** / 99.5 |
| live20k | 53.0 / **66.5** / – | 32 / **16** / – | 25.5 / **0.0** / – | 22.05 / **6.87** / – | 100.9 / **76.2** / 99.5 |
| storm20k | 55.8 / **62.5** / – | 32 / **16** / – | 9.2 / **0.0** / – | 18.30 / **7.50** / – | 95.5 / 98.2 / 99.4 |
| thermal | 48.9 / **62.3** / – | 32 / **16** / – | 26.8 / **0.0** / – | 22.00 / **7.76** / – | 100.2 / **73.7** / 100.0 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 47.46 / **6.33** / – | – / **2** / 1 | 0.3 / 0.3 / 99.7 |
| tick2k | 38.37 / **15.52** / – | – / – / – | 17.6 / **0.0** / 0.0 |
| jump2k | 62.03 / 62.58 / – | **72** / 52 / – | 97.2 / 61.4 / **22.9** |
| load20k | 48.93 / **8.78** / – | – / – / 39 | **0.0** / 0.8 / 103.1 |
| tick20k | 30.36 / **4.95** / – | – / – / – | **0.0** / 0.0 / 99.6 |
| jump20k | 59.38 / **8.36** / – | 56 / **64** / – | 75.8 / **40.8** / 123.8 |
| idle2k | **0.35** / 4.71 / – | – / – / – | 8.7 / 1.6 / **1.0** |
| idle20k | **0.31** / 3.02 / – | – / – / – | 4.6 / **1.5** / 99.6 |

### Across phones · median presented fps

| phase | LMQ720340f6afa | RFCN704NGCP |
|---|---|---|
| scroll2k | – / – / – | 70.4 / 72.1 / 65.3 |
| live2k | – / – / – | 53.1 / 65.5 / 65.5 |
| storm2k | – / – / – | 55.6 / 62.9 / 65.7 |
| scroll20k | – / – / – | 69.6 / **73.3** / – |
| live20k | – / – / – | 53.0 / **66.5** / – |
| storm20k | – / – / – | 55.8 / **62.5** / – |
| thermal | – / – / – | 48.9 / **62.3** / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
