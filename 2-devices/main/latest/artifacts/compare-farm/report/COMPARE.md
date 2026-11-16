# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### 0B301FDD400775 · Android 11 · lito · 7.3 GB · 90.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 160 / **136** / 149 |
| memory after the run, PSS (MB) | **88.0** / 93.7 / 409.2 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 67.6 / 69.4 / **81.8** | 16 / 16 / **11** | 3.7 / 2.0 / **0.2** | **14.66** / 18.62 / – | 82.6 / **34.8** / 68.2 |
| live2k | 57.5 / 62.7 / **87.2** | 33 / **16** / 22 | 14.2 / **1.1** / 6.3 | 20.08 / **18.76** / – | 112.1 / **46.0** / 113.0 |
| storm2k | 34.5 / 60.3 / 62.9 | 33 / **16** / 33 | 67.6 / **2.7** / 41.4 | 29.90 / **18.88** / – | 100.4 / **57.0** / 120.3 |
| scroll20k | 63.9 / **70.3** / – | 16 / 16 / – | 4.1 / **1.8** / – | 19.43 / **18.12** / – | 77.2 / **32.1** / 99.8 |
| live20k | 50.0 / **62.9** / – | 33 / **16** / – | 26.2 / **2.4** / – | 28.21 / **18.72** / – | 102.2 / **45.4** / 99.5 |
| storm20k | 37.5 / **61.6** / – | 33 / **16** / – | 62.2 / **4.5** / – | 29.60 / **18.92** / – | 103.9 / **59.0** / 99.4 |
| thermal | 57.3 / **60.6** / – | 33 / **16** / – | 9.6 / **2.8** / – | 19.78 / **18.76** / – | 113.0 / **45.4** / 99.1 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 5.51 / 5.36 | 4.34 / 4.25 | 14.66 / 19.48 | 3.7 / 7.2 |
| live2k | 11.71 / 11.48 | 4.27 / 4.23 | 20.08 / 19.63 | 14.2 / 9.1 |
| storm2k | 23.00 / 22.34 | 2.82 / 2.79 | 29.90 / 28.60 | 67.6 / 64.2 |
| scroll20k | 5.65 / 5.49 | 4.71 / 4.19 | 19.43 / 19.13 | 4.1 / 3.9 |
| live20k | 12.93 / 11.72 | 4.86 / 4.23 | 28.21 / 19.80 | 26.2 / 12.7 |
| storm20k | 21.71 / 22.41 | 2.95 / 2.88 | 29.60 / 29.45 | 62.2 / 62.4 |
| thermal | 11.58 / 11.51 | 4.25 / 4.21 | 19.78 / 19.52 | 9.6 / 8.6 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.22 / 1.23 / 0.52 / 0.00 / 0.00 \n 0.22 / 1.21 / 0.49 / 0.00 / 0.00 |
| live2k | 0.74 / 4.42 / 0.56 / 0.00 / 0.00 \n 0.74 / 4.34 / 0.55 / 0.00 / 0.00 |
| storm2k | 0.89 / 9.01 / 2.04 / 0.00 / 0.00 \n 0.87 / 8.70 / 2.01 / 0.00 / 0.00 |
| scroll20k | 0.23 / 1.24 / 0.53 / 0.00 / 0.00 \n 0.22 / 1.22 / 0.51 / 0.00 / 0.00 |
| live20k | 0.81 / 4.79 / 0.62 / 0.00 / 0.00 \n 0.75 / 4.45 / 0.55 / 0.00 / 0.00 |
| storm20k | 0.89 / 8.78 / 1.88 / 0.00 / 0.00 \n 0.88 / 8.81 / 1.99 / 0.00 / 0.00 |
| thermal | 0.73 / 4.37 / 0.55 / 0.00 / 0.00 \n 0.73 / 4.37 / 0.54 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 56.50 / **16.20** / – | – / 1 / **24** | **0.9** / 1.4 / 105.9 |
| tick2k | 26.26 / **11.29** / – | – / – / – | 11.4 / 22.2 / **0.0** |
| jump2k | 63.20 / **16.28** / – | **44** / 9 / – | 102.3 / 10.6 / **0.0** |
| load20k | 47.42 / **13.15** / – | – / – / 79 | **0.0** / 0.0 / 118.4 |
| tick20k | 14.86 / **7.54** / – | – / – / – | 10.9 / **0.0** / 100.8 |
| jump20k | 60.97 / **17.54** / – | **16** / 8 / – | 62.5 / **11.0** / 97.8 |
| idle2k | **0.32** / 6.30 / – | – / – / – | 5.4 / **0.8** / 75.1 |
| idle20k | **0.24** / 3.53 / – | – / – / – | 1.7 / **0.9** / 100.3 |

### 31060DLH2000TM · Android 13 · GS201 · 7.3 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 140 / 144 / 139 |
| memory after the run, PSS (MB) | **101.0** / 154.9 / 469.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 68.5 / 81.3 / **94.0** | 16 / 16 / **11** | 0.2 / **0.0** / 0.0 | **14.04** / 17.95 / – | 78.8 / **40.3** / 99.7 |
| live2k | 63.2 / 63.6 / **87.0** | 16 / 16 / 22 | **0.0** / 0.0 / 0.9 | **16.50** / 18.98 / – | 85.5 / **47.6** / 139.2 |
| storm2k | 58.4 / **61.9** / 58.6 | 16 / 16 / 33 | 1.4 / **0.0** / 17.9 | 19.38 / 18.77 / – | 103.6 / **65.5** / 126.1 |
| scroll20k | 67.6 / 66.7 / – | 16 / 16 / – | **0.0** / 0.0 / – | 17.55 / **10.44** / – | 77.5 / **46.9** / 100.0 |
| live20k | 63.5 / 64.1 / – | 16 / 16 / – | **0.0** / 0.0 / – | 17.90 / **15.87** / – | 87.4 / **59.9** / 99.4 |
| storm20k | 60.7 / 61.3 / – | 16 / 16 / – | **0.0** / 0.0 / – | 18.51 / 19.12 / – | 106.1 / **63.9** / 98.9 |
| thermal | 60.2 / 60.7 / – | 16 / 16 / – | **0.0** / 0.0 / – | 17.93 / 18.56 / – | 88.8 / **49.5** / 99.2 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 4.50 / 3.58 | 3.94 / 3.43 | 14.04 / 11.96 | 0.2 / 0.0 |
| live2k | 6.85 / 6.87 | 3.27 / 3.28 | 16.50 / 14.00 | 0.0 / 0.0 |
| storm2k | 13.09 / 12.63 | 2.18 / 2.19 | 19.38 / 18.29 | 1.4 / 0.0 |
| scroll20k | 4.55 / 4.74 | 4.05 / 4.11 | 17.55 / 14.03 | 0.0 / 0.0 |
| live20k | 6.97 / 6.90 | 3.34 / 3.22 | 17.90 / 13.68 | 0.0 / 0.0 |
| storm20k | 12.28 / 12.30 | 2.16 / 2.08 | 18.51 / 18.14 | 0.0 / 0.0 |
| thermal | 6.86 / 6.81 | 3.37 / 3.30 | 17.93 / 17.68 | 0.0 / 0.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.22 / 0.94 / 0.41 / 0.00 / 0.00 \n 0.19 / 0.77 / 0.32 / 0.00 / 0.00 |
| live2k | 0.65 / 2.59 / 0.30 / 0.00 / 0.00 \n 0.65 / 2.59 / 0.31 / 0.00 / 0.00 |
| storm2k | 0.80 / 5.20 / 1.32 / 0.00 / 0.00 \n 0.78 / 5.08 / 1.30 / 0.00 / 0.00 |
| scroll20k | 0.22 / 0.94 / 0.41 / 0.00 / 0.00 \n 0.23 / 0.98 / 0.43 / 0.00 / 0.00 |
| live20k | 0.66 / 2.64 / 0.32 / 0.00 / 0.00 \n 0.66 / 2.63 / 0.31 / 0.00 / 0.00 |
| storm20k | 0.79 / 4.81 / 1.27 / 0.00 / 0.00 \n 0.79 / 4.75 / 1.31 / 0.00 / 0.00 |
| thermal | 0.64 / 2.58 / 0.31 / 0.00 / 0.00 \n 0.64 / 2.57 / 0.31 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 31.54 / **8.13** / – | – / 2 / **28** | **0.0** / 0.7 / 98.2 |
| tick2k | 32.18 / **23.39** / – | – / – / – | 19.8 / 26.5 / **11.5** |
| jump2k | 101.32 / **38.12** / – | **57** / 38 / – | 114.9 / 50.0 / **0.0** |
| load20k | 25.62 / **15.60** / – | – / – / 32 | **1.8** / 3.4 / 110.0 |
| tick20k | 13.09 / **7.59** / – | – / – / – | 48.8 / **0.0** / 100.0 |
| jump20k | 54.24 / **17.68** / – | 25 / **32** / – | 90.9 / **44.4** / 97.1 |
| idle2k | **0.33** / 3.02 / – | – / – / – | 5.4 / 3.0 / **1.1** |
| idle20k | **0.25** / 7.95 / – | – / – / – | 2.6 / **0.8** / 99.6 |

### 3A040DLJG00042 · Android 15 · Tensor G3 · 11.3 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 141 / 142 / **129** |
| memory after the run, PSS (MB) | **80.6** / 135.2 / 480.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 70.2 / **135.0** / 124.1 | 25 / 8 / 8 | 1.6 / **0.0** / 0.0 | 15.18 / **10.29** / – | 95.6 / **61.4** / 94.8 |
| live2k | 64.0 / **127.9** / 75.6 | 24 / **8** / 33 | **0.0** / 0.0 / 5.3 | 17.47 / **10.48** / – | 112.2 / **69.0** / 127.3 |
| storm2k | 59.9 / **121.0** / 45.3 | 16 / **8** / 42 | **0.0** / 0.0 / 39.1 | 17.33 / **10.39** / – | 104.2 / **78.2** / 117.7 |
| scroll20k | 67.1 / **137.5** / – | 25 / **8** / – | 0.4 / **0.0** / – | 15.16 / **10.34** / – | 93.4 / **57.6** / 99.7 |
| live20k | 64.7 / **128.5** / – | 25 / **8** / – | 0.2 / **0.0** / – | 18.04 / **10.39** / – | 114.1 / **72.7** / 99.8 |
| storm20k | 59.8 / **118.5** / – | 16 / **8** / – | **0.0** / 0.0 / – | 16.92 / **10.51** / – | 103.6 / **76.1** / 99.1 |
| thermal | 60.6 / **120.4** / – | 25 / **8** / – | **0.0** / 0.0 / – | 17.46 / **10.50** / – | 113.1 / **70.3** / 99.6 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 3.42 / 3.40 | 4.66 / 4.57 | 15.18 / 14.91 | 1.6 / 0.9 |
| live2k | 7.35 / 7.15 | 4.32 / 4.60 | 17.47 / 18.20 | 0.0 / 0.4 |
| storm2k | 10.42 / 10.47 | 2.94 / 2.96 | 17.33 / 17.48 | 0.0 / 0.0 |
| scroll20k | 3.83 / 3.74 | 4.97 / 5.03 | 15.16 / 15.41 | 0.4 / 0.4 |
| live20k | 7.34 / 7.20 | 4.35 / 4.50 | 18.04 / 17.84 | 0.2 / 0.2 |
| storm20k | 10.21 / 10.35 | 3.02 / 2.98 | 16.92 / 17.06 | 0.0 / 0.0 |
| thermal | 7.32 / 7.26 | 4.46 / 4.50 | 17.46 / 17.41 | 0.0 / 0.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.26 / 0.78 / 0.35 / 0.00 / 0.00 \n 0.27 / 0.76 / 0.36 / 0.00 / 0.00 |
| live2k | 0.70 / 2.95 / 0.33 / 0.00 / 0.00 \n 0.68 / 2.84 / 0.31 / 0.00 / 0.00 |
| storm2k | 0.63 / 4.68 / 0.85 / 0.00 / 0.00 \n 0.62 / 4.68 / 0.85 / 0.00 / 0.00 |
| scroll20k | 0.26 / 0.90 / 0.38 / 0.00 / 0.00 \n 0.26 / 0.88 / 0.38 / 0.00 / 0.00 |
| live20k | 0.70 / 2.93 / 0.33 / 0.00 / 0.00 \n 0.68 / 2.84 / 0.32 / 0.00 / 0.00 |
| storm20k | 0.62 / 4.44 / 0.87 / 0.00 / 0.00 \n 0.63 / 4.47 / 0.86 / 0.00 / 0.00 |
| thermal | 0.68 / 2.92 / 0.32 / 0.00 / 0.00 \n 0.68 / 2.87 / 0.32 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 22.30 / **6.59** / – | – / 2 / **34** | **0.6** / 1.0 / 95.2 |
| tick2k | 31.34 / **18.34** / – | – / – / – | 39.7 / 6.5 / **0.0** |
| jump2k | 62.19 / **49.37** / – | 59 / **96** / – | 169.0 / 53.6 / **7.8** |
| load20k | 25.70 / **8.75** / – | – / – / 41 | **0.9** / 1.9 / 108.8 |
| tick20k | 15.96 / **4.43** / – | – / – / – | 39.0 / **0.0** / 104.3 |
| jump20k | 74.73 / **16.09** / – | 19 / **102** / – | **59.7** / 92.0 / 111.1 |
| idle2k | **0.68** / 5.19 / – | – / – / – | 3.9 / 3.8 / **3.3** |
| idle20k | **0.94** / 11.27 / – | – / – / – | 2.8 / **0.0** / 94.4 |

### 54051JEBF02305 · Android 15 · Tensor G4 · 7.4 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 122 / 118 / 125 |
| memory after the run, PSS (MB) | **89.7** / 144.1 / 476.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 68.6 / 67.5 / 63.3 | 16 / 16 / 16 | **0.0** / 0.0 / 0.2 | **13.81** / 18.72 / – | 76.6 / **25.2** / 48.7 |
| live2k | 63.7 / 64.0 / 62.9 | 16 / 16 / 16 | 0.8 / **0.0** / 0.9 | **16.66** / 19.07 / – | 99.0 / **35.2** / 112.7 |
| storm2k | 61.1 / 60.3 / 55.3 | 16 / 16 / 33 | **0.0** / 0.0 / 14.1 | **16.82** / 19.36 / – | 98.7 / **43.5** / 114.7 |
| scroll20k | 68.0 / 68.1 / – | 16 / 16 / – | 0.2 / **0.0** / – | **13.95** / 18.74 / – | 79.5 / **24.0** / 99.6 |
| live20k | 63.6 / 64.1 / – | 16 / 16 / – | 0.4 / **0.0** / – | **16.64** / 18.81 / – | 98.8 / **34.1** / 99.1 |
| storm20k | 60.0 / 60.8 / – | 16 / 16 / – | **0.0** / 0.0 / – | **16.36** / 18.96 / – | 98.5 / **44.8** / 99.6 |
| thermal | 60.3 / 60.2 / – | 16 / 16 / – | 0.3 / **0.0** / – | **16.69** / 18.92 / – | 99.0 / **33.8** / 99.6 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 4.24 / 4.29 | 4.15 / 4.44 | 13.81 / 13.52 | 0.0 / 0.0 |
| live2k | 8.51 / 8.59 | 3.69 / 3.65 | 16.66 / 16.65 | 0.8 / 0.9 |
| storm2k | 10.46 / 10.27 | 2.43 / 2.47 | 16.82 / 16.72 | 0.0 / 0.2 |
| scroll20k | 4.54 / 4.54 | 4.39 / 4.49 | 13.95 / 14.56 | 0.2 / 0.2 |
| live20k | 8.49 / 8.57 | 3.70 / 3.71 | 16.64 / 16.63 | 0.4 / 0.4 |
| storm20k | 10.52 / 10.24 | 2.37 / 2.35 | 16.36 / 16.42 | 0.0 / 0.0 |
| thermal | 8.48 / 8.53 | 3.79 / 3.74 | 16.69 / 16.76 | 0.3 / 0.4 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.22 / 0.99 / 0.54 / 0.00 / 0.00 \n 0.22 / 1.02 / 0.55 / 0.00 / 0.00 |
| live2k | 0.63 / 3.34 / 0.51 / 0.00 / 0.00 \n 0.64 / 3.32 / 0.52 / 0.00 / 0.00 |
| storm2k | 0.48 / 4.07 / 0.86 / 0.00 / 0.00 \n 0.49 / 4.01 / 0.86 / 0.00 / 0.00 |
| scroll20k | 0.22 / 1.06 / 0.60 / 0.00 / 0.00 \n 0.22 / 1.06 / 0.61 / 0.00 / 0.00 |
| live20k | 0.63 / 3.32 / 0.51 / 0.00 / 0.00 \n 0.64 / 3.38 / 0.51 / 0.00 / 0.00 |
| storm20k | 0.49 / 4.10 / 0.87 / 0.00 / 0.00 \n 0.48 / 3.99 / 0.83 / 0.00 / 0.00 |
| thermal | 0.62 / 3.30 / 0.51 / 0.00 / 0.00 \n 0.63 / 3.34 / 0.51 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 21.97 / **6.25** / – | – / 2 / **20** | **0.0** / 0.7 / 71.1 |
| tick2k | 39.14 / **16.83** / – | – / – / – | **0.0** / 5.5 / 10.2 |
| jump2k | 83.74 / **29.13** / – | **30** / 18 / – | 36.5 / 9.6 / **0.0** |
| load20k | 23.83 / **10.29** / – | – / – / 53 | **0.0** / 0.9 / 108.1 |
| tick20k | 16.28 / **8.49** / – | – / – / – | 38.5 / **3.0** / 96.6 |
| jump20k | 57.46 / **16.87** / – | 21 / **44** / – | 90.0 / **29.7** / 93.2 |
| idle2k | **0.41** / 3.04 / – | – / – / – | 5.0 / **2.2** / 4.4 |
| idle20k | **0.65** / 5.00 / – | – / – / – | 4.9 / **0.0** / 98.2 |

### LMQ72016369a75 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS, vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`
- vieww-cpuserial: `blur off (tier low), fill off (tier low), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 350 / 353 / 468 |
| memory after the run, PSS (MB) | **74.9** / 175.6 / 247.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **31.13** / 165.59 / – | 147.0 / **43.2** / 102.4 |
| live2k | – / – / – | – / – / – | – / – / – | **52.87** / 62.58 / – | 132.4 / **57.9** / 97.4 |
| storm2k | – / – / – | – / – / – | – / – / – | 75.99 / **61.69** / – | 108.4 / **77.2** / 119.5 |
| scroll20k | – / – / – | – / – / – | – / – / – | **31.50** / 60.65 / – | 148.9 / **27.0** / 106.1 |
| live20k | – / – / – | – / – / – | – / – / – | **53.44** / 61.48 / – | 132.0 / **54.3** / 98.3 |
| storm20k | – / – / – | – / – / – | – / – / – | 74.55 / **61.95** / – | 109.2 / **76.9** / 99.1 |
| thermal | – / – / – | – / – / – | – / – / – | **52.83** / 60.85 / – | 132.4 / **57.5** / 98.5 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 17.54 / 17.63 | 6.55 / 6.52 | 31.13 / 31.44 | – / – |
| live2k | 39.16 / 39.11 | 6.61 / 6.61 | 52.87 / 52.89 | – / – |
| storm2k | 64.49 / 65.35 | 4.48 / 4.49 | 75.99 / 75.43 | – / – |
| scroll20k | 17.44 / 17.45 | 6.56 / 6.54 | 31.50 / 31.12 | – / – |
| live20k | 39.28 / 39.19 | 6.65 / 6.57 | 53.44 / 53.20 | – / – |
| storm20k | 64.72 / 66.61 | 4.38 / 4.61 | 74.55 / 77.78 | – / – |
| thermal | 38.62 / 38.79 | 6.59 / 6.55 | 52.83 / 52.73 | – / – |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.69 / 4.18 / 2.68 / 0.00 / 0.00 \n 0.69 / 4.18 / 2.67 / 0.00 / 0.00 |
| live2k | 2.65 / 15.59 / 2.78 / 0.00 / 0.00 \n 2.65 / 15.65 / 2.74 / 0.00 / 0.00 |
| storm2k | 2.90 / 29.02 / 7.95 / 0.00 / 0.00 \n 2.93 / 29.67 / 7.91 / 0.00 / 0.00 |
| scroll20k | 0.61 / 4.17 / 2.74 / 0.00 / 0.00 \n 0.61 / 4.17 / 2.66 / 0.00 / 0.00 |
| live20k | 2.64 / 15.95 / 2.86 / 0.00 / 0.00 \n 2.65 / 15.93 / 2.79 / 0.00 / 0.00 |
| storm20k | 3.03 / 29.16 / 7.98 / 0.00 / 0.00 \n 3.01 / 30.56 / 7.94 / 0.00 / 0.00 |
| thermal | 2.58 / 15.90 / 2.74 / 0.00 / 0.00 \n 2.59 / 15.88 / 2.76 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 200.20 / **45.57** / – | – / – / – | **0.0** / 0.7 / 99.2 |
| tick2k | 46.90 / **16.43** / – | – / – / – | **0.0** / 11.6 / 98.2 |
| jump2k | 168.99 / **65.47** / – | – / – / – | 130.1 / **21.7** / 94.0 |
| load20k | 147.04 / **38.84** / – | – / – / – | 0.8 / **0.0** / 125.9 |
| tick20k | 30.70 / 31.58 / – | – / – / – | **0.0** / 30.0 / 114.7 |
| jump20k | 159.90 / **32.48** / – | – / – / – | 216.2 / **29.9** / 112.1 |
| idle2k | **1.06** / 17.12 / – | – / – / – | **0.0** / 0.0 / 99.0 |
| idle20k | **1.21** / 15.44 / – | – / – / – | **0.0** / 4.3 / 120.1 |

### RFCX31FN5CF · Android 14 · s5e8835 · 5.3 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 236 / 228 / 328 |
| memory after the run, PSS (MB) | **115.1** / 155.9 / 447.2 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 72.7 / 70.9 / **117.6** | 16 / 16 / **8** | 91.0 / 96.1 / **0.3** | **13.89** / 17.60 / – | 77.0 / **60.3** / 121.2 |
| live2k | 63.3 / 62.8 / **121.9** | 16 / 17 / **8** | 100.0 / 100.0 / **3.9** | **16.10** / 24.06 / – | 98.0 / **65.4** / 155.8 |
| storm2k | 46.7 / 61.1 / **85.7** | 33 / **16** / 25 | 100.0 / 100.0 / **38.3** | 22.43 / **18.73** / – | 102.7 / **69.3** / 149.0 |
| scroll20k | 69.0 / 70.3 / – | 16 / 16 / – | 100.0 / 100.0 / – | **13.77** / 14.97 / – | 78.5 / **57.1** / 99.2 |
| live20k | 64.3 / 63.9 / – | 16 / 16 / – | 100.0 / 100.0 / – | **16.25** / 20.98 / – | 100.0 / **67.7** / 99.0 |
| storm20k | 48.0 / **63.1** / – | 33 / **16** / – | 100.0 / 100.0 / – | 22.42 / 21.60 / – | 106.9 / **72.5** / 99.0 |
| thermal | 60.3 / 60.3 / – | 16 / 16 / – | 100.0 / 100.0 / – | **15.95** / 21.23 / – | 97.7 / **67.7** / 99.0 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 5.25 / 5.34 | 3.93 / 3.91 | 13.89 / 13.62 | 91.0 / 97.5 |
| live2k | 9.97 / 9.91 | 3.32 / 3.46 | 16.10 / 16.37 | 100.0 / 100.0 |
| storm2k | 17.48 / 10.71 | 2.53 / 2.51 | 22.43 / 15.53 | 100.0 / 100.0 |
| scroll20k | 5.44 / 5.52 | 3.95 / 4.08 | 13.77 / 13.48 | 100.0 / 100.0 |
| live20k | 10.09 / 10.02 | 3.35 / 3.41 | 16.25 / 16.23 | 100.0 / 100.0 |
| storm20k | 17.33 / 17.23 | 2.54 / 2.54 | 22.42 / 22.64 | 100.0 / 100.0 |
| thermal | 9.98 / 9.98 | 3.34 / 3.40 | 15.95 / 16.32 | 100.0 / 100.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.26 / 1.17 / 0.64 / 0.00 / 0.00 \n 0.26 / 1.19 / 0.69 / 0.00 / 0.00 |
| live2k | 0.79 / 3.75 / 0.56 / 0.00 / 0.00 \n 0.79 / 3.71 / 0.55 / 0.00 / 0.00 |
| storm2k | 1.04 / 6.89 / 1.77 / 0.00 / 0.00 \n 1.00 / 5.51 / 0.06 / 0.00 / 0.00 |
| scroll20k | 0.22 / 1.22 / 0.72 / 0.00 / 0.00 \n 0.23 / 1.25 / 0.72 / 0.00 / 0.00 |
| live20k | 0.82 / 3.81 / 0.56 / 0.00 / 0.00 \n 0.82 / 3.78 / 0.54 / 0.00 / 0.00 |
| storm20k | 1.04 / 6.91 / 1.75 / 0.00 / 0.00 \n 1.05 / 6.82 / 1.75 / 0.00 / 0.00 |
| thermal | 0.79 / 3.74 / 0.56 / 0.00 / 0.00 \n 0.78 / 3.71 / 0.54 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 32.05 / **11.11** / – | – / 2 / – | **0.3** / 1.3 / 97.4 |
| tick2k | 22.26 / **17.35** / – | – / – / – | 7.3 / **0.0** / 0.0 |
| jump2k | 47.98 / **21.90** / – | 53 / **84** / – | 117.6 / 80.3 / **0.0** |
| load20k | 36.57 / **10.74** / – | – / – / 109 | **0.0** / 1.6 / 143.1 |
| tick20k | 14.57 / **13.37** / – | – / – / – | 22.1 / **7.1** / 94.0 |
| jump20k | 39.53 / **15.18** / – | 29 / **39** / – | 49.2 / **27.0** / 102.2 |
| idle2k | **0.19** / 6.81 / – | – / – / – | 5.4 / 3.3 / **0.0** |
| idle20k | **0.19** / 9.51 / – | – / – / – | 3.0 / **2.8** / 97.7 |

### RFCY510TASW · Android 15 · SM8750 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 116 / 115 / 261 |
| memory after the run, PSS (MB) | **87.5** / 140.2 / 480.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 127.9 / 132.3 / 126.5 | 8 / 8 / 8 | 0.3 / **0.0** / 0.0 | **7.81** / 9.14 / – | 76.6 / **53.3** / 57.6 |
| live2k | 68.6 / 68.9 / **125.6** | 16 / 16 / **8** | 82.0 / 86.5 / **0.7** | 15.08 / 15.55 / – | 81.0 / **54.7** / 96.2 |
| storm2k | 59.1 / 58.8 / **124.1** | 16 / 16 / **8** | 100.0 / 100.0 / **1.2** | 16.35 / **14.70** / – | 77.2 / **61.8** / 108.9 |
| scroll20k | 64.7 / 67.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | 15.34 / **7.11** / – | 70.3 / **30.2** / 99.6 |
| live20k | 62.2 / 63.6 / – | 16 / 16 / – | 100.0 / 100.0 / – | **13.57** / 18.00 / – | 73.9 / **59.0** / 99.2 |
| storm20k | 59.0 / **62.8** / – | 16 / 16 / – | 100.0 / 100.0 / – | **11.89** / 16.17 / – | 71.6 / **66.3** / 99.1 |
| thermal | 60.0 / 60.6 / – | 16 / 16 / – | 100.0 / 100.0 / – | **13.28** / 17.59 / – | 73.1 / **58.3** / 99.2 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 2.37 / 2.35 | 1.65 / 1.67 | 7.81 / 7.91 | 0.3 / 0.4 |
| live2k | 7.74 / 3.94 | 1.92 / 2.49 | 15.08 / 8.48 | 82.0 / 0.4 |
| storm2k | 8.74 / 4.30 | 1.58 / 2.22 | 16.35 / 8.06 | 100.0 / 0.2 |
| scroll20k | 4.58 / 2.38 | 2.94 / 1.68 | 15.34 / 7.56 | 100.0 / 0.4 |
| live20k | 7.40 / 3.92 | 1.85 / 2.42 | 13.57 / 8.33 | 100.0 / 0.3 |
| storm20k | 8.39 / 4.35 | 1.55 / 2.20 | 11.89 / 7.93 | 100.0 / 0.1 |
| thermal | 7.44 / 3.91 | 1.88 / 2.48 | 13.28 / 8.38 | 100.0 / 0.4 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.09 / 0.66 / 0.34 / 0.00 / 0.00 \n 0.09 / 0.66 / 0.34 / 0.00 / 0.00 |
| live2k | 0.57 / 3.35 / 0.41 / 0.00 / 0.00 \n 0.32 / 1.68 / 0.14 / 0.00 / 0.00 |
| storm2k | 0.61 / 4.76 / 0.02 / 0.00 / 0.00 \n 0.38 / 2.45 / 0.01 / 0.00 / 0.00 |
| scroll20k | 0.15 / 1.18 / 0.69 / 0.00 / 0.00 \n 0.08 / 0.67 / 0.34 / 0.00 / 0.00 |
| live20k | 0.56 / 3.19 / 0.40 / 0.00 / 0.00 \n 0.32 / 1.68 / 0.14 / 0.00 / 0.00 |
| storm20k | 0.65 / 4.52 / 0.02 / 0.00 / 0.00 \n 0.38 / 2.49 / 0.01 / 0.00 / 0.00 |
| thermal | 0.55 / 3.21 / 0.40 / 0.00 / 0.00 \n 0.32 / 1.66 / 0.14 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 10.46 / **3.33** / – | – / 2 / – | 0.3 / 0.3 / 43.7 |
| tick2k | 13.62 / **12.22** / – | – / – / – | 18.0 / 8.6 / **0.0** |
| jump2k | 37.12 / **10.92** / – | **42** / 3 / – | 40.1 / 22.0 / **0.0** |
| load20k | 28.92 / **12.39** / – | – / – / 51 | **0.0** / 0.0 / 104.0 |
| tick20k | 29.65 / **10.53** / – | – / – / – | 6.0 / **0.0** / 111.1 |
| jump20k | 37.64 / **22.91** / – | **40** / 33 / – | 59.5 / **25.2** / 123.5 |
| idle2k | **0.26** / 1.73 / – | – / – / – | 2.2 / 1.4 / **0.0** |
| idle20k | **0.44** / 3.44 / – | – / – / – | 2.5 / **0.9** / 99.3 |

### R3GL105R47R · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS, vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 104 / 101 / 131 |
| memory after the run, PSS (MB) | **97.8** / 215.8 / 1047.1 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 71.2 / 70.1 / **119.9** | 16 / 16 / 16 | 87.4 / 91.4 / **9.3** | 12.87 / **8.72** / – | 61.2 / **28.9** / 33.1 |
| live2k | 63.6 / 64.2 / **123.8** | 16 / 16 / **8** | 100.0 / 100.0 / **1.2** | 14.61 / **12.98** / – | 74.3 / **54.2** / 96.0 |
| storm2k | 60.1 / 61.4 / **107.0** | 16 / 16 / 16 | 100.0 / 100.0 / **15.7** | 17.52 / **12.62** / – | 82.1 / **59.1** / 107.1 |
| scroll20k | 66.6 / 67.1 / – | 16 / 16 / – | 100.0 / 100.0 / – | 13.05 / **6.08** / – | 64.1 / **25.9** / 99.5 |
| live20k | 64.1 / 64.0 / – | 16 / 16 / – | 100.0 / 100.0 / – | 13.85 / 13.16 / – | 72.5 / **54.4** / 98.9 |
| storm20k | 59.4 / 60.6 / – | 16 / 16 / – | 100.0 / 100.0 / – | 13.74 / 13.16 / – | 79.1 / **58.5** / 99.4 |
| thermal | 60.4 / 60.6 / 3.6 | 16 / 16 / 16 | 100.0 / 100.0 / **13.7** | 14.17 / **13.38** / – | 73.2 / **53.9** / 91.9 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 5.28 / 4.07 | 1.88 / 1.35 | 12.87 / 11.97 | 87.4 / 54.4 |
| live2k | 9.00 / 9.13 | 1.36 / 1.35 | 14.61 / 14.83 | 100.0 / 100.0 |
| storm2k | 10.19 / 9.48 | 1.42 / 1.01 | 17.52 / 12.17 | 100.0 / 100.0 |
| scroll20k | 5.71 / 5.82 | 2.14 / 2.19 | 13.05 / 13.47 | 100.0 / 100.0 |
| live20k | 8.70 / 9.02 | 1.35 / 1.35 | 13.85 / 14.57 | 100.0 / 100.0 |
| storm20k | 11.13 / 11.18 | 0.94 / 0.96 | 13.74 / 15.57 | 100.0 / 100.0 |
| thermal | 8.85 / 8.86 | 1.36 / 1.38 | 14.17 / 14.35 | 100.0 / 100.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.28 / 1.45 / 0.89 / 0.00 / 0.00 \n 0.23 / 1.14 / 0.65 / 0.00 / 0.00 |
| live2k | 0.67 / 4.02 / 0.54 / 0.00 / 0.00 \n 0.68 / 4.09 / 0.56 / 0.00 / 0.00 |
| storm2k | 0.60 / 4.64 / 0.67 / 0.00 / 0.00 \n 0.71 / 5.32 / 0.02 / 0.00 / 0.00 |
| scroll20k | 0.25 / 1.68 / 0.97 / 0.00 / 0.00 \n 0.25 / 1.70 / 1.01 / 0.00 / 0.00 |
| live20k | 0.66 / 3.90 / 0.51 / 0.00 / 0.00 \n 0.67 / 4.06 / 0.52 / 0.00 / 0.00 |
| storm20k | 0.58 / 4.82 / 1.02 / 0.00 / 0.00 \n 0.59 / 4.79 / 1.01 / 0.00 / 0.00 |
| thermal | 0.66 / 3.99 / 0.53 / 0.00 / 0.00 \n 0.65 / 4.00 / 0.52 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 11.96 / **3.11** / – | – / **2** / 1 | **0.0** / 0.0 / 31.8 |
| tick2k | 16.88 / **13.81** / – | – / – / – | 24.6 / 3.5 / **2.2** |
| jump2k | 29.47 / **18.69** / – | 41 / **58** / 3 | 69.0 / 23.8 / **0.0** |
| load20k | 15.13 / **12.05** / – | – / – / 19 | **0.0** / 0.6 / 98.7 |
| tick20k | 20.02 / **10.23** / – | – / – / – | **0.0** / 3.1 / 99.4 |
| jump20k | 28.64 / **13.60** / – | 29 / **42** / – | 59.3 / **43.5** / 106.8 |
| idle2k | **0.23** / 2.20 / – | – / – / – | 2.0 / 2.1 / **0.0** |
| idle20k | **0.22** / 3.44 / – | – / – / – | 1.7 / **1.5** / 99.4 |

### Across phones · median presented fps

| phase | 0B301FDD400775 | 31060DLH2000TM | 3A040DLJG00042 | 54051JEBF02305 | LMQ72016369a75 | RFCX31FN5CF | RFCY510TASW | R3GL105R47R |
|---|---|---|---|---|---|---|---|---|
| scroll2k | 67.6 / 69.4 / **81.8** | 68.5 / 81.3 / **94.0** | 70.2 / **135.0** / 124.1 | 68.6 / 67.5 / 63.3 | – / – / – | 72.7 / 70.9 / **117.6** | 127.9 / 132.3 / 126.5 | 71.2 / 70.1 / **119.9** |
| live2k | 57.5 / 62.7 / **87.2** | 63.2 / 63.6 / **87.0** | 64.0 / **127.9** / 75.6 | 63.7 / 64.0 / 62.9 | – / – / – | 63.3 / 62.8 / **121.9** | 68.6 / 68.9 / **125.6** | 63.6 / 64.2 / **123.8** |
| storm2k | 34.5 / 60.3 / 62.9 | 58.4 / **61.9** / 58.6 | 59.9 / **121.0** / 45.3 | 61.1 / 60.3 / 55.3 | – / – / – | 46.7 / 61.1 / **85.7** | 59.1 / 58.8 / **124.1** | 60.1 / 61.4 / **107.0** |
| scroll20k | 63.9 / **70.3** / – | 67.6 / 66.7 / – | 67.1 / **137.5** / – | 68.0 / 68.1 / – | – / – / – | 69.0 / 70.3 / – | 64.7 / 67.5 / – | 66.6 / 67.1 / – |
| live20k | 50.0 / **62.9** / – | 63.5 / 64.1 / – | 64.7 / **128.5** / – | 63.6 / 64.1 / – | – / – / – | 64.3 / 63.9 / – | 62.2 / 63.6 / – | 64.1 / 64.0 / – |
| storm20k | 37.5 / **61.6** / – | 60.7 / 61.3 / – | 59.8 / **118.5** / – | 60.0 / 60.8 / – | – / – / – | 48.0 / **63.1** / – | 59.0 / **62.8** / – | 59.4 / 60.6 / – |
| thermal | 57.3 / **60.6** / – | 60.2 / 60.7 / – | 60.6 / **120.4** / – | 60.3 / 60.2 / – | – / – / – | 60.3 / 60.3 / – | 60.0 / 60.6 / – | 60.4 / 60.6 / 3.6 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
