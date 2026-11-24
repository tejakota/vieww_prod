# Valdi vs Vieww — Market Watch

run: https://github.com/tejakota/vieww_prod/actions/runs/45325161447 · vieww c1054f27a1 · Firebase Test Lab (free)

Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

## Phones (Firebase Test Lab)

### Pixel 9 · Android 16 · Tensor G4 · – · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint on (target 16.67 ms)`
- vieww-gpu frame rate: `window#primary reports 60.0Hz; frame budget follows it` · `performance hint on (target 16.67 ms)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start (ms) | – / – / – |
| memory after the run, PSS (MB) | 86.6 / **48.4** / – |
| APK (MB) | 2.8 / 2.8 / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 59.9 / 58.7 / – | – / – / – | **0.0** / 1.9 / – | 7.30 / **5.47** / – | **0.39** / 16.30 / – | 52.7 / **25.5** / – |
| live2k | 59.9 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 11.67 / **4.84** / – | **0.38** / 15.37 / – | 77.2 / **31.1** / – |
| storm2k | 59.7 / 59.9 / – | – / – / – | **0.0** / 0.0 / – | 16.04 / **5.86** / – | **0.33** / 14.45 / – | 90.1 / **37.2** / – |
| scroll20k | 59.7 / 60.3 / – | – / – / – | **0.0** / 0.0 / – | 8.80 / **3.09** / – | **0.45** / 16.38 / – | 60.0 / **21.9** / – |
| live20k | 59.9 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 11.61 / **4.92** / – | **2.65** / 15.27 / – | 83.1 / **32.0** / – |
| storm20k | 59.7 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 16.32 / **5.82** / – | **0.80** / 14.41 / – | 90.0 / **37.3** / – |
| thermal | 59.8 / 60.0 / – | – / – / – | **0.0** / 0.0 / – | 11.56 / **4.62** / – | **0.53** / 15.79 / – | 81.8 / **28.7** / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 18.86 / **5.66** / – | 0.48 / **0.01** / – | – / – / – | **2.3** / 6.0 / – |
| tick2k | **5.84** / 8.93 / – | 0.29 / **0.04** / – | – / – / – | 2.0 / **1.0** / – |
| jump2k | 27.12 / **10.59** / – | 0.31 / **0.03** / – | – / – / – | 2.0 / **1.0** / – |
| load20k | 18.73 / **5.77** / – | 0.32 / **0.05** / – | – / – / – | 1.5 / **1.0** / – |
| tick20k | 7.08 / **2.78** / – | 0.34 / **0.08** / – | – / – / – | 1.0 / 1.0 / – |
| jump20k | 33.91 / **7.79** / – | 0.25 / **0.09** / – | – / – / – | 4.0 / **1.0** / – |
| idle2k | 0.18 / **0.15** / – | **0.00** / 0.00 / – | – / – / – | 0.5 / **0.0** / – |
| idle20k | 0.21 / **0.16** / – | **0.00** / 0.00 / – | – / – / – | 1.5 / **0.5** / – |

### Pixel 9 Pro · Android 15 · Tensor G4 · – · 120.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start (ms) | – / – / – |
| memory after the run, PSS (MB) | 76.3 / **49.7** / – |
| APK (MB) | 2.8 / 2.8 / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 119.6 / 116.6 / – | – / – / – | **0.0** / 1.7 / – | 5.64 / **3.21** / – | **2.98** / 7.28 / – | 85.7 / **39.8** / – |
| live2k | 119.4 / 120.0 / – | – / – / – | 0.2 / **0.0** / – | 7.65 / **5.01** / – | **1.10** / 5.65 / – | 118.7 / **56.6** / – |
| storm2k | 119.6 / 120.0 / – | – / – / – | **0.0** / 0.3 / – | 7.45 / **6.93** / – | **0.86** / 4.50 / – | 101.6 / **72.9** / – |
| scroll20k | 118.9 / 120.4 / – | – / – / – | 0.1 / 0.1 / – | 6.26 / **2.78** / – | **3.31** / 7.15 / – | 99.0 / **37.6** / – |
| live20k | 117.8 / 120.0 / – | – / – / – | 0.5 / **0.1** / – | 8.02 / **5.43** / – | **2.40** / 5.58 / – | 116.9 / **56.5** / – |
| storm20k | 119.7 / 119.9 / – | – / – / – | **0.0** / 0.0 / – | 7.31 / 7.36 / – | **1.11** / 4.63 / – | 112.4 / **72.7** / – |
| thermal | 118.0 / 120.1 / – | – / – / – | 0.4 / **0.2** / – | 8.67 / **4.92** / – | **0.43** / 6.41 / – | 120.0 / **50.8** / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 15.97 / **5.35** / – | 0.42 / **0.02** / – | – / – / – | **2.0** / 4.9 / – |
| tick2k | 12.26 / **10.61** / – | 0.26 / **0.04** / – | – / – / – | **0.0** / 2.0 / – |
| jump2k | 49.72 / **43.33** / – | 0.35 / **0.11** / – | – / – / – | 5.0 / **4.0** / – |
| load20k | 15.83 / **10.84** / – | 0.20 / **0.06** / – | – / – / – | **1.5** / 2.0 / – |
| tick20k | 4.63 / **3.13** / – | 0.28 / **0.09** / – | – / – / – | **0.0** / 0.0 / – |
| jump20k | 22.16 / **9.53** / – | 0.34 / **0.07** / – | – / – / – | 4.0 / **2.0** / – |
| idle2k | 0.15 / **0.11** / – | **0.00** / 0.00 / – | – / – / – | **0.5** / 1.5 / – |
| idle20k | 0.31 / **0.25** / – | **0.00** / 0.00 / – | – / – / – | **0.5** / 1.0 / – |

### Pixel 9 Pro XL · Android 15 · Tensor G4 · – · 120.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS
- vieww-cpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`
- vieww-gpu frame rate: `window#primary reports 120.0Hz; frame budget follows it, falling to 60 Hz while it cannot be held` · `performance hint on (target 8.33 ms)` · `window#primary asks for 120 Hz at start; asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)`

_Firebase Test Lab (free): Vieww CPU and GPU only. Presented fps and jank come from the app's own frame log, as Test Lab has no compositor counts._
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start (ms) | – / – / – |
| memory after the run, PSS (MB) | 79.0 / **48.4** / – |
| APK (MB) | 2.8 / 2.8 / – |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | app wait p95 (ms) | CPU % |
|---|---|---|---|---|---|---|
| scroll2k | 119.2 / 116.3 / – | – / – / – | **0.2** / 1.5 / – | 6.75 / **3.01** / – | **2.61** / 7.04 / – | 85.8 / **40.1** / – |
| live2k | 118.9 / 120.1 / – | – / – / – | **0.1** / 0.2 / – | 8.23 / **5.13** / – | **0.33** / 5.83 / – | 120.4 / **56.7** / – |
| storm2k | 119.5 / 120.0 / – | – / – / – | **0.0** / 0.2 / – | 7.81 / **7.40** / – | **0.21** / 4.61 / – | 103.5 / **72.8** / – |
| scroll20k | 119.0 / 120.5 / – | – / – / – | 0.3 / **0.2** / – | 6.65 / **2.89** / – | **0.49** / 7.23 / – | 93.0 / **36.9** / – |
| live20k | 117.8 / 120.1 / – | – / – / – | 0.4 / **0.3** / – | 8.70 / **5.36** / – | **0.39** / 5.69 / – | 126.9 / **55.4** / – |
| storm20k | 119.7 / 119.9 / – | – / – / – | **0.0** / 0.8 / – | 7.53 / 7.64 / – | **0.22** / 4.71 / – | 111.3 / **77.2** / – |
| thermal | 118.3 / 120.1 / – | – / – / – | 0.5 / **0.2** / – | 8.46 / **5.39** / – | **0.37** / 5.62 / – | 123.7 / **57.0** / – |


| event | app time (ms) | app wait (ms) | frames presented | CPU % |
|---|---|---|---|---|
| load2k | 17.97 / **4.85** / – | 0.44 / **0.02** / – | – / – / – | **2.0** / 4.9 / – |
| tick2k | **8.07** / 13.00 / – | 0.36 / **0.03** / – | – / – / – | **1.0** / 2.0 / – |
| jump2k | 41.40 / **20.78** / – | 0.35 / **0.08** / – | – / – / – | 5.0 / **2.0** / – |
| load20k | 16.34 / **6.56** / – | 0.18 / **0.04** / – | – / – / – | **1.0** / 1.5 / – |
| tick20k | 5.82 / **2.75** / – | 0.48 / **0.06** / – | – / – / – | 1.0 / **0.0** / – |
| jump20k | 27.31 / **9.51** / – | 0.39 / **0.07** / – | – / – / – | 3.0 / 3.0 / – |
| idle2k | 0.18 / **0.10** / – | **0.00** / 0.00 / – | – / – / – | 0.5 / **0.0** / – |
| idle20k | 0.31 / **0.12** / – | **0.00** / 0.00 / – | – / – / – | 2.5 / **0.0** / – |

### Across phones · median presented fps

| phase | Pixel 9 | Pixel 9 Pro | Pixel 9 Pro XL |
|---|---|---|---|
| scroll2k | 59.9 / 58.7 / – | 119.6 / 116.6 / – | 119.2 / 116.3 / – |
| live2k | 59.9 / 60.0 / – | 119.4 / 120.0 / – | 118.9 / 120.1 / – |
| storm2k | 59.7 / 59.9 / – | 119.6 / 120.0 / – | 119.5 / 120.0 / – |
| scroll20k | 59.7 / 60.3 / – | 118.9 / 120.4 / – | 119.0 / 120.5 / – |
| live20k | 59.9 / 60.0 / – | 117.8 / 120.0 / – | 117.8 / 120.1 / – |
| storm20k | 59.7 / 60.0 / – | 119.7 / 119.9 / – | 119.7 / 119.9 / – |
| thermal | 59.8 / 60.0 / – | 118.0 / 120.1 / – | 118.3 / 120.1 / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- **App wait** is the other half of a frame's wall clock beside app work: what the display made the frame wait (the image acquire and the flight-slot fence). Runs from before the field was logged show a dash there — and their work counts the waits in, so it runs high beside a new run's work + wait.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
