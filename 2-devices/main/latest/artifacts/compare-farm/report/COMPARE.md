# Valdi vs Vieww — Market Watch



Same screen in both frameworks (identical data generator, tick rule and schedule; `cargo test -p market-watch` checks the data matches Valdi's). Cells read **Vieww CPU / Vieww GPU / Valdi**; the clearly best of three (by 5% or more) is bold.

Valdi built as it ships: `--snap_flavor=production -c opt --copt=-DANDROID_WITH_JNI --repo_env=VALDI_PLATFORM_DEPENDENCIES=android --define=client_repo_arm64=true --android_platforms=@snap_platforms//os:android_arm64` at `e90a983749b3` — arm64-v8a, JS engine quickjs, debuggable false (the flags Valdi's CLI uses for `valdi package android --build_config release`).

## Phones (AWS Device Farm)

### 0C171FDD4000JL · Android 11 · lito · 7.3 GB · 90.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 151 / **139** / 170 |
| memory after the run, PSS (MB) | **86.1** / 103.0 / 413.4 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 67.3 / 0.6 / **79.2** | 17 / 11 / 11 | 5.8 / **0.0** / 0.2 | 15.37 / **8.71** / – | 82.0 / **49.7** / 63.2 |
| live2k | 56.8 / 0.5 / **87.1** | 33 / **11** / 22 | 11.0 / **0.0** / 7.1 | 20.17 / **9.00** / – | 110.6 / **65.2** / 111.5 |
| storm2k | 36.6 / 2.1 / **62.9** | 33 / **11** / 33 | 68.0 / **0.0** / 40.7 | 29.80 / **9.47** / – | 105.5 / **82.1** / 119.1 |
| scroll20k | **66.6** / 0.1 / – | 16 / **10** / – | 2.6 / **0.0** / – | 14.16 / **8.44** / – | 81.6 / **47.9** / 99.9 |
| live20k | **56.4** / 0.5 / – | 33 / **11** / – | 12.6 / **0.0** / – | 19.97 / **8.84** / – | 113.0 / **64.8** / 99.4 |
| storm20k | **35.1** / 1.1 / – | 33 / **11** / – | 71.7 / **0.0** / – | 30.00 / **8.84** / – | 101.5 / **79.8** / 99.4 |
| thermal | **55.9** / 0.6 / – | 33 / **11** / – | 9.3 / **0.0** / – | 19.83 / **8.81** / – | 111.0 / **64.5** / 99.2 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 5.56 / 5.51 | 4.48 / 4.16 | 15.37 / 17.14 | 5.8 / 3.4 |
| live2k | 11.75 / 11.51 | 4.44 / 4.26 | 20.17 / 19.60 | 11.0 / 10.0 |
| storm2k | 23.36 / 22.37 | 2.91 / 2.77 | 29.80 / 28.91 | 68.0 / 63.9 |
| scroll20k | 5.53 / 5.46 | 4.53 / 4.25 | 14.16 / 14.45 | 2.6 / 2.6 |
| live20k | 11.85 / 11.61 | 4.34 / 4.24 | 19.97 / 19.77 | 12.6 / 9.8 |
| storm20k | 23.45 / 23.32 | 2.93 / 2.94 | 30.00 / 30.50 | 71.7 / 71.7 |
| thermal | 11.70 / 11.64 | 4.34 / 4.32 | 19.83 / 19.82 | 9.3 / 8.6 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.23 / 1.25 / 0.51 / 0.00 / 0.00 \n 0.22 / 1.22 / 0.49 / 0.00 / 0.00 |
| live2k | 0.74 / 4.42 / 0.56 / 0.00 / 0.00 \n 0.72 / 4.34 / 0.55 / 0.00 / 0.00 |
| storm2k | 0.87 / 9.17 / 2.08 / 0.00 / 0.00 \n 0.85 / 8.65 / 1.99 / 0.00 / 0.00 |
| scroll20k | 0.22 / 1.21 / 0.52 / 0.00 / 0.00 \n 0.22 / 1.22 / 0.51 / 0.00 / 0.00 |
| live20k | 0.75 / 4.47 / 0.56 / 0.00 / 0.00 \n 0.74 / 4.38 / 0.54 / 0.00 / 0.00 |
| storm20k | 0.89 / 9.24 / 2.06 / 0.00 / 0.00 \n 0.89 / 9.28 / 2.03 / 0.00 / 0.00 |
| thermal | 0.73 / 4.41 / 0.55 / 0.00 / 0.00 \n 0.73 / 4.37 / 0.56 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 43.20 / **5.25** / – | – / 1 / **24** | **1.0** / 1.4 / 100.7 |
| tick2k | 16.55 / **11.83** / – | – / – / – | 19.4 / 3.8 / **0.0** |
| jump2k | 86.39 / **16.93** / – | 35 / – / – | 113.4 / 32.6 / **0.0** |
| load20k | 50.41 / **10.63** / – | – / – / 97 | **0.0** / 0.0 / 117.5 |
| tick20k | 17.22 / **6.07** / – | – / – / – | 11.1 / **3.3** / 101.1 |
| jump20k | 60.89 / **14.06** / – | 27 / – / – | 108.7 / **44.4** / 103.4 |
| idle2k | **0.28** / 2.26 / – | – / – / – | 2.2 / 2.3 / 58.0 |
| idle20k | **0.32** / 2.46 / – | – / – / – | 0.9 / **0.0** / 95.8 |

### 33111FDH300270 · Android 13 · GS201 · 11.2 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 145 / 146 / **136** |
| memory after the run, PSS (MB) | **93.6** / 162.9 / 470.6 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 69.9 / **134.9** / 123.1 | 16 / 8 / 8 | 0.2 / **0.0** / 0.0 | 14.08 / **3.93** / – | **79.8** / 90.3 / 104.9 |
| live2k | 64.2 / **129.4** / 103.6 | 16 / **8** / 16 | **0.0** / 0.0 / 0.3 | 14.54 / **4.67** / – | **84.5** / 98.0 / 133.7 |
| storm2k | 63.1 / **122.5** / 66.5 | 16 / **8** / 33 | **0.0** / 0.0 / 9.1 | 17.49 / **6.01** / – | **102.2** / 108.7 / 123.3 |
| scroll20k | 67.5 / **132.6** / – | 16 / **8** / – | **0.0** / 0.0 / – | 13.99 / **3.73** / – | **80.2** / 87.0 / 99.6 |
| live20k | 64.0 / **129.3** / – | 16 / **8** / – | **0.0** / 0.0 / – | 14.60 / **4.20** / – | **83.2** / 98.0 / 99.2 |
| storm20k | 60.6 / **118.8** / – | 16 / **8** / – | **0.0** / 0.0 / – | 16.88 / **5.31** / – | 95.0 / 106.9 / 99.3 |
| thermal | 60.6 / **121.1** / – | 16 / **8** / – | **0.0** / 0.0 / – | 13.79 / **3.98** / – | **80.6** / 98.2 / 99.2 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 4.65 / 4.58 | 3.59 / 3.42 | 14.08 / 13.40 | 0.2 / 0.2 |
| live2k | 6.90 / 6.92 | 3.13 / 3.10 | 14.54 / 14.13 | 0.0 / 0.0 |
| storm2k | 12.01 / 12.26 | 1.80 / 1.83 | 17.49 / 17.61 | 0.0 / 0.0 |
| scroll20k | 5.05 / 5.26 | 3.86 / 3.87 | 13.99 / 14.48 | 0.0 / 0.0 |
| live20k | 6.97 / 6.98 | 3.08 / 3.12 | 14.60 / 14.02 | 0.0 / 0.0 |
| storm20k | 11.63 / 11.29 | 1.81 / 1.72 | 16.88 / 16.56 | 0.0 / 0.0 |
| thermal | 6.74 / 6.77 | 3.07 / 3.06 | 13.79 / 14.06 | 0.0 / 0.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.20 / 0.94 / 0.45 / 0.00 / 0.00 \n 0.20 / 0.93 / 0.45 / 0.00 / 0.00 |
| live2k | 0.65 / 2.70 / 0.32 / 0.00 / 0.00 \n 0.64 / 2.68 / 0.33 / 0.00 / 0.00 |
| storm2k | 0.74 / 4.76 / 1.27 / 0.00 / 0.00 \n 0.76 / 4.97 / 1.22 / 0.00 / 0.00 |
| scroll20k | 0.21 / 1.00 / 0.49 / 0.00 / 0.00 \n 0.21 / 1.04 / 0.51 / 0.00 / 0.00 |
| live20k | 0.66 / 2.74 / 0.32 / 0.00 / 0.00 \n 0.66 / 2.77 / 0.32 / 0.00 / 0.00 |
| storm20k | 0.76 / 4.70 / 1.15 / 0.00 / 0.00 \n 0.76 / 4.64 / 1.13 / 0.00 / 0.00 |
| thermal | 0.62 / 2.60 / 0.32 / 0.00 / 0.00 \n 0.62 / 2.59 / 0.32 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 27.23 / **8.56** / – | – / 2 / **35** | **0.3** / 1.0 / 103.4 |
| tick2k | 42.44 / **14.83** / – | – / – / – | **0.0** / 7.1 / 1.8 |
| jump2k | 52.61 / **39.72** / – | 71 / **103** / – | 114.3 / 102.6 / **16.1** |
| load20k | 25.68 / **6.29** / – | – / – / 36 | **0.0** / 0.0 / 110.2 |
| tick20k | 13.19 / **4.27** / – | – / – / – | 33.3 / **0.0** / 97.5 |
| jump20k | 52.82 / **8.88** / – | 32 / **82** / – | 90.9 / 107.1 / **75.5** |
| idle2k | **0.30** / 2.80 / – | – / – / – | 5.8 / 4.9 / **0.0** |
| idle20k | **0.28** / 2.22 / – | – / – / – | 3.4 / **0.8** / 95.3 |

### 3C011JEKB21900 · Android 17 · Tensor G3 · 7.4 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 161 / 156 / 151 |
| memory after the run, PSS (MB) | **89.1** / 199.6 / 463.6 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 72.8 / 73.7 / 72.8 | 16 / 16 / 16 | 0.7 / **0.0** / 0.3 | 17.72 / **5.24** / – | 98.5 / 92.5 / **53.2** |
| live2k | 64.3 / 66.1 / 63.1 | 16 / 16 / 16 | 1.8 / **0.0** / 1.1 | 19.53 / **3.81** / – | 120.5 / **102.3** / 114.2 |
| storm2k | 57.2 / **61.0** / 50.4 | 33 / **16** / 33 | 5.7 / **0.0** / 24.9 | 21.07 / **4.71** / – | 117.3 / **106.8** / 116.7 |
| scroll20k | 72.6 / 72.4 / – | 16 / 16 / – | 0.5 / **0.0** / – | 16.51 / **4.87** / – | 104.9 / **92.5** / 99.6 |
| live20k | 65.0 / 65.7 / – | 16 / 16 / – | 2.1 / **0.0** / – | 19.03 / **3.85** / – | 121.3 / 101.3 / 99.5 |
| storm20k | 57.3 / **60.5** / – | 33 / **16** / – | 6.2 / **0.0** / – | 21.09 / **4.63** / – | 122.5 / 107.0 / **99.9** |
| thermal | 59.4 / 60.7 / – | 16 / 16 / – | 1.9 / **0.0** / – | 19.07 / **3.75** / – | 118.0 / 98.7 / 99.6 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 3.81 / 3.84 | 6.39 / 6.21 | 17.72 / 16.83 | 0.7 / 0.5 |
| live2k | 6.47 / 6.64 | 5.79 / 5.84 | 19.53 / 18.32 | 1.8 / 1.7 |
| storm2k | 10.27 / 10.74 | 4.60 / 4.82 | 21.07 / 20.99 | 5.7 / 6.9 |
| scroll20k | 3.67 / 3.64 | 6.21 / 6.28 | 16.51 / 17.49 | 0.5 / 0.7 |
| live20k | 6.50 / 6.59 | 5.96 / 5.94 | 19.03 / 19.31 | 2.1 / 2.1 |
| storm20k | 10.69 / 10.58 | 4.67 / 4.71 | 21.09 / 21.45 | 6.2 / 6.8 |
| thermal | 6.44 / 6.44 | 5.78 / 5.97 | 19.07 / 18.97 | 1.9 / 1.9 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.19 / 0.83 / 0.41 / 0.00 / 0.00 \n 0.19 / 0.81 / 0.43 / 0.00 / 0.00 |
| live2k | 0.52 / 2.37 / 0.34 / 0.00 / 0.00 \n 0.51 / 2.43 / 0.35 / 0.00 / 0.00 |
| storm2k | 0.55 / 4.02 / 0.95 / 0.00 / 0.00 \n 0.56 / 4.36 / 0.96 / 0.00 / 0.00 |
| scroll20k | 0.17 / 0.77 / 0.40 / 0.00 / 0.00 \n 0.17 / 0.80 / 0.40 / 0.00 / 0.00 |
| live20k | 0.52 / 2.44 / 0.33 / 0.00 / 0.00 \n 0.53 / 2.41 / 0.34 / 0.00 / 0.00 |
| storm20k | 0.56 / 4.26 / 0.95 / 0.00 / 0.00 \n 0.56 / 4.20 / 0.96 / 0.00 / 0.00 |
| thermal | 0.51 / 2.39 / 0.33 / 0.00 / 0.00 \n 0.51 / 2.39 / 0.33 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 29.60 / **10.95** / – | – / 2 / **20** | **0.3** / 0.6 / 96.5 |
| tick2k | 47.00 / **17.30** / – | – / 1 / – | **0.0** / 0.0 / 25.0 |
| jump2k | 75.03 / **25.16** / – | 75 / **81** / 16 | 87.8 / 89.4 / **6.8** |
| load20k | 23.00 / **6.65** / – | – / – / 24 | **0.0** / 1.0 / 106.9 |
| tick20k | 20.63 / **5.30** / – | 3 / – / – | **0.0** / 0.0 / 88.1 |
| jump20k | 60.90 / **14.11** / – | **83** / 72 / – | 80.3 / **67.8** / 138.6 |
| idle2k | **0.49** / 4.63 / – | – / – / – | 11.3 / **0.0** / 0.0 |
| idle20k | **0.73** / 4.85 / – | – / – / – | **0.0** / 0.0 / 101.9 |

### 4A191JEBF28019 · Android 15 · Tensor G4 · 7.4 GB · 60.0 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 126 / 130 / 127 |
| memory after the run, PSS (MB) | **89.5** / 200.9 / 478.0 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 67.4 / 67.0 / 62.3 | 16 / 16 / 16 | **0.0** / 0.0 / 0.2 | 13.85 / **3.87** / – | 77.4 / 94.2 / **47.2** |
| live2k | 63.6 / 64.1 / 62.2 | 16 / 16 / 16 | 0.7 / **0.0** / 1.3 | 16.58 / **4.59** / – | 98.7 / 99.6 / 112.0 |
| storm2k | 60.3 / 59.1 / 56.6 | 16 / 16 / 33 | **0.0** / 0.0 / 11.6 | 16.76 / **5.08** / – | 98.0 / 97.8 / 114.7 |
| scroll20k | 68.5 / 67.3 / – | 16 / 16 / – | 0.2 / **0.0** / – | 13.97 / **3.27** / – | **77.5** / 93.6 / 99.8 |
| live20k | 63.7 / 64.0 / – | 16 / 16 / – | 0.6 / **0.0** / – | 16.31 / **4.69** / – | 97.8 / 99.5 / 98.9 |
| storm20k | 58.2 / **62.1** / – | 16 / 16 / – | **0.0** / 0.0 / – | 16.59 / **4.99** / – | **94.4** / 104.0 / 99.4 |
| thermal | 60.0 / 60.5 / – | 16 / 16 / – | 0.3 / **0.0** / – | 16.68 / **4.25** / – | 98.4 / 98.9 / 99.6 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 4.31 / 4.54 | 4.13 / 4.34 | 13.85 / 14.25 | 0.0 / 0.0 |
| live2k | 8.49 / 8.45 | 3.73 / 3.84 | 16.58 / 16.79 | 0.7 / 0.4 |
| storm2k | 10.56 / 10.40 | 2.41 / 2.39 | 16.76 / 16.64 | 0.0 / 0.0 |
| scroll20k | 4.45 / 4.50 | 4.27 / 4.62 | 13.97 / 13.88 | 0.2 / 0.0 |
| live20k | 8.48 / 8.44 | 3.76 / 3.75 | 16.31 / 16.83 | 0.6 / 0.6 |
| storm20k | 10.49 / 10.44 | 2.48 / 2.47 | 16.59 / 16.97 | 0.0 / 0.2 |
| thermal | 8.45 / 8.50 | 3.76 / 3.75 | 16.68 / 16.85 | 0.3 / 0.4 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.22 / 0.99 / 0.56 / 0.00 / 0.00 \n 0.23 / 1.06 / 0.58 / 0.00 / 0.00 |
| live2k | 0.63 / 3.32 / 0.51 / 0.00 / 0.00 \n 0.63 / 3.30 / 0.51 / 0.00 / 0.00 |
| storm2k | 0.48 / 4.10 / 0.90 / 0.00 / 0.00 \n 0.47 / 4.06 / 0.84 / 0.00 / 0.00 |
| scroll20k | 0.22 / 1.05 / 0.59 / 0.00 / 0.00 \n 0.22 / 1.07 / 0.62 / 0.00 / 0.00 |
| live20k | 0.63 / 3.36 / 0.51 / 0.00 / 0.00 \n 0.63 / 3.36 / 0.51 / 0.00 / 0.00 |
| storm20k | 0.49 / 4.16 / 0.82 / 0.00 / 0.00 \n 0.48 / 4.15 / 0.84 / 0.00 / 0.00 |
| thermal | 0.61 / 3.32 / 0.51 / 0.00 / 0.00 \n 0.62 / 3.34 / 0.52 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 20.96 / **6.71** / – | – / 2 / **19** | 0.3 / 0.3 / 69.3 |
| tick2k | 47.90 / **18.65** / – | – / – / – | **18.2** / 57.7 / 23.6 |
| jump2k | 93.53 / **59.19** / – | **48** / 22 / – | **58.4** / 88.2 / 83.3 |
| load20k | 25.37 / **7.61** / – | – / – / 55 | **0.0** / 0.0 / 107.1 |
| tick20k | 18.04 / **5.59** / – | – / – / – | 14.2 / **0.0** / 96.8 |
| jump20k | 80.00 / **13.41** / – | **55** / 35 / – | **74.1** / 105.8 / 109.1 |
| idle2k | **0.38** / 3.03 / – | – / – / – | 5.1 / 4.1 / 4.0 |
| idle20k | **0.52** / 3.69 / – | – / – / – | 4.7 / **1.7** / 97.6 |

### LMQ72016369a75 · Android 9 · msm8953 · 2.7 GB · 60.000004 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS, vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier low), fill off (tier low), raster threads serial (tier low)`
- vieww-cpuserial: `blur off (tier low), fill off (tier low), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 370 / **345** / 492 |
| memory after the run, PSS (MB) | **76.0** / 183.4 / 248.8 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | – / – / – | – / – / – | – / – / – | **32.17** / 180.74 / – | 143.2 / **44.5** / 101.8 |
| live2k | – / – / – | – / – / – | – / – / – | **53.12** / 63.12 / – | 131.2 / **57.3** / 99.9 |
| storm2k | – / – / – | – / – / – | – / – / – | 76.67 / **61.46** / – | 109.9 / **76.2** / 120.2 |
| scroll20k | – / – / – | – / – / – | – / – / – | **31.11** / 59.70 / – | 148.1 / **27.5** / 105.5 |
| live20k | – / – / – | – / – / – | – / – / – | **53.66** / 61.40 / – | 132.6 / **57.0** / 98.1 |
| storm20k | – / – / – | – / – / – | – / – / – | 68.23 / **61.08** / – | 110.2 / **76.9** / 98.7 |
| thermal | – / – / – | – / – / – | – / – / – | **53.41** / 61.02 / – | 130.5 / **57.5** / 98.7 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 17.95 / 17.67 | 6.47 / 6.48 | 32.17 / 32.01 | – / – |
| live2k | 39.15 / 38.74 | 6.52 / 6.63 | 53.12 / 53.94 | – / – |
| storm2k | 60.93 / 62.15 | 4.45 / 4.34 | 76.67 / 72.13 | – / – |
| scroll20k | 17.86 / 17.47 | 6.47 / 6.55 | 31.11 / 31.22 | – / – |
| live20k | 39.27 / 39.35 | 6.56 / 6.69 | 53.66 / 53.56 | – / – |
| storm20k | 57.76 / 64.56 | 4.38 / 4.44 | 68.23 / 74.89 | – / – |
| thermal | 39.26 / 38.77 | 6.50 / 6.62 | 53.41 / 53.06 | – / – |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.69 / 4.22 / 2.74 / 0.00 / 0.00 \n 0.70 / 4.22 / 2.69 / 0.00 / 0.00 |
| live2k | 2.64 / 15.67 / 2.79 / 0.00 / 0.00 \n 2.64 / 15.65 / 2.76 / 0.00 / 0.00 |
| storm2k | 2.67 / 30.51 / 6.30 / 0.00 / 0.00 \n 3.00 / 28.61 / 7.08 / 0.00 / 0.00 |
| scroll20k | 0.62 / 4.21 / 2.67 / 0.00 / 0.00 \n 0.62 / 4.16 / 2.64 / 0.00 / 0.00 |
| live20k | 2.65 / 15.88 / 2.78 / 0.00 / 0.00 \n 2.66 / 15.91 / 2.77 / 0.00 / 0.00 |
| storm20k | 2.65 / 28.14 / 6.24 / 0.00 / 0.00 \n 2.99 / 29.14 / 7.89 / 0.00 / 0.00 |
| thermal | 2.59 / 15.96 / 2.77 / 0.00 / 0.00 \n 2.59 / 15.89 / 2.76 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 203.97 / **45.95** / – | – / – / – | **0.3** / 1.0 / 99.5 |
| tick2k | 40.69 / **30.20** / – | – / – / – | **11.3** / 22.4 / 85.5 |
| jump2k | 178.48 / **60.46** / – | – / – / – | 240.0 / **16.8** / 84.0 |
| load20k | 147.62 / **33.86** / – | – / – / – | **0.0** / 0.0 / 128.0 |
| tick20k | 40.51 / **28.05** / – | – / – / – | 28.7 / **0.0** / 118.9 |
| jump20k | 163.67 / **34.19** / – | – / – / – | 157.0 / 9.4 / **8.1** |
| idle2k | **1.09** / 11.84 / – | – / – / – | **0.0** / 5.1 / 99.2 |
| idle20k | **2.67** / 6.67 / – | – / – / – | **0.0** / 4.4 / 115.4 |

### R5CY609HEBF · Android 15 · s5e8835 · 5.3 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)`
- vieww-cpuserial: `blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 227 / 221 / 333 |
| memory after the run, PSS (MB) | **109.6** / 158.6 / 443.5 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 70.5 / 69.8 / 60.8 | 16 / 16 / 16 | 100.0 / 100.0 / 100.0 | 14.84 / **9.51** / – | 77.4 / 76.5 / 98.0 |
| live2k | 64.0 / 64.9 / 64.1 | 16 / 16 / 16 | 100.0 / 100.0 / 100.0 | 17.14 / **11.43** / – | 96.8 / 94.9 / 117.1 |
| storm2k | 47.9 / 61.3 / 63.6 | 33 / 16 / 16 | 100.0 / 100.0 / 100.0 | 23.24 / **12.55** / – | 107.7 / **96.8** / 121.7 |
| scroll20k | 71.0 / 69.0 / – | 16 / 16 / – | 100.0 / 100.0 / – | 16.23 / **8.64** / – | **74.5** / 80.2 / 99.2 |
| live20k | 65.1 / 65.1 / – | 16 / 16 / – | 100.0 / 100.0 / – | 16.96 / **7.63** / – | 96.6 / **89.5** / 98.5 |
| storm20k | 63.1 / 62.8 / – | 16 / 16 / – | 100.0 / 100.0 / – | 16.59 / **11.65** / – | 104.1 / 100.9 / 99.1 |
| thermal | 60.3 / 60.5 / – | 16 / 16 / – | 100.0 / 100.0 / – | 16.77 / **9.77** / – | 96.2 / **84.8** / 99.0 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 5.48 / 5.55 | 3.99 / 3.92 | 14.84 / 15.27 | 100.0 / 100.0 |
| live2k | 9.86 / 9.78 | 3.41 / 3.39 | 17.14 / 17.00 | 100.0 / 100.0 |
| storm2k | 17.91 / 11.16 | 2.57 / 2.49 | 23.24 / 16.13 | 100.0 / 100.0 |
| scroll20k | 5.58 / 5.51 | 3.91 / 3.98 | 16.23 / 15.23 | 100.0 / 100.0 |
| live20k | 9.98 / 9.91 | 3.45 / 3.46 | 16.96 / 17.10 | 100.0 / 100.0 |
| storm20k | 11.37 / 18.04 | 2.56 / 2.58 | 16.59 / 22.98 | 100.0 / 100.0 |
| thermal | 9.87 / 9.77 | 3.43 / 3.44 | 16.77 / 16.81 | 100.0 / 100.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.26 / 1.23 / 0.66 / 0.00 / 0.00 \n 0.27 / 1.22 / 0.67 / 0.00 / 0.00 |
| live2k | 0.77 / 3.72 / 0.54 / 0.00 / 0.00 \n 0.77 / 3.70 / 0.53 / 0.00 / 0.00 |
| storm2k | 1.01 / 7.18 / 1.76 / 0.00 / 0.00 \n 0.98 / 5.90 / 0.06 / 0.00 / 0.00 |
| scroll20k | 0.22 / 1.25 / 0.69 / 0.00 / 0.00 \n 0.22 / 1.22 / 0.68 / 0.00 / 0.00 |
| live20k | 0.79 / 3.80 / 0.54 / 0.00 / 0.00 \n 0.79 / 3.79 / 0.52 / 0.00 / 0.00 |
| storm20k | 1.02 / 6.01 / 0.06 / 0.00 / 0.00 \n 1.02 / 7.24 / 1.78 / 0.00 / 0.00 |
| thermal | 0.77 / 3.73 / 0.54 / 0.00 / 0.00 \n 0.77 / 3.71 / 0.52 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 31.54 / **9.44** / – | – / 2 / – | **0.0** / 0.7 / 104.2 |
| tick2k | 20.34 / 19.86 / – | – / – / – | **0.0** / 0.0 / 0.0 |
| jump2k | 49.91 / **25.79** / – | 56 / 55 / – | 42.4 / 101.0 / **0.0** |
| load20k | 36.07 / **10.71** / – | – / – / 88 | **0.0** / 0.8 / 116.2 |
| tick20k | 15.92 / **12.04** / – | – / – / – | **0.0** / 0.0 / 89.3 |
| jump20k | 44.03 / **14.78** / – | **70** / 45 / – | 54.6 / **50.0** / 93.3 |
| idle2k | **0.20** / 7.22 / – | – / – / – | 4.0 / 2.8 / **0.0** |
| idle20k | **0.26** / 5.50 / – | – / – / – | **1.6** / 2.8 / 99.2 |

### RFGYB1EN8LF · Android 16 · SM8750 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: FAIL (no frames presented in thermal: the app is not drawing), vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 112 / 114 / 141 |
| memory after the run, PSS (MB) | **95.1** / 155.7 / 506.3 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 131.5 / 133.6 / 127.6 | 8 / 8 / 8 | 0.3 / **0.0** / 0.0 | 8.01 / **3.80** / – | 76.7 / 66.4 / **55.3** |
| live2k | 127.5 / 127.1 / 125.8 | 8 / 8 / 8 | 0.8 / 0.2 / 0.2 | 8.60 / **3.33** / – | 112.9 / **80.4** / 95.3 |
| storm2k | 123.5 / 59.1 / 122.5 | 8 / 16 / 8 | **0.1** / 100.0 / 2.1 | **7.96** / 14.78 / – | 106.3 / **60.6** / 106.7 |
| scroll20k | **132.2** / 66.5 / – | **8** / 16 / – | **0.4** / 100.0 / – | 8.01 / **6.54** / – | 79.2 / **29.3** / 99.5 |
| live20k | **128.4** / 64.4 / – | **8** / 16 / – | **0.9** / 100.0 / – | **8.40** / 14.43 / – | 112.3 / **58.2** / 99.5 |
| storm20k | **119.7** / 62.6 / – | **8** / 16 / – | **0.1** / 100.0 / – | **8.15** / 14.22 / – | 104.0 / **65.9** / 99.3 |
| thermal | 60.8 / 60.2 / – | 16 / 16 / – | 98.7 / 100.0 / – | **14.18** / 16.37 / – | 75.0 / **58.2** / 99.2 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 2.40 / 2.41 | 1.65 / 1.66 | 8.01 / 7.87 | 0.3 / 0.3 |
| live2k | 3.91 / 3.91 | 2.44 / 2.56 | 8.60 / 8.51 | 0.8 / 0.4 |
| storm2k | 4.17 / 4.32 | 2.26 / 2.28 | 7.96 / 7.99 | 0.1 / 0.2 |
| scroll20k | 2.42 / 2.38 | 1.67 / 1.67 | 8.01 / 8.00 | 0.4 / 0.3 |
| live20k | 3.98 / 3.93 | 2.46 / 2.49 | 8.40 / 8.65 | 0.9 / 0.8 |
| storm20k | 4.64 / 4.19 | 2.05 / 2.11 | 8.15 / 7.88 | 0.1 / 0.1 |
| thermal | 7.78 / 7.28 | 1.89 / 1.98 | 14.18 / 13.53 | 98.7 / 74.8 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.09 / 0.67 / 0.35 / 0.00 / 0.00 \n 0.09 / 0.68 / 0.35 / 0.00 / 0.00 |
| live2k | 0.32 / 1.66 / 0.14 / 0.00 / 0.00 \n 0.32 / 1.66 / 0.14 / 0.00 / 0.00 |
| storm2k | 0.38 / 2.32 / 0.01 / 0.00 / 0.00 \n 0.39 / 2.47 / 0.01 / 0.00 / 0.00 |
| scroll20k | 0.09 / 0.68 / 0.36 / 0.00 / 0.00 \n 0.08 / 0.67 / 0.35 / 0.00 / 0.00 |
| live20k | 0.33 / 1.69 / 0.14 / 0.00 / 0.00 \n 0.33 / 1.68 / 0.14 / 0.00 / 0.00 |
| storm20k | 0.38 / 2.62 / 0.01 / 0.00 / 0.00 \n 0.37 / 2.33 / 0.01 / 0.00 / 0.00 |
| thermal | 0.57 / 3.37 / 0.42 / 0.00 / 0.00 \n 0.52 / 3.15 / 0.39 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 13.23 / **3.48** / – | – / 2 / – | 0.3 / **0.0** / 40.0 |
| tick2k | 31.24 / **15.04** / – | – / – / – | **0.0** / 2.0 / 5.7 |
| jump2k | 40.19 / **18.42** / – | **54** / 30 / – | 73.4 / 36.4 / **0.0** |
| load20k | 13.17 / **8.86** / – | – / – / 18 | **0.0** / 0.0 / 99.8 |
| tick20k | 13.77 / **9.35** / – | – / – / – | **0.0** / 8.5 / 97.0 |
| jump20k | 32.23 / **16.12** / – | **57** / 19 / – | 77.7 / **33.6** / 107.6 |
| idle2k | **0.26** / 2.70 / – | – / – / – | 2.8 / 1.0 / **0.0** |
| idle20k | **0.17** / 2.02 / – | – / – / – | 1.7 / **0.9** / 101.7 |

### R3GYC0JRP5F · Android 16 · SM8850 · 10.9 GB · 120.00001 Hz

Runs — vieww-cpu: PASS, vieww-gpu: PASS, valdi: PASS, vieww-cpuserial: PASS
- vieww-cpu: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)`
- vieww-cpuserial: `blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=1)`

| | Vieww CPU / Vieww GPU / Valdi |
|---|---|
| cold start, median of 5 (ms) | 104 / 100 / 127 |
| memory after the run, PSS (MB) | **97.6** / 232.8 / 1039.3 |
| APK (MB) | 2.7 / 2.7 / 9.2 |

| phase | presented fps | frame gap p95 (ms) | jank % | app work p95 (ms) | CPU % |
|---|---|---|---|---|---|
| scroll2k | 62.8 / 52.0 / **120.9** | 20 / 20 / **16** | 100.0 / 100.0 / **6.3** | 22.25 / **5.01** / – | 64.4 / 68.0 / **32.7** |
| live2k | 63.9 / 50.4 / **124.4** | 16 / 20 / **8** | 100.0 / 100.0 / **1.3** | 14.28 / **4.19** / – | **72.9** / 77.6 / 93.6 |
| storm2k | 60.2 / 48.2 / **117.0** | 16 / 20 / 16 | 100.0 / 100.0 / **7.3** | 16.46 / **4.44** / – | 81.6 / **77.5** / 106.4 |
| scroll20k | **64.8** / 51.7 / – | **16** / 20 / – | 100.0 / 100.0 / – | 13.13 / **5.10** / – | **63.9** / 67.9 / 100.2 |
| live20k | **62.9** / 50.3 / – | **16** / 20 / – | 100.0 / 100.0 / – | 14.37 / **4.18** / – | **72.8** / 77.4 / 98.9 |
| storm20k | **58.8** / 48.4 / – | **16** / 20 / – | 100.0 / 100.0 / – | 13.96 / **4.36** / – | 79.7 / 78.6 / 99.2 |
| thermal | **60.2** / 48.1 / 18.7 | 16 / 20 / 16 | 100.0 / 100.0 / **5.0** | 14.38 / **4.58** / – | **72.7** / 77.9 / 88.6 |

<details open><summary>Vieww CPU · parallel vs serial raster (same phone, same run)</summary>

Cells read parallel / serial; the raster kinds are the p50 of the raster stage split by command kind (a profiled run's `kinds` field).

| phase | raster p50 (ms) | present p50 (ms) | work p95 (ms) | jank % |
|---|---|---|---|---|
| scroll2k | 6.21 / 6.31 | 2.29 / 2.35 | 22.25 / 22.10 | 100.0 / 100.0 |
| live2k | 9.06 / 9.42 | 1.32 / 1.29 | 14.28 / 16.90 | 100.0 / 100.0 |
| storm2k | 11.22 / 11.48 | 0.92 / 0.97 | 16.46 / 17.58 | 100.0 / 100.0 |
| scroll20k | 5.70 / 5.61 | 2.17 / 1.91 | 13.13 / 13.00 | 100.0 / 100.0 |
| live20k | 8.94 / 8.80 | 1.33 / 1.35 | 14.37 / 14.13 | 100.0 / 100.0 |
| storm20k | 11.39 / 11.12 | 0.84 / 0.94 | 13.96 / 16.14 | 100.0 / 100.0 |
| thermal | 8.97 / 8.65 | 1.33 / 1.33 | 14.38 / 14.02 | 100.0 / 100.0 |

| phase | raster kinds, p50 (ms): text / paths / shadows / layers / composite |
|---|---|
| scroll2k | 0.34 / 1.78 / 1.01 / 0.00 / 0.00 \n 0.35 / 1.84 / 1.04 / 0.00 / 0.00 |
| live2k | 0.68 / 4.06 / 0.56 / 0.00 / 0.00 \n 0.69 / 4.25 / 0.58 / 0.00 / 0.00 |
| storm2k | 0.58 / 4.75 / 1.00 / 0.00 / 0.00 \n 0.61 / 4.97 / 1.03 / 0.00 / 0.00 |
| scroll20k | 0.24 / 1.66 / 0.99 / 0.00 / 0.00 \n 0.23 / 1.55 / 0.98 / 0.00 / 0.00 |
| live20k | 0.67 / 4.02 / 0.53 / 0.00 / 0.00 \n 0.66 / 3.93 / 0.51 / 0.00 / 0.00 |
| storm20k | 0.60 / 4.96 / 1.05 / 0.00 / 0.00 \n 0.60 / 4.83 / 1.01 / 0.00 / 0.00 |
| thermal | 0.67 / 4.03 / 0.53 / 0.00 / 0.00 \n 0.65 / 3.86 / 0.51 / 0.00 / 0.00 |

</details>


| event | app time (ms) | frames presented | CPU % |
|---|---|---|---|
| load2k | 11.66 / **2.68** / – | – / **2** / 1 | **0.0** / 0.0 / 27.0 |
| tick2k | 22.95 / **10.48** / – | – / – / – | 7.4 / **4.2** / 6.6 |
| jump2k | 37.92 / **14.22** / – | **19** / 15 / – | 75.9 / 46.7 / **0.0** |
| load20k | 21.09 / **3.46** / – | – / – / 7 | **0.0** / 0.0 / 99.8 |
| tick20k | 12.75 / **3.62** / – | – / – / – | 10.3 / **0.0** / 102.5 |
| jump20k | 34.74 / **6.61** / – | 9 / **10** / – | **24.2** / 29.4 / 101.3 |
| idle2k | **0.34** / 1.08 / – | – / – / – | 3.2 / 1.5 / **0.8** |
| idle20k | **0.31** / 2.25 / – | – / – / – | 2.6 / **1.2** / 103.9 |

### Across phones · median presented fps

| phase | 0C171FDD4000JL | 33111FDH300270 | 3C011JEKB21900 | 4A191JEBF28019 | LMQ72016369a75 | R5CY609HEBF | RFGYB1EN8LF | R3GYC0JRP5F |
|---|---|---|---|---|---|---|---|---|
| scroll2k | 67.3 / 0.6 / **79.2** | 69.9 / **134.9** / 123.1 | 72.8 / 73.7 / 72.8 | 67.4 / 67.0 / 62.3 | – / – / – | 70.5 / 69.8 / 60.8 | 131.5 / 133.6 / 127.6 | 62.8 / 52.0 / **120.9** |
| live2k | 56.8 / 0.5 / **87.1** | 64.2 / **129.4** / 103.6 | 64.3 / 66.1 / 63.1 | 63.6 / 64.1 / 62.2 | – / – / – | 64.0 / 64.9 / 64.1 | 127.5 / 127.1 / 125.8 | 63.9 / 50.4 / **124.4** |
| storm2k | 36.6 / 2.1 / **62.9** | 63.1 / **122.5** / 66.5 | 57.2 / **61.0** / 50.4 | 60.3 / 59.1 / 56.6 | – / – / – | 47.9 / 61.3 / 63.6 | 123.5 / 59.1 / 122.5 | 60.2 / 48.2 / **117.0** |
| scroll20k | **66.6** / 0.1 / – | 67.5 / **132.6** / – | 72.6 / 72.4 / – | 68.5 / 67.3 / – | – / – / – | 71.0 / 69.0 / – | **132.2** / 66.5 / – | **64.8** / 51.7 / – |
| live20k | **56.4** / 0.5 / – | 64.0 / **129.3** / – | 65.0 / 65.7 / – | 63.7 / 64.0 / – | – / – / – | 65.1 / 65.1 / – | **128.4** / 64.4 / – | **62.9** / 50.3 / – |
| storm20k | **35.1** / 1.1 / – | 60.6 / **118.8** / – | 57.3 / **60.5** / – | 58.2 / **62.1** / – | – / – / – | 63.1 / 62.8 / – | **119.7** / 62.6 / – | **58.8** / 48.4 / – |
| thermal | **55.9** / 0.6 / – | 60.6 / **121.1** / – | 59.4 / 60.7 / – | 60.0 / 60.5 / – | – / – / – | 60.3 / 60.5 / – | 60.8 / 60.2 / – | **60.2** / 48.1 / 18.7 |

## Reading this

- **Presented fps** is what a user sees. A phase that changes something every frame can show at most the panel's rate.
- **Idle** phases should present nothing; any frames there are wasted battery.
- Valdi runs as native Android views drawn by HWUI; Vieww draws its own surface, on its CPU rasterizer or its GPU path.
- A production Valdi build narrates nothing (its runtime logs are compiled out), so the runner reads its phases from the schedule both apps share — 'app work' is blank for Valdi, its JS-side numbers being unobservable in a build that ships; the compositor and HWUI numbers cover it.
- Valdi's continuous phases are paced on its platform draw callback (`onNextDraw`), Vieww's on `before_frame`: one change per drawn frame on both sides.
- Configurations run in the order Vieww CPU → Valdi → Vieww CPU (serial, the A/B leg) → Vieww GPU with a minute's rest between, so heat from one run does not throttle the next; the thermal phase (60 s of live load) shows sustained behaviour. The serial leg draws the same screen with the raster walk pinned to one core (`debug.mw.threads=0`): the same-unit measure of what the row-parallel walk bought.
