# Valdi vs Vieww — Market Watch

run: https://github.com/tejakota/vieww_prod/actions/runs/44085244072 · vieww 5892c66ffb · Firebase Test Lab (free)

Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

## Phones (AWS Device Farm)

### Pixel 9 · Android 16 · Tensor G4 · – · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint on (target 16.67 ms)`
- vieww-gpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint on (target 16.67 ms)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 59.9 / 60.3 / – | – / – / – | **0.0** / 0.0 / – | **10.91** / 18.73 / – | – / – / – |
| live2k | 59.6 / 59.9 / – | – / – / – | 0.2 / **0.0** / – | **15.96** / 18.93 / – | – / – / – |
| storm2k | 59.2 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 18.43 / 18.85 / – | – / – / – |
| scroll20k | 60.0 / 60.3 / – | – / – / – | **0.0** / 0.0 / – | **12.40** / 18.46 / – | – / – / – |
| live20k | 59.9 / 59.9 / – | – / – / – | **0.0** / 0.0 / – | **14.66** / 18.83 / – | – / – / – |
| storm20k | 59.5 / 60.0 / – | – / – / – | 0.2 / **0.0** / – | **17.70** / 19.22 / – | – / – / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 25.09 / **10.60** / – | – / – / – | – / – / – |
| tick2k | 13.27 / **6.78** / – | – / – / – | – / – / – |
| jump2k | 40.87 / **12.46** / – | – / – / – | – / – / – |
| load20k | 22.85 / **9.59** / – | – / – / – | – / – / – |
| tick20k | 12.59 / **6.42** / – | – / – / – | – / – / – |
| jump20k | 33.30 / **17.46** / – | – / – / – | – / – / – |
| idle2k | **0.20** / 1.71 / – | – / – / – | – / – / – |
| idle20k | **0.23** / 1.71 / – | – / – / – | – / – / – |

### Pixel 9 Pro · Android 15 · Tensor G4 · – · 120.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 117.6 / 120.5 / – | – / – / – | 1.1 / **0.1** / – | **8.80** / 9.51 / – | – / – / – |
| live2k | 117.5 / 120.1 / – | – / – / – | 0.3 / **0.1** / – | 9.41 / 9.51 / – | – / – / – |
| storm2k | 103.5 / **120.1** / – | – / – / – | 0.5 / **0.0** / – | 10.80 / **9.60** / – | – / – / – |
| scroll20k | 118.5 / 120.4 / – | – / – / – | **0.1** / 0.3 / – | 8.74 / 9.16 / – | – / – / – |
| live20k | 118.3 / 120.1 / – | – / – / – | **0.0** / 0.0 / – | 9.12 / 9.48 / – | – / – / – |
| storm20k | 101.6 / **120.0** / – | – / – / – | 0.7 / **0.3** / – | 10.99 / **9.44** / – | – / – / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 29.73 / **4.79** / – | – / – / – | – / – / – |
| tick2k | 15.48 / **11.04** / – | – / – / – | – / – / – |
| jump2k | **32.91** / 38.50 / – | – / – / – | – / – / – |
| load20k | 18.73 / **10.39** / – | – / – / – | – / – / – |
| tick20k | 11.03 / **7.81** / – | – / – / – | – / – / – |
| jump20k | 37.96 / **17.83** / – | – / – / – | – / – / – |
| idle2k | **0.43** / 2.08 / – | – / – / – | – / – / – |
| idle20k | **0.37** / 4.67 / – | – / – / – | – / – / – |

### Pixel 9 Pro XL · Android 15 · Tensor G4 · – · 120.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | – / – / – |
| memory after the run, PSS (MB) | – / – / – |
| APK (MB) | – / – / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 118.3 / 120.6 / – | – / – / – | 0.3 / **0.2** / – | **8.88** / 9.36 / – | – / – / – |
| live2k | 118.8 / 120.1 / – | – / – / – | **0.0** / 0.0 / – | **8.81** / 9.55 / – | – / – / – |
| storm2k | 96.7 / **120.0** / – | – / – / – | 0.5 / **0.3** / – | 11.49 / **9.63** / – | – / – / – |
| scroll20k | 117.7 / 120.6 / – | – / – / – | 0.5 / **0.2** / – | 9.15 / 9.17 / – | – / – / – |
| live20k | 118.2 / 120.1 / – | – / – / – | **0.0** / 0.2 / – | 9.16 / 9.57 / – | – / – / – |
| storm20k | 97.1 / **120.1** / – | – / – / – | 0.8 / **0.1** / – | 11.31 / **9.64** / – | – / – / – |


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 26.33 / **4.53** / – | – / – / – | – / – / – |
| tick2k | 17.41 / **9.00** / – | – / – / – | – / – / – |
| jump2k | 50.73 / **21.16** / – | – / – / – | – / – / – |
| load20k | 19.54 / **9.95** / – | – / – / – | – / – / – |
| tick20k | 17.61 / **10.74** / – | – / – / – | – / – / – |
| jump20k | 68.54 / **16.49** / – | – / – / – | – / – / – |
| idle2k | **0.19** / 2.53 / – | – / – / – | – / – / – |
| idle20k | **0.78** / 5.04 / – | – / – / – | – / – / – |

### Across phones · median presented fps

| phase | Pixel 9 | Pixel 9 Pro | Pixel 9 Pro XL |
|---|---|---|---|
| scroll2k | 59.9 / 60.3 / – | 117.6 / 120.5 / – | 118.3 / 120.6 / – |
| live2k | 59.6 / 59.9 / – | 117.5 / 120.1 / – | 118.8 / 120.1 / – |
| storm2k | 59.2 / 60.0 / – | 103.5 / **120.1** / – | 96.7 / **120.0** / – |
| scroll20k | 60.0 / 60.3 / – | 118.5 / 120.4 / – | 117.7 / 120.6 / – |
| live20k | 59.9 / 59.9 / – | 118.3 / 120.1 / – | 118.2 / 120.1 / – |
| storm20k | 59.5 / 60.0 / – | 101.6 / **120.0** / – | 97.1 / **120.1** / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
