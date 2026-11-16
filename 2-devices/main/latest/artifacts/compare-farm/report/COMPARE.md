# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ72016369a75 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 380 / **344** / – |
| memory after the run, PSS (MB) | **79.2** / 182.8 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **31.37** / 166.36 / – | 145.8 / **43.3** / – |
| live2k | – / – / – | – / – / – | – / – / – | **52.89** / 62.55 / – | 132.0 / **58.7** / – |
| storm2k | – / – / – | – / – / – | – / – / – | 73.53 / **61.45** / – | 109.1 / **76.6** / – |
| scroll20k | – / – / – | – / – / – | – / – / – | **31.27** / 59.49 / – | 145.4 / **27.4** / – |
| live20k | – / – / – | – / – / – | – / – / – | **53.74** / 61.33 / – | 131.7 / **54.8** / – |
| storm20k | – / – / – | – / – / – | – / – / – | 72.12 / **61.63** / – | 110.1 / **76.3** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 188.44 / **49.36** / – | – / – / – | **0.0** / 0.4 / – |
| tick2k | 33.00 / 31.94 / – | – / – / – | 37.3 / **17.7** / – |
| jump2k | 174.88 / **53.83** / – | – / – / – | 146.2 / **15.3** / – |
| load20k | 147.00 / **34.70** / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | 48.83 / **16.86** / – | – / – / – | 99.0 / **0.0** / – |
| jump20k | 152.24 / **55.92** / – | – / – / – | 142.9 / **7.4** / – |
| idle2k | **1.06** / 7.64 / – | – / – / – | 35.7 / **5.1** / – |
| idle20k | **0.72** / 272.90 / – | – / – / – | 8.9 / **0.0** / – |

### R5CT41Q1P2M · Android 12 · SM7325 · 7.1 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 204 / **192** / – |
| memory after the run, PSS (MB) | **94.5** / 145.5 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 74.0 / **117.6** / – | 24 / **16** / – | 74.0 / **15.3** / – | 16.88 / **8.84** / – | 89.5 / **55.6** / – |
| live2k | 63.9 / 64.5 / – | 24 / 24 / – | 94.3 / **79.4** / – | 18.90 / **16.29** / – | 107.4 / **76.0** / – |
| storm2k | 46.1 / **62.4** / – | 24 / 24 / – | 100.0 / **77.6** / – | 22.96 / **17.89** / – | 110.5 / **84.4** / – |
| scroll20k | 69.3 / 67.7 / – | 24 / 24 / – | **84.5** / 93.1 / – | 17.30 / **9.68** / – | 91.0 / **47.0** / – |
| live20k | 63.4 / 64.8 / – | 24 / 24 / – | 92.5 / **82.8** / – | 19.14 / **15.97** / – | 107.2 / **76.9** / – |
| storm20k | 59.5 / 61.9 / – | 24 / 24 / – | 96.5 / **76.9** / – | 18.28 / 18.27 / – | 112.5 / **83.8** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 32.91 / **12.96** / – | – / 2 / – | 1.0 / 1.0 / – |
| tick2k | 18.41 / **15.99** / – | – / – / – | **0.0** / 0.0 / – |
| jump2k | 42.23 / **30.08** / – | 27 / **49** / – | 88.0 / **56.0** / – |
| load20k | 36.42 / **13.10** / – | – / – / – | **0.0** / 0.6 / – |
| tick20k | 22.04 / **12.05** / – | – / – / – | 9.9 / **3.1** / – |
| jump20k | 53.91 / **11.72** / – | **57** / 43 / – | 148.6 / **38.5** / – |
| idle2k | **0.42** / 10.74 / – | – / – / – | 3.3 / **2.0** / – |
| idle20k | **0.39** / 6.51 / – | – / – / – | 4.7 / **1.6** / – |

### RFGL126N3TY · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 106 / 103 / – |
| memory after the run, PSS (MB) | **99.6** / 217.8 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | **130.3** / 69.6 / – | **8** / 16 / – | **0.5** / 90.2 / – | 8.24 / 8.61 / – | 71.3 / **29.6** / – |
| live2k | 66.0 / 63.4 / – | 16 / 16 / – | **91.8** / 100.0 / – | 14.59 / **13.05** / – | 76.2 / **54.1** / – |
| storm2k | 61.7 / 59.0 / – | 16 / 16 / – | 100.0 / 100.0 / – | 13.91 / **13.19** / – | 75.0 / **56.4** / – |
| scroll20k | 66.2 / **89.5** / – | 16 / 16 / – | 100.0 / **49.2** / – | 13.19 / **8.52** / – | 61.0 / **33.7** / – |
| live20k | 63.2 / 64.2 / – | 16 / 16 / – | 100.0 / 97.1 / – | 14.06 / **12.08** / – | 73.9 / **50.8** / – |
| storm20k | 58.7 / **61.9** / – | 16 / 16 / – | 100.0 / 100.0 / – | 17.00 / **13.32** / – | 81.9 / **60.6** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 12.40 / **3.15** / – | – / 2 / – | **0.0** / 0.0 / – |
| tick2k | **9.47** / 12.69 / – | – / – / – | 8.4 / **3.9** / – |
| jump2k | 30.22 / **17.55** / – | 59 / 59 / – | 68.6 / **50.8** / – |
| load20k | 24.10 / **9.75** / – | – / – / – | **0.0** / 0.9 / – |
| tick20k | 14.79 / **11.38** / – | – / – / – | 9.8 / **1.5** / – |
| jump20k | 35.65 / **14.71** / – | 13 / **32** / – | **19.6** / 35.1 / – |
| idle2k | **0.24** / 1.00 / – | – / – / – | **0.8** / 2.1 / – |
| idle20k | **0.23** / 2.56 / – | – / – / – | 1.8 / 1.8 / – |

### Across phones · median presented fps

| phase | LMQ72016369a75 | R5CT41Q1P2M | RFGL126N3TY |
|---|---|---|---|
| scroll2k | – / – / – | 74.0 / **117.6** / – | **130.3** / 69.6 / – |
| live2k | – / – / – | 63.9 / 64.5 / – | 66.0 / 63.4 / – |
| storm2k | – / – / – | 46.1 / **62.4** / – | 61.7 / 59.0 / – |
| scroll20k | – / – / – | 69.3 / 67.7 / – | 66.2 / **89.5** / – |
| live20k | – / – / – | 63.4 / 64.8 / – | 63.2 / 64.2 / – |
| storm20k | – / – / – | 59.5 / 61.9 / – | 58.7 / **61.9** / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
