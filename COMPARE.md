# Valdi vs Vieww — Market Watch

run: https://github.com/tejakota/vieww_prod/actions/runs/43612904937 · vieww ea15356228 · valdi e90a983749b384ba403c3bcdd40f7d0432404694 · phones: failure

Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### LMQ720340f6afa · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: TIMEOUT

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 268 / 267 / 477 |
| memory after the run, PSS (MB) | **89.6** / 178.7 / 348.6 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | – / – / – | 186.1 / **26.3** / – |
| live2k | – / – / – | – / – / – | – / – / – | – / – / – | 184.0 / **41.7** / – |
| storm2k | – / – / – | – / – / – | – / – / – | – / – / – | 127.1 / **59.0** / – |
| scroll20k | – / – / – | – / – / – | – / – / – | – / – / – | 190.2 / **24.5** / – |
| live20k | – / – / – | – / – / – | – / – / – | – / – / – | 186.1 / **43.5** / – |
| storm20k | – / – / – | – / – / – | – / – / – | – / – / – | 138.5 / **57.5** / – |
| thermal | – / – / – | – / – / – | – / – / – | – / – / – | 183.3 / **41.5** / – |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | – / – / – | – / – / – | **0.0** / 7.3 / – |
| tick2k | – / – / – | – / – / – | 13.8 / **1.8** / – |
| jump2k | – / – / – | – / – / – | 176.8 / **41.5** / – |
| load20k | – / – / – | – / – / – | **0.0** / 0.9 / – |
| tick20k | – / – / – | – / – / – | 72.5 / **0.0** / – |
| jump20k | – / – / – | – / – / – | 257.7 / **25.6** / – |
| idle2k | – / – / – | – / – / – | **0.0** / 0.0 / – |
| idle20k | – / – / – | – / – / – | **0.0** / 0.0 / – |

### RFCN20N6DYW · Android 13 · SM8250 · 10.4 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: TIMEOUT

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 140 / 145 / 168 |
| memory after the run, PSS (MB) | **168.8** / 217.6 / 915.3 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 36.3 / **72.6** / – | 50 / **16** / – | 87.5 / **0.0** / – | – / – / – | 99.9 / **65.4** / – |
| live2k | 20.3 / **66.6** / – | 66 / **16** / – | 100.0 / **0.0** / – | – / – / – | 100.2 / **70.1** / – |
| storm2k | 29.5 / **62.5** / – | 32 / **16** / – | 100.0 / **0.0** / – | – / – / – | 92.4 / **80.9** / – |
| scroll20k | 35.7 / **73.6** / – | 50 / **16** / – | 89.4 / **0.0** / – | – / – / – | 100.6 / **68.5** / – |
| live20k | 20.0 / **66.7** / – | 66 / **16** / – | 100.0 / **0.0** / – | – / – / – | 99.5 / **70.7** / – |
| storm20k | 30.8 / **58.9** / – | 32 / **16** / – | 98.7 / **0.0** / – | – / – / – | 96.2 / **76.0** / – |
| thermal | 17.5 / **61.7** / – | 66 / **16** / – | 100.0 / **0.0** / – | – / – / – | 98.7 / **67.9** / – |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | – / – / – | – / 2 / – | **0.3** / 1.6 / – |
| tick2k | – / – / – | – / – / – | 11.6 / **9.1** / – |
| jump2k | – / – / – | 52 / **83** / – | 100.5 / **70.4** / – |
| load20k | – / – / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | – / – / – | – / 11 / – | **0.0** / 4.9 / – |
| jump20k | – / – / – | 53 / **97** / – | 99.3 / **59.4** / – |
| idle2k | – / – / – | – / – / – | 7.5 / **5.3** / – |
| idle20k | – / – / – | – / – / – | 3.5 / **0.9** / – |

### R3GL10AHD8A · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: TIMEOUT

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 74 / 75 / 128 |
| memory after the run, PSS (MB) | **99.4** / 280.3 / 620.3 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 62.2 / 65.0 / – | 20 / **16** / – | 100.0 / 100.0 / – | – / – / – | 81.4 / **26.9** / – |
| live2k | 63.4 / 62.4 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 79.5 / **49.0** / – |
| storm2k | 61.2 / 59.9 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 79.9 / **58.9** / – |
| scroll20k | 65.3 / 66.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 81.3 / **24.5** / – |
| live20k | 63.0 / 63.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 77.6 / **54.2** / – |
| storm20k | 58.2 / 59.1 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 76.8 / **56.9** / – |
| thermal | 59.8 / 60.3 / – | 16 / 16 / – | 100.0 / 100.0 / – | – / – / – | 79.1 / **53.8** / – |

| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | – / – / – | – / 2 / – | **0.3** / 0.6 / – |
| tick2k | – / – / – | – / – / – | 4.2 / 4.2 / – |
| jump2k | – / – / – | **29** / 27 / – | 76.9 / **51.1** / – |
| load20k | – / – / – | – / – / – | **0.0** / 0.0 / – |
| tick20k | – / – / – | – / – / – | 6.1 / **1.8** / – |
| jump20k | – / – / – | 45 / **60** / – | 84.7 / **20.8** / – |
| idle2k | – / – / – | – / – / – | **2.5** / 3.5 / – |
| idle20k | – / – / – | – / – / – | 2.2 / 2.1 / – |

### Across phones · median presented fps

| phase | LMQ720340f6afa | RFCN20N6DYW | R3GL10AHD8A |
|---|---|---|---|
| scroll2k | – / – / – | 36.3 / **72.6** / – | 62.2 / 65.0 / – |
| live2k | – / – / – | 20.3 / **66.6** / – | 63.4 / 62.4 / – |
| storm2k | – / – / – | 29.5 / **62.5** / – | 61.2 / 59.9 / – |
| scroll20k | – / – / – | 35.7 / **73.6** / – | 65.3 / 66.5 / – |
| live20k | – / – / – | 20.0 / **66.7** / – | 63.0 / 63.5 / – |
| storm20k | – / – / – | 30.8 / **58.9** / – | 58.2 / 59.1 / – |
| thermal | – / – / – | 17.5 / **61.7** / – | 59.8 / 60.3 / – |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour.
