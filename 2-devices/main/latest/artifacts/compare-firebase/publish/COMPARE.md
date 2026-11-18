# Valdi vs Vieww — Market Watch

run: https://github.com/tejakota/vieww_prod/actions/runs/44340953404 · vieww ee5a473927 · Firebase Test Lab (free)

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

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 59.8 / 58.6 / – | – / – / – | **0.0** / 2.6 / – | 9.85 / **5.66** / – | **1.43** / 16.57 / – | – / – / – |
| live2k | 59.8 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 13.39 / **5.99** / – | **1.63** / 15.52 / – | – / – / – |
| storm2k | 59.6 / 60.0 / – | – / – / – | **0.0** / 0.2 / – | 16.63 / **7.98** / – | **0.49** / 14.87 / – | – / – / – |
| scroll20k | 59.8 / 60.3 / – | – / – / – | **0.0** / 0.0 / – | 11.16 / **3.88** / – | **2.28** / 16.77 / – | – / – / – |
| live20k | 59.8 / 59.9 / – | – / – / – | **0.0** / 0.0 / – | 14.05 / **6.32** / – | **0.63** / 15.06 / – | – / – / – |
| storm20k | 59.3 / 60.0 / – | – / – / – | **0.2** / 0.4 / – | 17.77 / **8.46** / – | **0.56** / 14.94 / – | – / – / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 20.98 / **4.97** / – | 0.50 / **0.01** / – | – / – / – | – / – / – |
| tick2k | 25.75 / 25.39 / – | 0.42 / **0.05** / – | – / – / – | – / – / – |
| jump2k | 45.96 / **20.17** / – | 0.39 / **0.03** / – | – / – / – | – / – / – |
| load20k | 19.29 / **7.41** / – | 0.26 / **0.04** / – | – / – / – | – / – / – |
| tick20k | 19.79 / **9.82** / – | 0.56 / **0.25** / – | – / – / – | – / – / – |
| jump20k | 32.40 / **20.87** / – | 0.42 / **0.11** / – | – / – / – | – / – / – |
| idle2k | 0.26 / **0.21** / – | **0.00** / 0.00 / – | – / – / – | – / – / – |
| idle20k | 0.85 / **0.18** / – | **0.00** / 0.00 / – | – / – / – | – / – / – |

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

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 119.6 / 116.7 / – | – / – / – | **0.1** / 1.6 / – | 5.98 / **3.50** / – | **2.62** / 7.21 / – | – / – / – |
| live2k | 119.1 / 120.0 / – | – / – / – | **0.0** / 0.2 / – | 8.14 / **4.83** / – | **0.30** / 6.14 / – | – / – / – |
| storm2k | 119.6 / 120.1 / – | – / – / – | **0.0** / 0.1 / – | 7.58 / **7.18** / – | **0.28** / 4.45 / – | – / – / – |
| scroll20k | 119.5 / 120.5 / – | – / – / – | **0.0** / 0.2 / – | 6.64 / **3.25** / – | **0.49** / 7.28 / – | – / – / – |
| live20k | 118.1 / 120.0 / – | – / – / – | 0.5 / **0.1** / – | 8.62 / **5.01** / – | **0.34** / 5.94 / – | – / – / – |
| storm20k | 119.7 / 120.1 / – | – / – / – | **0.0** / 0.0 / – | 7.24 / 7.61 / – | **0.28** / 4.32 / – | – / – / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 11.04 / **5.06** / – | 0.31 / **0.02** / – | – / – / – | – / – / – |
| tick2k | **8.49** / 11.04 / – | 0.37 / **0.06** / – | – / – / – | – / – / – |
| jump2k | 57.21 / **16.66** / – | 0.29 / **0.03** / – | – / – / – | – / – / – |
| load20k | 14.90 / **8.60** / – | 0.18 / **0.04** / – | – / – / – | – / – / – |
| tick20k | 6.95 / **5.00** / – | 0.53 / **0.08** / – | – / – / – | – / – / – |
| jump20k | 36.38 / **13.35** / – | 0.52 / **0.17** / – | – / – / – | – / – / – |
| idle2k | 0.37 / **0.17** / – | **0.00** / 0.00 / – | – / – / – | – / – / – |
| idle20k | 0.51 / **0.22** / – | **0.00** / 0.00 / – | – / – / – | – / – / – |

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

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 119.4 / 116.1 / – | – / – / – | **0.0** / 1.8 / – | 6.56 / **3.16** / – | **1.95** / 7.66 / – | – / – / – |
| live2k | 118.8 / 120.1 / – | – / – / – | 0.3 / 0.3 / – | 8.22 / **5.27** / – | **0.34** / 6.10 / – | – / – / – |
| storm2k | 119.6 / 120.1 / – | – / – / – | **0.0** / 0.2 / – | 7.61 / 7.29 / – | **0.24** / 4.58 / – | – / – / – |
| scroll20k | 119.3 / 120.5 / – | – / – / – | **0.1** / 0.4 / – | 6.99 / **3.04** / – | **0.51** / 7.46 / – | – / – / – |
| live20k | 118.3 / 120.1 / – | – / – / – | **0.1** / 0.3 / – | 8.66 / **5.39** / – | **0.37** / 5.82 / – | – / – / – |
| storm20k | 119.6 / 120.0 / – | – / – / – | **0.0** / 0.0 / – | 7.59 / 7.58 / – | **0.26** / 4.29 / – | – / – / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 11.56 / **4.13** / – | 0.56 / **0.01** / – | – / – / – | – / – / – |
| tick2k | **6.27** / 15.92 / – | 0.35 / **0.07** / – | – / – / – | – / – / – |
| jump2k | 30.12 / **20.16** / – | 0.30 / **0.03** / – | – / – / – | – / – / – |
| load20k | 15.11 / **11.33** / – | 0.20 / **0.06** / – | – / – / – | – / – / – |
| tick20k | 11.71 / **8.71** / – | 0.91 / **0.34** / – | – / – / – | – / – / – |
| jump20k | 37.07 / **17.48** / – | 0.41 / **0.28** / – | – / – / – | – / – / – |
| idle2k | **0.13** / 0.38 / – | **0.00** / 0.00 / – | – / – / – | – / – / – |
| idle20k | 0.80 / **0.51** / – | **0.00** / 0.00 / – | – / – / – | – / – / – |

### Across phones · median presented fps

| phase | Pixel 9 | Pixel 9 Pro | Pixel 9 Pro XL |
|---|---|---|---|
| scroll2k | 59.8 / 58.6 / – | 119.6 / 116.7 / – | 119.4 / 116.1 / – |
| live2k | 59.8 / 60.0 / – | 119.1 / 120.0 / – | 118.8 / 120.1 / – |
| storm2k | 59.6 / 60.0 / – | 119.6 / 120.1 / – | 119.6 / 120.1 / – |
| scroll20k | 59.8 / 60.3 / – | 119.5 / 120.5 / – | 119.3 / 120.5 / – |
| live20k | 59.8 / 59.9 / – | 118.1 / 120.0 / – | 118.3 / 120.1 / – |
| storm20k | 59.3 / 60.0 / – | 119.7 / 120.1 / – | 119.6 / 120.0 / – |
| thermal | – / – / – | – / – / – | – / – / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- **App wait** is the other half of a frame's wall clock beside app work: what the display made the frame wait (the image acquire and the flight-slot fence). Runs from before the field was logged show a dash there — and their work counts the waits in, so it runs high beside a new run's work + wait.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
