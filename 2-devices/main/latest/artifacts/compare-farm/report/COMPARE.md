# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201bff072f · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 269 / 268 / 479 |
| memory after the run, PSS (MB) | **78.2** / 114.3 / 250.6 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **44.02** / 61.82 / – | 223.4 / **28.6** / 102.2 |
| live2k | – / – / – | – / – / – | – / – / – | 67.71 / **62.22** / – | 222.8 / **56.3** / 97.9 |
| storm2k | – / – / – | – / – / – | – / – / – | 120.27 / **61.39** / – | 207.1 / **72.4** / 119.4 |
| scroll20k | – / – / – | – / – / – | – / – / – | **43.89** / 59.46 / – | 228.5 / **27.8** / 103.9 |
| live20k | – / – / – | – / – / – | – / – / – | 67.25 / **60.94** / – | 230.1 / **56.3** / 97.9 |
| storm20k | – / – / – | – / – / – | – / – / – | 120.01 / **61.11** / – | 229.8 / **77.6** / 98.9 |
| thermal | – / – / – | – / – / – | – / – / – | 67.28 / **60.76** / – | 226.1 / **56.0** / 98.9 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 186.42 / **44.48** / – | – / – / – | **0.0** / 1.0 / 98.2 |
| tick2k | 48.09 / **33.45** / – | – / – / – | **0.0** / 9.9 / 82.0 |
| jump2k | 141.90 / **63.37** / – | – / – / – | 300.0 / **25.4** / 101.7 |
| load20k | 126.25 / **37.84** / – | – / – / – | **0.0** / 0.8 / 130.5 |
| tick20k | 46.27 / **14.41** / – | – / – / – | **0.0** / 7.4 / 44.4 |
| jump20k | 120.39 / **51.37** / – | – / – / – | 266.1 / **9.2** / 144.6 |
| idle2k | **0.88** / 13.32 / – | – / – / – | 44.1 / **0.0** / 98.7 |
| idle20k | **1.04** / 11.38 / – | – / – / – | 7.8 / **0.0** / 110.9 |

### RFCN20R4YRB · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 143 / **130** / 162 |
| memory after the run, PSS (MB) | **164.8** / 218.1 / 1037.5 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 69.1 / 71.6 / 64.5 | 16 / 16 / 16 | 3.0 / **0.0** / 0.0 | 16.06 / **6.30** / – | 129.8 / 67.3 / **55.4** |
| live2k | 53.8 / 65.1 / 64.7 | 32 / 16 / 16 | 22.6 / **0.0** / 0.4 | 21.57 / **6.24** / – | 135.3 / **78.5** / 96.0 |
| storm2k | 50.9 / 61.5 / **64.6** | 32 / 16 / 16 | 21.7 / **0.0** / 0.2 | 22.52 / **7.56** / – | 113.2 / 97.3 / 96.5 |
| scroll20k | 69.8 / 72.0 / – | 16 / 16 / – | 3.6 / **0.0** / – | 16.11 / **6.84** / – | 136.6 / **68.9** / 100.0 |
| live20k | 53.3 / **66.5** / – | 32 / **16** / – | 23.8 / **0.0** / – | 21.98 / **6.07** / – | 138.6 / **78.2** / 99.3 |
| storm20k | 53.1 / **62.7** / – | 32 / **16** / – | 19.0 / **0.0** / – | 22.02 / **7.40** / – | 121.4 / 99.8 / 99.6 |
| thermal | 49.2 / **61.4** / – | 32 / **16** / – | 24.9 / **0.0** / – | 22.82 / **6.43** / – | 138.6 / **78.1** / 99.8 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 46.86 / **8.67** / – | – / **2** / 1 | **0.0** / 0.0 / 99.6 |
| tick2k | 32.17 / **19.31** / – | – / – / – | 14.7 / **0.0** / 0.0 |
| jump2k | 81.00 / **30.15** / – | **52** / 44 / – | 186.0 / 68.2 / **14.6** |
| load20k | 47.35 / **8.64** / – | – / – / 59 | **1.8** / 2.5 / 99.4 |
| tick20k | 30.82 / **7.77** / – | – / – / – | 37.9 / **0.0** / 98.5 |
| jump20k | 77.77 / **8.95** / – | 55 / 55 / – | 131.1 / **53.1** / 86.5 |
| idle2k | **0.47** / 3.01 / – | – / – / – | 6.7 / 5.1 / **0.0** |
| idle20k | **0.28** / 3.19 / – | – / – / – | 6.0 / **1.5** / 100.2 |

### Across phones · median presented fps

| phase | LMQ7201bff072f | RFCN20R4YRB |
|---|---|---|
| scroll2k | – / – / – | 69.1 / 71.6 / 64.5 |
| live2k | – / – / – | 53.8 / 65.1 / 64.7 |
| storm2k | – / – / – | 50.9 / 61.5 / **64.6** |
| scroll20k | – / – / – | 69.8 / 72.0 / – |
| live20k | – / – / – | 53.3 / **66.5** / – |
| storm20k | – / – / – | 53.1 / **62.7** / – |
| thermal | – / – / – | 49.2 / **61.4** / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
