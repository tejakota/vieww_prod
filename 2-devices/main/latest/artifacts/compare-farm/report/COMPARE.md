# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201bd50691 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: FAIL (no frames presented in thermal: the app is not drawing), vieww-gpu: FAIL (no frames presented in thermal: the app is not drawing), valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 267 / 257 / 471 |
| memory after the run, PSS (MB) | **89.5** / 179.5 / 255.4 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **129.85** / 291.53 / – | 188.1 / **25.1** / 102.1 |
| live2k | – / – / – | – / – / – | – / – / – | 129.15 / **84.42** / – | 185.3 / **41.6** / 95.7 |
| storm2k | – / – / – | – / – / – | – / – / – | 93.76 / **71.17** / – | 131.9 / **59.8** / 119.1 |
| scroll20k | – / – / – | – / – / – | – / – / – | 129.31 / **72.39** / – | 187.1 / **24.0** / 105.2 |
| live20k | – / – / – | – / – / – | – / – / – | 131.55 / **80.17** / – | 186.7 / **42.6** / 99.2 |
| storm20k | – / – / – | – / – / – | – / – / – | 91.30 / **71.42** / – | 144.0 / **60.7** / 98.9 |
| thermal | – / – / – | – / – / – | – / – / – | 132.54 / **78.70** / – | 185.3 / **42.3** / 98.9 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 204.11 / **123.47** / – | – / – / – | **0.0** / 7.7 / 98.8 |
| tick2k | **71.75** / 133.79 / – | – / – / – | 57.6 / **0.0** / 102.6 |
| jump2k | 150.62 / **135.01** / – | – / – / – | 207.7 / **78.6** / 100.8 |
| load20k | 146.61 / **42.30** / – | – / – / – | **0.0** / 0.8 / 128.2 |
| tick20k | 70.75 / **34.17** / – | – / – / – | 47.6 / **37.6** / 112.5 |
| jump20k | 135.58 / **52.33** / – | – / – / – | 261.4 / **8.6** / 144.2 |
| idle2k | **0.87** / 114.46 / – | – / – / – | **0.0** / 28.6 / 101.3 |
| idle20k | **7.54** / 14.28 / – | – / – / – | 10.4 / **0.0** / 112.9 |

### RFCN20EFB6L · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing)

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 141 / 143 / 166 |
| memory after the run, PSS (MB) | **165.3** / 216.6 / 1065.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 35.8 / **71.6** / 65.8 | 50 / 16 / 16 | 87.1 / **0.0** / 0.0 | 46.41 / **14.69** / – | 99.8 / 67.8 / **46.1** |
| live2k | 19.7 / 65.3 / 64.9 | 66 / 16 / 16 | 100.0 / **0.0** / 0.0 | 58.62 / **16.10** / – | 99.6 / **68.4** / 84.3 |
| storm2k | 32.2 / 60.7 / **64.5** | 33 / 16 / 16 | 97.4 / **0.0** / 0.0 | 33.57 / **15.06** / – | 99.5 / **69.8** / 95.0 |
| scroll20k | 36.0 / **72.1** / – | 50 / **16** / – | 87.4 / **0.0** / – | 45.73 / **10.08** / – | 100.0 / **67.4** / 99.4 |
| live20k | 19.7 / **66.2** / – | 66 / **16** / – | 100.0 / **0.0** / – | 58.24 / **15.93** / – | 99.7 / **70.1** / 99.9 |
| storm20k | 31.2 / **61.8** / – | 33 / **16** / – | 98.2 / **0.0** / – | 33.88 / **11.82** / – | 98.9 / **79.9** / 99.4 |
| thermal | 17.9 / **61.5** / – | 66 / **16** / – | 100.0 / **0.0** / – | 58.88 / **16.47** / – | 99.1 / **68.5** / 100.0 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 48.58 / **28.88** / – | – / **2** / 1 | **0.3** / 2.1 / 98.9 |
| tick2k | 40.41 / 41.08 / – | – / – / – | **0.0** / 36.0 / 0.0 |
| jump2k | 61.77 / **53.27** / – | 27 / 28 / – | 105.3 / 58.8 / **7.4** |
| load20k | 58.31 / **15.59** / – | – / – / 41 | **0.0** / 0.0 / 99.3 |
| tick20k | 28.07 / **16.96** / – | – / – / – | 26.8 / **0.0** / 90.6 |
| jump20k | 60.42 / **15.93** / – | 24 / **60** / – | 104.8 / **60.2** / 103.4 |
| idle2k | **0.48** / 30.67 / – | – / – / – | 7.5 / 6.7 / **0.0** |
| idle20k | **0.26** / 3.01 / – | – / – / – | 42.6 / **4.4** / 102.6 |

### RFGL126KZHX · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 80 / **76** / 131 |
| memory after the run, PSS (MB) | **99.8** / 279.4 / 1038.7 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 62.6 / 65.7 / **121.6** | 20 / 16 / 16 | 100.0 / 100.0 / **7.9** | 21.67 / **9.75** / – | 80.3 / **29.2** / 31.4 |
| live2k | 64.3 / 63.4 / **124.9** | 16 / 16 / **8** | 100.0 / 100.0 / **0.9** | 15.88 / 15.84 / – | 79.6 / **54.0** / 92.9 |
| storm2k | 60.2 / 59.6 / **114.9** | 16 / 16 / 16 | 100.0 / 100.0 / **8.7** | 19.02 / **14.31** / – | 77.7 / **56.8** / 106.3 |
| scroll20k | 65.7 / 66.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | 20.82 / **6.66** / – | 82.6 / **24.2** / 100.0 |
| live20k | 64.3 / 64.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | 20.11 / **15.29** / – | 80.9 / **52.1** / 99.8 |
| storm20k | 60.4 / 60.4 / – | 16 / 16 / – | 100.0 / 100.0 / – | 19.62 / **14.30** / – | 79.8 / **59.1** / 99.3 |
| thermal | 60.3 / 60.4 / 18.2 | 16 / 16 / **8** | 100.0 / 100.0 / **4.1** | 19.91 / **15.51** / – | 80.2 / **53.1** / 88.7 |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 14.16 / **10.56** / – | – / **2** / 1 | **0.3** / 0.9 / 36.0 |
| tick2k | **25.49** / 31.70 / – | – / – / – | 7.5 / **1.9** / 2.3 |
| jump2k | 37.19 / **15.47** / – | 20 / **22** / 1 | 92.8 / 25.4 / **0.0** |
| load20k | **13.05** / 14.46 / – | – / – / 2 | **0.0** / 0.0 / 95.8 |
| tick20k | **14.12** / 27.52 / – | – / – / – | 11.2 / **0.0** / 93.4 |
| jump20k | **20.07** / 23.06 / – | 10 / **17** / – | 44.2 / **9.8** / 95.0 |
| idle2k | **0.25** / 14.36 / – | – / – / – | 3.1 / 1.6 / **0.0** |
| idle20k | **0.21** / 2.22 / – | – / – / – | 1.9 / 1.9 / 100.6 |

### Across phones · median presented fps

| phase | LMQ7201bd50691 | RFCN20EFB6L | RFGL126KZHX |
|---|---|---|---|
| scroll2k | – / – / – | 35.8 / **71.6** / 65.8 | 62.6 / 65.7 / **121.6** |
| live2k | – / – / – | 19.7 / 65.3 / 64.9 | 64.3 / 63.4 / **124.9** |
| storm2k | – / – / – | 32.2 / 60.7 / **64.5** | 60.2 / 59.6 / **114.9** |
| scroll20k | – / – / – | 36.0 / **72.1** / – | 65.7 / 66.5 / – |
| live20k | – / – / – | 19.7 / **66.2** / – | 64.3 / 64.5 / – |
| storm20k | – / – / – | 31.2 / **61.8** / – | 60.4 / 60.4 / – |
| thermal | – / – / – | 17.9 / **61.5** / – | 60.3 / 60.4 / 18.2 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
