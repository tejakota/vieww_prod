# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ7201bd50691 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`
- vieww-gpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 383 / **334** / – |
| memory after the run, PSS (MB) | **81.6** / 182.3 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **31.43** / 165.53 / – | 146.5 / **43.7** / – |
| live2k | – / – / – | – / – / – | – / – / – | **53.24** / 63.24 / – | 131.5 / **57.5** / – |
| storm2k | – / – / – | – / – / – | – / – / – | 73.53 / **61.46** / – | 109.2 / **77.8** / – |
| scroll20k | – / – / – | – / – / – | – / – / – | **31.49** / 59.52 / – | 144.9 / **27.4** / – |
| live20k | – / – / – | – / – / – | – / – / – | **53.40** / 61.02 / – | 131.4 / **52.9** / – |
| storm20k | – / – / – | – / – / – | – / – / – | 77.52 / **61.59** / – | 111.3 / **73.9** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 204.32 / **47.12** / – | – / – / – | **0.3** / 0.7 / – |
| tick2k | 49.65 / **32.76** / – | – / – / – | 18.7 / **9.9** / – |
| jump2k | 176.84 / **65.58** / – | – / – / – | 135.6 / **26.3** / – |
| load20k | 223.71 / **35.65** / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | **53.69** / 513.30 / – | – / – / – | **0.0** / 13.6 / – |
| jump20k | **144.47** / 194.11 / – | – / – / – | 202.5 / **16.7** / – |
| idle2k | **0.77** / 12.84 / – | – / – / – | **0.0** / 0.0 / – |
| idle20k | **0.72** / 804.92 / – | – / – / – | 10.3 / **0.0** / – |

### R5CT41PZHLF · Android 12 · SM7325 · 7.1 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint unavailable (the system declined a session)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint unavailable (the system declined a session)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 222 / **208** / – |
| memory after the run, PSS (MB) | **96.1** / 145.8 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 71.5 / 70.7 / – | 25 / 25 / – | **77.5** / 88.2 / – | 16.33 / **9.31** / – | 87.6 / **45.5** / – |
| live2k | 63.1 / 64.5 / – | 25 / 25 / – | 93.7 / **77.5** / – | 18.47 / **16.20** / – | 107.1 / **77.0** / – |
| storm2k | 49.3 / **60.7** / – | 25 / 25 / – | 99.4 / **77.3** / – | 22.73 / **17.74** / – | 110.1 / **81.6** / – |
| scroll20k | 69.9 / 68.6 / – | 25 / 25 / – | **80.8** / 89.2 / – | 17.60 / **9.98** / – | 90.0 / **48.4** / – |
| live20k | 64.4 / 64.9 / – | 25 / 25 / – | 90.7 / **80.6** / – | 19.53 / **16.14** / – | 107.1 / **77.0** / – |
| storm20k | 45.3 / **61.7** / – | 25 / 25 / – | 98.9 / **77.7** / – | 23.02 / **18.10** / – | 106.5 / **83.2** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 39.37 / **9.27** / – | – / 2 / – | **0.4** / 1.0 / – |
| tick2k | 22.84 / **20.85** / – | – / – / – | 9.4 / **0.0** / – |
| jump2k | 39.95 / **18.27** / – | 58 / **89** / – | 148.6 / **63.0** / – |
| load20k | 36.83 / **16.73** / – | – / – / – | **0.0** / 0.9 / – |
| tick20k | 21.94 / **10.35** / – | – / – / – | **0.0** / 1.6 / – |
| jump20k | 45.06 / **13.54** / – | **47** / 38 / – | 94.0 / **23.1** / – |
| idle2k | **0.45** / 10.30 / – | – / – / – | 3.1 / **2.0** / – |
| idle20k | **0.39** / 13.79 / – | – / – / – | 2.6 / **0.0** / – |

### R3GL105R47R · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 60 -> 120 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; asked (ANativeWindow_setFrameRateWithChangeStrategy, always)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 100 / 100 / – |
| memory after the run, PSS (MB) | **99.6** / 217.8 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | **79.3** / 71.2 / – | 16 / 16 / – | **65.4** / 84.6 / – | 12.50 / **8.92** / – | 60.2 / **29.2** / – |
| live2k | 63.0 / 64.1 / – | 16 / 16 / – | 100.0 / 100.0 / – | 14.50 / **12.82** / – | 75.0 / **54.3** / – |
| storm2k | 58.9 / 61.3 / – | 16 / 16 / – | 100.0 / 100.0 / – | 14.08 / **12.80** / – | 81.6 / **59.0** / – |
| scroll20k | 66.1 / **69.8** / – | 16 / 16 / – | 100.0 / **88.8** / – | 12.99 / **5.90** / – | 58.9 / **26.5** / – |
| live20k | 63.0 / **67.1** / – | 16 / 16 / – | 100.0 / **89.5** / – | 14.08 / **11.75** / – | 73.5 / **51.5** / – |
| storm20k | 57.8 / 60.0 / – | 16 / 16 / – | 100.0 / 100.0 / – | 17.11 / **12.96** / – | 79.8 / **58.1** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 11.91 / **2.96** / – | – / 2 / – | **0.0** / 0.0 / – |
| tick2k | 14.63 / **12.04** / – | – / – / – | 7.8 / **1.9** / – |
| jump2k | 39.85 / **15.09** / – | 25 / 25 / – | 28.8 / 28.8 / – |
| load20k | 20.27 / **12.55** / – | – / – / – | 0.9 / **0.0** / – |
| tick20k | 12.69 / **10.65** / – | – / – / – | 4.7 / **3.3** / – |
| jump20k | 27.07 / **14.35** / – | **35** / 27 / – | 41.7 / **21.5** / – |
| idle2k | **0.07** / 1.75 / – | – / – / – | 3.2 / **2.1** / – |
| idle20k | **0.22** / 2.71 / – | – / – / – | 2.5 / **1.9** / – |

### Across phones · median presented fps

| phase | LMQ7201bd50691 | R5CT41PZHLF | R3GL105R47R |
|---|---|---|---|
| scroll2k | – / – / – | 71.5 / 70.7 / – | **79.3** / 71.2 / – |
| live2k | – / – / – | 63.1 / 64.5 / – | 63.0 / 64.1 / – |
| storm2k | – / – / – | 49.3 / **60.7** / – | 58.9 / 61.3 / – |
| scroll20k | – / – / – | 69.9 / 68.6 / – | 66.1 / **69.8** / – |
| live20k | – / – / – | 64.4 / 64.9 / – | 63.0 / **67.1** / – |
| storm20k | – / – / – | 45.3 / **61.7** / – | 57.8 / 60.0 / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
