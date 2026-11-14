# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ720340f6afa · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 268 / 265 / 477 |
| memory after the run, PSS (MB) | **87.7** / 174.0 / 248.5 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **127.20** / 297.11 / – | 186.9 / **24.6** / 101.8 |
| live2k | – / – / – | – / – / – | – / – / – | 131.13 / **84.34** / – | 183.8 / **42.4** / 97.9 |
| storm2k | – / – / – | – / – / – | – / – / – | 95.45 / **73.96** / – | 137.3 / **60.7** / 117.7 |
| scroll20k | – / – / – | – / – / – | – / – / – | 126.29 / **72.59** / – | 185.4 / **24.4** / 104.1 |
| live20k | – / – / – | – / – / – | – / – / – | 133.96 / **80.06** / – | 186.9 / **41.8** / 98.5 |
| storm20k | – / – / – | – / – / – | – / – / – | 92.21 / **71.37** / – | 138.2 / **62.0** / 98.7 |
| thermal | – / – / – | – / – / – | – / – / – | 132.60 / **78.89** / – | 184.7 / **42.0** / 98.7 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 241.59 / **125.19** / – | – / – / – | **0.0** / 7.5 / 99.8 |
| tick2k | **78.34** / 129.39 / – | – / – / – | 44.1 / **0.0** / 100.4 |
| jump2k | 163.50 / 165.40 / – | – / – / – | 224.5 / **17.5** / 87.0 |
| load20k | 149.13 / **42.52** / – | – / – / – | 0.9 / **0.8** / 126.5 |
| tick20k | 71.27 / **35.22** / – | – / – / – | 135.3 / **0.0** / 107.6 |
| jump20k | 134.00 / **50.05** / – | – / – / – | 182.6 / **7.4** / 146.1 |
| idle2k | **1.09** / 116.79 / – | – / – / – | **0.0** / 46.4 / 98.0 |
| idle20k | **0.79** / 14.04 / – | – / – / – | 11.4 / **4.6** / 105.9 |

### R3CY407VQ2L · Android 15 · SM6475 · 5.2 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 194 / 190 / 359 |
| memory after the run, PSS (MB) | **107.5** / 157.4 / 451.3 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 29.8 / 69.5 / **108.4** | 50 / 16 / **8** | 100.0 / 91.0 / **0.1** | 40.22 / **14.00** / – | 100.2 / **48.6** / 65.1 |
| live2k | 28.1 / 61.6 / **118.2** | 50 / 16 / **8** | 100.0 / 100.0 / **5.0** | 39.11 / **21.64** / – | 101.0 / **73.2** / 119.3 |
| storm2k | 29.9 / 59.8 / **89.9** | 33 / 16 / 16 | 100.0 / 100.0 / **35.7** | 35.49 / **20.09** / – | 99.6 / **79.5** / 122.0 |
| scroll20k | 30.9 / **66.0** / – | 50 / **16** / – | 100.0 / 100.0 / – | 38.77 / **11.58** / – | 101.3 / **45.5** / 98.7 |
| live20k | 28.6 / **62.4** / – | 50 / **16** / – | 100.0 / 100.0 / – | 38.36 / **17.80** / – | 101.2 / **70.9** / 98.5 |
| storm20k | 30.2 / **61.6** / – | 50 / **16** / – | 100.0 / 100.0 / – | 34.50 / **17.98** / – | 99.0 / **81.9** / 98.3 |
| thermal | 27.4 / **60.6** / – | 50 / **16** / – | 100.0 / 100.0 / – | 38.32 / **17.44** / – | 101.2 / **72.5** / 98.5 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 42.95 / **18.85** / – | – / 2 / – | **0.3** / 1.4 / 100.1 |
| tick2k | 31.34 / 30.22 / – | – / – / – | 11.9 / 3.9 / **0.0** |
| jump2k | 50.91 / **29.33** / – | 7 / **28** / – | 109.8 / 68.5 / **0.0** |
| load20k | 38.78 / **20.50** / – | – / – / 128 | **0.0** / 0.6 / 121.6 |
| tick20k | **28.92** / 31.52 / – | – / – / – | 8.4 / **0.0** / 105.2 |
| jump20k | 49.87 / **25.65** / – | **17** / 11 / – | 102.3 / **23.8** / 100.0 |
| idle2k | **0.30** / 13.06 / – | – / – / – | 5.7 / 3.1 / **0.0** |
| idle20k | **0.14** / 5.91 / – | – / – / – | **2.6** / 3.2 / 99.5 |

### RFGL126R73T · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | **72** / 78 / 130 |
| memory after the run, PSS (MB) | **100.3** / 279.6 / 1036.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 63.0 / 65.6 / **120.5** | 20 / 20 / **16** | 100.0 / 100.0 / **6.0** | 21.54 / **9.73** / – | 81.8 / **30.3** / 32.2 |
| live2k | 63.2 / 63.2 / **124.1** | 16 / 16 / **8** | 100.0 / 100.0 / **1.4** | **14.54** / 16.48 / – | 79.7 / **54.3** / 91.3 |
| storm2k | 62.2 / 60.4 / **116.9** | 16 / 16 / 16 | 100.0 / 100.0 / **7.7** | 19.43 / **14.71** / – | 81.3 / **58.4** / 105.9 |
| scroll20k | 65.5 / 66.2 / – | 16 / 16 / – | 100.0 / 100.0 / – | 20.04 / **7.00** / – | 82.0 / **26.4** / 99.2 |
| live20k | 64.0 / 63.8 / – | 16 / 16 / – | 100.0 / 100.0 / – | 18.99 / **14.50** / – | 79.5 / **53.4** / 99.5 |
| storm20k | 61.3 / 58.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | 19.38 / **14.16** / – | 79.3 / **56.0** / 99.3 |
| thermal | 60.2 / 60.1 / 3.7 | 16 / 16 / **8** | 100.0 / 100.0 / **1.4** | 19.54 / **15.35** / – | 79.5 / **52.2** / 92.6 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 13.94 / **7.72** / – | – / **2** / 1 | **0.3** / 0.8 / 35.0 |
| tick2k | 25.70 / 25.44 / – | – / – / – | 9.0 / **4.0** / 4.6 |
| jump2k | 37.01 / **27.94** / – | 23 / **30** / – | 84.9 / 67.6 / **0.0** |
| load20k | **13.70** / 15.91 / – | – / – / – | **0.0** / 0.0 / 99.1 |
| tick20k | **13.89** / 27.35 / – | – / – / – | **5.0** / 8.7 / 100.4 |
| jump20k | 27.85 / **19.87** / – | **30** / 11 / – | 62.5 / **46.2** / 68.6 |
| idle2k | **0.34** / 10.91 / – | – / – / – | 2.2 / 1.0 / **0.8** |
| idle20k | **0.21** / 1.93 / – | – / – / – | 0.9 / **0.7** / 98.3 |

### Across phones · median presented fps

| phase | LMQ720340f6afa | R3CY407VQ2L | RFGL126R73T |
|---|---|---|---|
| scroll2k | – / – / – | 29.8 / 69.5 / **108.4** | 63.0 / 65.6 / **120.5** |
| live2k | – / – / – | 28.1 / 61.6 / **118.2** | 63.2 / 63.2 / **124.1** |
| storm2k | – / – / – | 29.9 / 59.8 / **89.9** | 62.2 / 60.4 / **116.9** |
| scroll20k | – / – / – | 30.9 / **66.0** / – | 65.5 / 66.2 / – |
| live20k | – / – / – | 28.6 / **62.4** / – | 64.0 / 63.8 / – |
| storm20k | – / – / – | 30.2 / **61.6** / – | 61.3 / 58.5 / – |
| thermal | – / – / – | 27.4 / **60.6** / – | 60.2 / 60.1 / 3.7 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
