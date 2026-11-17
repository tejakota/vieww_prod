# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ72016369b1d · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`
- vieww-gpu frame rate: `window#primary is on Android Device, which reports no refresh rate; keeping the current budget` · `performance hint unavailable (Android 12 or older: no APerformanceHint)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 368 / **339** / – |
| memory after the run, PSS (MB) | **81.8** / 184.5 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **31.08** / 167.81 / – | 147.3 / **42.5** / – |
| live2k | – / – / – | – / – / – | – / – / – | **52.65** / 63.82 / – | 132.8 / **57.1** / – |
| storm2k | – / – / – | – / – / – | – / – / – | 76.24 / **61.20** / – | 110.2 / **76.1** / – |
| scroll20k | – / – / – | – / – / – | – / – / – | **31.15** / 59.70 / – | 146.9 / **27.8** / – |
| live20k | – / – / – | – / – / – | – / – / – | **53.63** / 61.35 / – | 131.6 / **58.8** / – |
| storm20k | – / – / – | – / – / – | – / – / – | 75.24 / **61.19** / – | 110.3 / **76.7** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 210.96 / **46.53** / – | – / – / – | **0.0** / 1.0 / – |
| tick2k | 46.70 / **18.56** / – | – / – / – | **0.0** / 12.0 / – |
| jump2k | 170.75 / **70.42** / – | – / – / – | 146.6 / **41.3** / – |
| load20k | 212.41 / **38.64** / – | – / – / – | **0.0** / 3.0 / – |
| tick20k | 48.57 / **24.15** / – | – / – / – | **0.0** / 45.1 / – |
| jump20k | 149.42 / **56.94** / – | – / – / – | 126.1 / **24.0** / – |
| idle2k | **0.90** / 13.28 / – | – / – / – | 5.8 / **0.0** / – |
| idle20k | **0.92** / 710.74 / – | – / – / – | 10.4 / **0.0** / – |

### RFCN2083V5Y · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint unavailable (the system declined a session)`
- vieww-gpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint unavailable (the system declined a session)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 179 / **170** / – |
| memory after the run, PSS (MB) | **142.0** / 170.6 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 69.2 / 71.0 / – | 16 / 16 / – | 1.1 / **0.0** / – | **15.96** / 18.76 / – | 93.7 / **39.8** / – |
| live2k | 62.0 / 64.9 / – | 17 / **16** / – | 3.6 / **0.0** / – | 18.99 / 18.51 / – | 118.2 / **65.8** / – |
| storm2k | 53.1 / **63.1** / – | 33 / **16** / – | 13.2 / **0.0** / – | 21.76 / **17.84** / – | 105.7 / **71.3** / – |
| scroll20k | 71.7 / 72.6 / – | 16 / 16 / – | 1.0 / **0.0** / – | **15.91** / 18.57 / – | 90.4 / **38.1** / – |
| live20k | 63.2 / 65.5 / – | 16 / 16 / – | 3.3 / **0.0** / – | 19.24 / **18.09** / – | 121.2 / **66.0** / – |
| storm20k | 55.7 / **60.4** / – | 33 / **16** / – | 12.5 / **0.0** / – | 21.79 / **17.55** / – | 108.8 / **63.0** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 53.61 / **6.25** / – | – / 2 / – | **0.0** / 0.3 / – |
| tick2k | 20.00 / 20.63 / – | – / – / – | **0.0** / 0.0 / – |
| jump2k | 74.03 / **24.69** / – | 16 / **61** / – | 84.3 / **35.3** / – |
| load20k | 47.60 / **14.90** / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | 23.11 / **4.00** / – | – / – / – | 12.6 / **3.1** / – |
| jump20k | 76.32 / **24.38** / – | 60 / **71** / – | 138.3 / **44.2** / – |
| idle2k | **0.18** / 3.25 / – | – / – / – | **3.2** / 5.1 / – |
| idle20k | **0.28** / 2.37 / – | – / – / – | **0.0** / 0.0 / – |

### RFGL126MAVW · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Quick run: Vieww CPU and GPU only, no thermal pass. Valdi is pinned, so its numbers stand from the last full report._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 101 / 104 / – |
| memory after the run, PSS (MB) | **98.6** / 219.5 / – |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 130.7 / 131.5 / – | 8 / 8 / – | 0.4 / **0.0** / – | **8.31** / 9.44 / – | 69.8 / **46.3** / – |
| live2k | 124.3 / 126.2 / – | 8 / 8 / – | 0.7 / **0.0** / – | **8.70** / 9.71 / – | 91.6 / **64.3** / – |
| storm2k | 116.9 / **122.9** / – | 8 / 8 / – | **0.0** / 0.0 / – | **7.21** / 9.93 / – | 93.6 / **67.7** / – |
| scroll20k | 130.5 / 130.7 / – | 8 / 8 / – | 0.3 / **0.0** / – | **8.12** / 8.63 / – | 58.8 / **42.0** / – |
| live20k | 124.6 / 125.3 / – | 8 / 8 / – | 1.1 / **0.0** / – | **8.85** / 9.45 / – | 91.7 / **64.4** / – |
| storm20k | 119.6 / 122.1 / – | 8 / 8 / – | 3.3 / **0.0** / – | 9.71 / **8.67** / – | 92.1 / **68.6** / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 9.68 / **2.95** / – | – / 2 / – | **0.0** / 0.3 / – |
| tick2k | 15.96 / **14.54** / – | – / – / – | **3.2** / 4.0 / – |
| jump2k | 34.80 / **20.25** / – | 37 / **56** / – | 50.0 / **43.6** / – |
| load20k | 10.97 / **7.19** / – | – / – / – | **0.0** / 0.6 / – |
| tick20k | 18.43 / **7.79** / – | – / – / – | 10.0 / **3.5** / – |
| jump20k | 24.71 / **14.29** / – | **45** / 35 / – | **28.3** / 64.9 / – |
| idle2k | **0.17** / 2.62 / – | – / – / – | **1.6** / 2.2 / – |
| idle20k | **0.20** / 2.15 / – | – / – / – | 1.9 / **0.9** / – |

### Across phones · median presented fps

| phase | LMQ72016369b1d | RFCN2083V5Y | RFGL126MAVW |
|---|---|---|---|
| scroll2k | – / – / – | 69.2 / 71.0 / – | 130.7 / 131.5 / – |
| live2k | – / – / – | 62.0 / 64.9 / – | 124.3 / 126.2 / – |
| storm2k | – / – / – | 53.1 / **63.1** / – | 116.9 / **122.9** / – |
| scroll20k | – / – / – | 71.7 / 72.6 / – | 130.5 / 130.7 / – |
| live20k | – / – / – | 63.2 / 65.5 / – | 124.6 / 125.3 / – |
| storm20k | – / – / – | 55.7 / **60.4** / – | 119.6 / 122.1 / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
