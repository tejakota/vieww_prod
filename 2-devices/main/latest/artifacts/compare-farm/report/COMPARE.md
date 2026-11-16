# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ720163b9ae8 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`
- vieww-gpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 378 / **344** / – |
| memory after the run, PSS (MB) | **81.7** / 183.0 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **32.36** / 167.42 / – | 145.3 / **44.3** / – |
| live2k | – / – / – | – / – / – | – / – / – | **55.80** / 62.34 / – | 131.1 / **57.0** / – |
| storm2k | – / – / – | – / – / – | – / – / – | 76.01 / **61.36** / – | 111.8 / **77.7** / – |
| scroll20k | – / – / – | – / – / – | – / – / – | **33.02** / 59.60 / – | 135.0 / **28.9** / – |
| live20k | – / – / – | – / – / – | – / – / – | **53.65** / 60.91 / – | 132.2 / **56.5** / – |
| storm20k | – / – / – | – / – / – | – / – / – | 75.65 / **61.51** / – | 110.5 / **75.9** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 192.52 / **45.13** / – | – / – / – | **0.0** / 1.0 / – |
| tick2k | 47.55 / **29.11** / – | – / – / – | **0.0** / 0.0 / – |
| jump2k | 170.80 / **60.31** / – | – / – / – | 153.8 / **17.4** / – |
| load20k | 148.06 / **36.16** / – | – / – / – | 0.6 / **0.0** / – |
| tick20k | **31.59** / 611.90 / – | – / – / – | **0.0** / 5.9 / – |
| jump20k | **148.00** / 464.24 / – | – / – / – | 111.1 / **32.3** / – |
| idle2k | **0.48** / 14.63 / – | – / – / – | **0.0** / 5.2 / – |
| idle20k | **0.97** / 6.43 / – | – / – / – | **0.0** / 6.6 / – |

### R3CY407X32L · Android 15 · SM6475 · 5.2 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; paced by the window; last 120 frames: 120 missed a vsync at 120 Hz; own work p50 13.94 / p95 15.40 ms against the panel's 8.33 ms` · `window#primary frame rate 60 -> 120 Hz on a 120 Hz panel; paced by the window; last 120 frames: 1 missed a vsync at 60 Hz; own work p50 20.68 / p95 22.41 ms against the panel's 8.33 ms; probing the panel's rate` · `window#primary frame rate 120 -> 60 Hz on a 120 Hz panel; paced by the window; last 120 frames: 119 missed a vsync at 120 Hz; own work p50 14.30 / p95 15.89 ms against the panel's 8.33 ms` · `window#primary frame rate 60 -> 120 Hz on a 120 Hz panel; paced by the window; last 120 frames: 0 missed a vsync at 60 Hz; own work p50 21.98 / p95 23.02 ms against the panel's 8.33 ms; probing the panel's rate`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 286 / **254** / – |
| memory after the run, PSS (MB) | **92.6** / 152.4 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 117.1 / **134.5** / – | 16 / **8** / – | 14.5 / **0.0** / – | 11.76 / **9.31** / – | 116.9 / **58.9** / – |
| live2k | 66.6 / **126.0** / – | 16 / **8** / – | 88.1 / **0.3** / – | 16.69 / **10.58** / – | 108.8 / **74.6** / – |
| storm2k | 47.7 / **119.6** / – | 24 / **8** / – | 100.0 / **1.3** / – | 22.28 / **11.00** / – | 103.9 / **78.4** / – |
| scroll20k | 117.1 / **134.2** / – | 16 / **8** / – | 15.5 / **0.0** / – | 11.51 / **8.64** / – | 115.2 / **57.4** / – |
| live20k | 68.6 / **125.8** / – | 24 / **8** / – | 78.3 / **0.0** / – | 16.06 / **9.24** / – | 109.6 / **71.4** / – |
| storm20k | 45.3 / **123.7** / – | 24 / **8** / – | 100.0 / **0.0** / – | 23.60 / **8.82** / – | 103.3 / **74.0** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 33.74 / **9.61** / – | – / 2 / – | **0.4** / 0.7 / – |
| tick2k | **13.57** / 15.57 / – | – / – / – | 7.5 / **6.2** / – |
| jump2k | 41.51 / **23.59** / – | 8 / **58** / – | **0.0** / 61.5 / – |
| load20k | 35.46 / **11.68** / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | 20.97 / **4.92** / – | – / – / – | **0.0** / 2.9 / – |
| jump20k | 50.66 / **18.81** / – | 28 / **41** / – | 111.1 / **58.8** / – |
| idle2k | **0.31** / 3.08 / – | – / – / – | 2.1 / 2.0 / – |
| idle20k | **0.14** / 2.05 / – | – / – / – | 1.7 / **0.0** / – |

### RFGL1225WVM · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 106 / 106 / – |
| memory after the run, PSS (MB) | **100.3** / 217.1 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 129.8 / 133.5 / – | 8 / 8 / – | 0.5 / **0.0** / – | **8.38** / 9.53 / – | 70.2 / **45.8** / – |
| live2k | 126.0 / 126.9 / – | 8 / 8 / – | 0.7 / **0.0** / – | **8.57** / 9.83 / – | 91.8 / **61.3** / – |
| storm2k | 118.8 / 118.3 / – | 8 / 8 / – | **0.0** / 0.0 / – | **8.04** / 10.21 / – | 93.0 / **63.4** / – |
| scroll20k | 131.0 / 132.3 / – | 8 / 8 / – | 0.4 / **0.0** / – | 8.23 / 8.62 / – | 66.6 / **44.4** / – |
| live20k | 128.6 / 126.6 / – | 8 / 8 / – | 0.7 / **0.0** / – | **8.59** / 9.15 / – | 90.7 / **63.6** / – |
| storm20k | 114.3 / **120.9** / – | 8 / 8 / – | 2.2 / **0.0** / – | 9.38 / **8.64** / – | 85.5 / **66.9** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 12.25 / **3.06** / – | – / 2 / – | **0.0** / 0.0 / – |
| tick2k | 17.04 / **13.40** / – | – / – / – | 7.2 / **3.6** / – |
| jump2k | 29.77 / **20.77** / – | 27 / **49** / – | 61.5 / **38.1** / – |
| load20k | 11.08 / **7.66** / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | 14.24 / **7.27** / – | – / – / – | 4.4 / **0.0** / – |
| jump20k | 24.96 / **7.30** / – | 58 / 56 / – | 65.4 / **33.6** / – |
| idle2k | **0.23** / 1.58 / – | – / – / – | 1.5 / **1.0** / – |
| idle20k | **0.20** / 2.18 / – | – / – / – | 1.6 / **0.6** / – |

### Across phones · median presented fps

| phase | LMQ720163b9ae8 | R3CY407X32L | RFGL1225WVM |
|---|---|---|---|
| scroll2k | – / – / – | 117.1 / **134.5** / – | 129.8 / 133.5 / – |
| live2k | – / – / – | 66.6 / **126.0** / – | 126.0 / 126.9 / – |
| storm2k | – / – / – | 47.7 / **119.6** / – | 118.8 / 118.3 / – |
| scroll20k | – / – / – | 117.1 / **134.2** / – | 131.0 / 132.3 / – |
| live20k | – / – / – | 68.6 / **125.8** / – | 128.6 / 126.6 / – |
| storm20k | – / – / – | 45.3 / **123.7** / – | 114.3 / **120.9** / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
