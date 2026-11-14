# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201c3807bd · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 268 / 267 / 473 |
| memory after the run, PSS (MB) | **88.7** / 175.3 / 247.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **126.54** / 295.78 / – | 186.9 / **25.4** / 102.5 |
| live2k | – / – / – | – / – / – | – / – / – | 132.72 / **83.96** / – | 183.4 / **40.9** / 97.5 |
| storm2k | – / – / – | – / – / – | – / – / – | 126.06 / **71.11** / – | 141.0 / **60.9** / 118.7 |
| scroll20k | – / – / – | – / – / – | – / – / – | 127.47 / **72.83** / – | 187.8 / **24.2** / 104.8 |
| live20k | – / – / – | – / – / – | – / – / – | 130.15 / **79.88** / – | 183.8 / **42.2** / 99.1 |
| storm20k | – / – / – | – / – / – | – / – / – | 91.78 / **71.13** / – | 144.7 / **61.8** / 98.8 |
| thermal | – / – / – | – / – / – | – / – / – | 132.98 / **78.71** / – | 183.9 / **42.1** / 98.7 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 238.23 / **126.05** / – | – / – / – | **0.0** / 7.7 / 98.5 |
| tick2k | **77.62** / 143.44 / – | – / – / – | 72.6 / **44.4** / 98.5 |
| jump2k | 165.87 / 162.67 / – | – / – / – | 189.2 / **35.7** / 99.1 |
| load20k | 134.45 / **50.09** / – | – / – / – | **0.0** / 0.9 / 124.9 |
| tick20k | 72.33 / **36.95** / – | – / – / – | **0.0** / 0.0 / 120.8 |
| jump20k | 144.95 / **38.83** / – | – / – / – | 144.0 / **8.3** / 108.7 |
| idle2k | **0.86** / 111.31 / – | – / – / – | **13.9** / 25.2 / 100.4 |
| idle20k | **1.16** / 11.63 / – | – / – / – | 10.9 / **4.4** / 107.3 |

### R3CY407VAMT · Android 15 · SM6475 · 5.2 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 194 / 196 / 377 |
| memory after the run, PSS (MB) | **106.1** / 154.9 / 451.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 30.9 / 69.5 / **106.1** | 50 / 16 / **8** | 100.0 / 91.9 / **0.1** | 38.81 / **13.48** / – | 100.6 / **49.1** / 67.5 |
| live2k | 29.0 / 63.1 / **118.1** | 50 / 16 / 16 | 100.0 / 100.0 / **5.5** | 38.10 / **17.00** / – | 101.5 / **71.6** / 118.3 |
| storm2k | 30.9 / 59.0 / **90.9** | 50 / 16 / 16 | 100.0 / 100.0 / **35.7** | 35.35 / **18.31** / – | 99.7 / **77.1** / 121.8 |
| scroll20k | 31.4 / **67.1** / – | 50 / **16** / – | 100.0 / 100.0 / – | 37.67 / **11.20** / – | 101.8 / **46.3** / 98.7 |
| live20k | 28.9 / **62.6** / – | 50 / **16** / – | 100.0 / 100.0 / – | 37.86 / **18.34** / – | 101.2 / **72.6** / 98.5 |
| storm20k | 31.0 / **60.8** / – | 33 / **16** / – | 100.0 / 100.0 / – | 34.36 / **16.99** / – | 100.9 / **79.1** / 98.5 |
| thermal | 27.4 / **60.4** / – | 50 / **16** / – | 100.0 / 100.0 / – | 38.64 / **17.09** / – | 100.8 / **70.7** / 98.5 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 36.70 / **15.07** / – | – / 2 / – | **0.4** / 1.7 / 100.0 |
| tick2k | **23.93** / 27.84 / – | – / – / – | 10.4 / 3.9 / **0.0** |
| jump2k | 48.27 / **36.99** / – | 12 / **38** / – | 142.9 / 41.1 / **0.0** |
| load20k | 45.65 / **16.86** / – | – / – / 134 | **0.0** / 0.9 / 123.4 |
| tick20k | 26.20 / 25.06 / – | – / – / – | **0.0** / 3.1 / 90.9 |
| jump20k | 54.09 / **24.78** / – | 11 / **40** / – | 125.0 / **34.9** / 98.8 |
| idle2k | **0.11** / 17.46 / – | – / – / – | 4.1 / 3.2 / **0.0** |
| idle20k | **0.19** / 5.69 / – | – / – / – | 3.5 / **0.0** / 95.2 |

### RFGL126R73T · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 78 / 78 / 129 |
| memory after the run, PSS (MB) | **98.5** / 281.8 / 1039.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 62.2 / 64.5 / **120.2** | 20 / 20 / **16** | 100.0 / 100.0 / **6.5** | 21.32 / **9.77** / – | 81.3 / 29.3 / 30.1 |
| live2k | 63.3 / 63.6 / **122.8** | 16 / 16 / **8** | 100.0 / 100.0 / **1.2** | 20.33 / **16.19** / – | 81.4 / **53.0** / 92.0 |
| storm2k | 59.3 / 58.9 / **116.0** | 16 / 16 / 16 | 100.0 / 100.0 / **8.2** | 19.09 / **15.28** / – | 77.5 / **57.3** / 106.8 |
| scroll20k | 65.5 / 66.4 / – | 16 / 16 / – | 100.0 / 100.0 / – | 20.70 / **6.72** / – | 82.7 / **24.3** / 99.7 |
| live20k | 63.6 / 63.4 / – | 16 / 16 / – | 100.0 / 100.0 / – | 20.33 / **15.40** / – | 80.5 / **53.7** / 99.6 |
| storm20k | 57.8 / 58.7 / – | 16 / 16 / – | 100.0 / 100.0 / – | 19.21 / **14.24** / – | 76.2 / **57.1** / 99.2 |
| thermal | 59.9 / 60.3 / 16.1 | 16 / 16 / 16 | 100.0 / 100.0 / **7.1** | 19.87 / **15.56** / – | 79.2 / **52.1** / 88.3 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | **11.52** / 12.28 / – | – / **2** / 1 | **0.3** / 0.8 / 41.0 |
| tick2k | 22.98 / **19.83** / – | – / – / – | 6.5 / 5.5 / **0.0** |
| jump2k | 32.23 / 33.21 / – | **15** / 7 / – | 111.1 / 74.6 / **0.0** |
| load20k | 12.90 / 12.94 / – | – / – / 12 | **0.0** / 0.0 / 98.6 |
| tick20k | **15.64** / 27.02 / – | – / – / – | 3.4 / **0.0** / 94.9 |
| jump20k | **24.41** / 28.56 / – | **29** / 18 / – | 73.4 / **39.2** / 92.5 |
| idle2k | **0.25** / 10.99 / – | – / – / – | 3.3 / 2.2 / **0.0** |
| idle20k | **0.47** / 1.53 / – | – / – / – | **1.3** / 1.5 / 100.2 |

### Across phones · median presented fps

| phase | LMQ7201c3807bd | R3CY407VAMT | RFGL126R73T |
|---|---|---|---|
| scroll2k | – / – / – | 30.9 / 69.5 / **106.1** | 62.2 / 64.5 / **120.2** |
| live2k | – / – / – | 29.0 / 63.1 / **118.1** | 63.3 / 63.6 / **122.8** |
| storm2k | – / – / – | 30.9 / 59.0 / **90.9** | 59.3 / 58.9 / **116.0** |
| scroll20k | – / – / – | 31.4 / **67.1** / – | 65.5 / 66.4 / – |
| live20k | – / – / – | 28.9 / **62.6** / – | 63.6 / 63.4 / – |
| storm20k | – / – / – | 31.0 / **60.8** / – | 57.8 / 58.7 / – |
| thermal | – / – / – | 27.4 / **60.4** / – | 59.9 / 60.3 / 16.1 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
