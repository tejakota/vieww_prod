# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/646cc337-8061-461c-8ac3-257106820a88` — FAILED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | named | 9 | msm8953<br>Qualcomm, Adreno (TM) 506, OpenGL ES 3.2 V@331.0 (GIT@50f1b2 | 2.7 GB | 2.8 MB | 640 / 582 | median 33.776459ms, worst 51.943125ms, budget 16.666668ms (240 samples) | median 16.085ms, worst 23.097084ms, budget 16.666668ms (240 samples) | 0 of 600 / 600 of 600 | 77 / 146 | FAIL / FAIL / FAIL |
| Samsung Galaxy A73 5G | named | 12 | SM7325 (lahaina)<br>Qualcomm, Adreno (TM) 642L, OpenGL ES 3.2 V@0530.0 (GIT@cac6 | 7.1 GB | 2.8 MB | 324 / 322 | median 7.274115ms, worst 23.485312ms, budget 16.666668ms (240 samples) | median 2.379376ms, worst 6.159427ms, budget 8.333333ms (240 samples) | 0 of 600 / 600 of 600 | 80 / 101 | PASS / PASS / PASS |

**LG Stylo 5**
- gating passes: auto cpu gpu (auto chose: gpu)
- CPU path: median 33.776459ms, worst 51.943125ms, budget 16.666668ms (240 samples)
  - known failure (known-failures.txt, not failing the run): frames_inside_their_budget the_typical_frame_is_inside_its_budget — RC5, PENDING.md §4.7: the CPU path on an msm8953 is 35 ms against 16.7 ms (run 39594824376), with the raster stage about 30 ms whatever the damage. The GPU path now passes here - 11.7 ms with frames overlapping, 16.4 ms without - and `auto` ships it; this is the fallback's bar.
  - memory: PSS 77 MB
  - start-up: before run 541 ms | app setup 0.1 | event loop started 0.0 | resumed 35.5 | window created 0.0 | present ready 1.9 | accessibility and show 0.0 | tree built 16.0 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 16.5 | first build and layout 6.6 | first raster and present 112.2 = 729.9 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.52 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.70
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 34.73 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.48 present 4.40
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.48 rects 23.72 images 0.00 layers 0.00 composite 0.00 total 24.20; p95 ms: text 0.00 shadows 0.00 paths 0.72 rects 28.62 images 0.00 layers 0.00 composite 0.00 total 29.34
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.33 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 18 s
  - input to present: input to present ms: median 40.1 p95 51.9 p99 52.1 worst 52.1 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 51.94 ms, outside the renderer 1.45 ms, on cpu5; stages ms: scale 0.08 plan 0.00 cpu 46.36 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.35 present 3.71
  - raster by core: cpu6 69 frames 28.29 ms | cpu5 68 frames 29.00 ms | cpu4 57 frames 28.74 ms | cpu0 21 frames 28.19 ms | cpu7 14 frames 28.79 ms | cpu2 11 frames 28.03 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.709038ms / p95 39.913698ms / worst 51.943125ms vs budget 16.666668ms · 240/240 over
- GPU path: median 16.085ms, worst 23.097084ms, budget 16.666668ms (240 samples)
  - **gate failed, run failed**: frames_inside_their_budget
  - memory: PSS 146 MB
  - start-up: before run 544 ms | app setup 0.2 | event loop started 0.0 | resumed 32.6 | window created 0.1 | present ready 57.7 | accessibility and show 0.0 | tree built 14.2 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 15.3 | first build and layout 5.8 | first raster and present 43.6 = 714.3 ms from process start
  - stages: medians ms: scale 0.09 plan 0.90 cpu 0.00 gpu_upload 1.01 gpu_record 0.73 gpu_submit 1.17 gpu_wait 9.69 gpu_readback 0.00 acquire 0.32 present 0.56
  - stages: p95 ms: scale 0.20 plan 1.53 cpu 0.00 gpu_upload 1.70 gpu_record 1.33 gpu_submit 1.92 gpu_wait 10.37 gpu_readback 0.00 acquire 0.49 present 0.92
  - GPU time by kind of work: medians ms: draw 12.19 layers 0.00 filters 0.00 shadows 0.00 blit 0.32 total 12.51; p95 ms: draw 13.20 layers 0.00 filters 0.00 shadows 0.00 blit 0.41 total 13.61
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 589 partial and 11 full copies, 9.7% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.25 ms per frame over the last 240 frames, every thread (37% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 11 s
  - input to present: input to present ms: median 16.3 p95 17.0 p99 34.6 worst 34.6 over 12 inputs in 12 frames; 0 unanswered within 1 s
  - worst frame: work 23.10 ms, outside the renderer 1.58 ms; stages ms: scale 0.12 plan 1.07 cpu 0.00 gpu_upload 4.95 gpu_record 0.72 gpu_submit 1.55 gpu_wait 12.23 gpu_readback 0.00 acquire 0.36 present 0.52
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 16.19252ms / p95 18.929063ms / worst 23.097084ms vs budget 16.666668ms · 58/240 over
- chosen by measurement (auto): median 14.464843ms, worst 26.120312ms, budget 16.666668ms (240 samples)
  - **gate failed, run failed**: frames_inside_their_budget
  - memory: PSS 146 MB
  - renderer: gpu (calibrated earlier: cpu 18.84 ms, gpu 11.64 ms)
  - calibration launches: launch 1: 10-29 04:42:42.640 23941 23958 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer cpu, measuring (first launch with this build here) 10-29 04:42:44.228 23941 23958 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer cpu for now; the gpu is measured next launch (measured: cpu 18.84 ms; budget 16.67 ms) launch 2: 10-29 04:42:52.056 24013 24030 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu, measuring (the cpu path measured 18.84 ms last launch) 10-29 04:42:54.745 24013 24030 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu (measured: cpu 18.84 ms, gpu 11.64 ms; budget 16.67 ms) launch 3: 10-29 04:43:01.397 24081 24098 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu (calibrated earlier: cpu 18.84 ms, gpu 11.64 ms)
  - start-up: before run 585 ms | app setup 0.2 | event loop started 0.0 | resumed 31.0 | window created 0.1 | present ready 57.6 | accessibility and show 0.0 | tree built 13.6 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 13.8 | first build and layout 6.0 | first raster and present 43.3 = 750.7 ms from process start
  - stages: medians ms: scale 0.09 plan 0.84 cpu 0.00 gpu_upload 0.89 gpu_record 0.73 gpu_submit 1.15 gpu_wait 7.87 gpu_readback 0.00 acquire 0.31 present 0.55
  - stages: p95 ms: scale 0.20 plan 1.56 cpu 0.00 gpu_upload 1.56 gpu_record 1.23 gpu_submit 1.87 gpu_wait 9.66 gpu_readback 0.00 acquire 0.47 present 0.90
  - GPU time by kind of work: medians ms: draw 11.14 layers 0.00 filters 0.00 shadows 0.00 blit 0.29 total 11.43; p95 ms: draw 12.47 layers 0.00 filters 0.00 shadows 0.00 blit 0.37 total 12.84
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 589 partial and 11 full copies, 9.7% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.12 ms per frame over the last 240 frames, every thread (41% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 38.9) over 10 s
  - input to present: input to present ms: median 15.8 p95 20.2 p99 20.3 worst 20.3 over 12 inputs in 12 frames; 0 unanswered within 1 s
  - worst frame: work 26.12 ms, outside the renderer 1.83 ms; stages ms: scale 0.14 plan 1.12 cpu 0.00 gpu_upload 0.96 gpu_record 1.26 gpu_submit 11.58 gpu_wait 8.14 gpu_readback 0.00 acquire 0.31 present 0.78
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 14.416792ms / p95 17.818073ms / worst 26.120312ms vs budget 16.666668ms · 19/240 over
- CPU path presented through Vulkan (diagnostic): median 45.467865ms, worst 89.374219ms, budget 16.666668ms (240 samples)
  - memory: PSS 128 MB
  - start-up: before run 533 ms | app setup 0.1 | event loop started 0.0 | resumed 35.6 | window created 0.0 | present ready 64.5 | accessibility and show 0.0 | tree built 13.3 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 14.0 | first build and layout 4.7 | first raster and present 160.0 = 825.3 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 21.88 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 19.22
  - stages: p95 ms: scale 0.13 plan 0.00 cpu 37.48 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 29.75
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.53 rects 19.28 images 0.00 layers 0.00 composite 0.00 total 19.81; p95 ms: text 0.00 shadows 0.00 paths 1.05 rects 31.14 images 0.00 layers 0.00 composite 0.00 total 32.19
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 31.17 ms per frame over the last 240 frames, every thread (67% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.3) over 27 s
  - input to present: input to present ms: median 57.4 p95 75.4 p99 89.5 worst 89.5 over 34 inputs in 34 frames; 0 unanswered within 1 s
  - worst frame: work 89.37 ms, outside the renderer 1.51 ms, on cpu6; stages ms: scale 0.08 plan 0.00 cpu 58.03 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 29.76
  - raster by core: cpu4 73 frames 23.00 ms | cpu5 59 frames 18.42 ms | cpu7 47 frames 27.55 ms | cpu6 34 frames 27.33 ms | cpu0 22 frames 18.53 ms | cpu1 3 frames 29.90 ms | cpu3 2 frames 32.26 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 46.010725ms / p95 62.989479ms / worst 89.374219ms vs budget 16.666668ms · 240/240 over
- CPU path with an Android performance hint (diagnostic): median 34.182135ms, worst 52.781562ms, budget 16.666668ms (240 samples)
  - memory: PSS 78 MB
  - start-up: before run 560 ms | app setup 0.2 | event loop started 0.0 | resumed 35.4 | window created 0.1 | present ready 2.2 | accessibility and show 0.0 | tree built 14.0 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 17.7 | first build and layout 8.2 | first raster and present 133.7 = 771.5 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.84 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.38 present 3.84
  - stages: p95 ms: scale 0.10 plan 0.00 cpu 36.96 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.46 present 4.73
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.49 rects 23.99 images 0.00 layers 0.00 composite 0.00 total 24.48; p95 ms: text 0.00 shadows 0.00 paths 0.72 rects 30.26 images 0.00 layers 0.00 composite 0.00 total 30.98
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 35.33 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.3 -> 37.9 °C (peak 38.6) over 19 s
  - input to present: input to present ms: median 40.6 p95 52.9 p99 57.2 worst 57.2 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 52.78 ms, outside the renderer 1.29 ms, on cpu5; stages ms: scale 0.07 plan 0.00 cpu 46.85 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 4.19
  - raster by core: cpu5 60 frames 28.54 ms | cpu7 51 frames 29.18 ms | cpu6 44 frames 29.25 ms | cpu3 26 frames 28.63 ms | cpu4 24 frames 28.52 ms | cpu0 23 frames 27.99 ms | cpu1 9 frames 31.95 ms | cpu2 3 frames 28.55 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - performance hint: unavailable (Android 12 or older: no APerformanceHint)
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 35.680289ms / p95 43.114584ms / worst 52.781562ms vs budget 16.666668ms · 240/240 over
- GPU path at the device tier's resolution (diagnostic): median 6.842917ms, worst 12.807656ms, budget 16.666668ms (240 samples)
  - memory: PSS 84 MB
  - start-up: before run 546 ms | app setup 0.2 | event loop started 0.0 | resumed 30.7 | window created 0.1 | present ready 67.8 | accessibility and show 0.0 | tree built 13.7 | window ready 0.6 | until first draw 0.1 | app before-frame hooks 15.0 | first build and layout 5.3 | first raster and present 30.1 = 709.7 ms from process start
  - stages: medians ms: scale 0.07 plan 0.68 cpu 0.00 gpu_upload 0.71 gpu_record 0.58 gpu_submit 0.46 gpu_wait 1.96 gpu_readback 0.00 acquire 0.29 present 0.42
  - stages: p95 ms: scale 0.13 plan 1.04 cpu 0.00 gpu_upload 0.97 gpu_record 0.81 gpu_submit 0.67 gpu_wait 3.72 gpu_readback 0.00 acquire 0.38 present 0.57
  - GPU time by kind of work: medians ms: draw 2.09 layers 0.00 filters 0.00 shadows 0.00 blit 2.31 total 4.40; p95 ms: draw 3.27 layers 0.00 filters 0.00 shadows 0.00 blit 2.66 total 5.93
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: probing: 0 partial and 600 full copies, 100.0% of the frames' pixels copied; 0 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 4.46 ms per frame over the last 240 frames, every thread (61% of one core)
  - thermal: hottest zone 37.0 -> 41.2 °C (peak 41.2) over 5 s
  - input to present: no input reached this window
  - worst frame: work 12.81 ms, outside the renderer 1.29 ms; stages ms: scale 0.07 plan 0.69 cpu 0.00 gpu_upload 0.73 gpu_record 0.67 gpu_submit 0.68 gpu_wait 7.84 gpu_readback 0.00 acquire 0.27 present 0.57
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - gpu resolution: GPU path renders at 0.50x resolution (VIEWW_GPU_SCALE=auto: tier low)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 7.043326ms / p95 8.497188ms / worst 12.807656ms vs budget 16.666668ms · 0/240 over · GPU path renders at 0.50x resolution (VIEWW_GPU_SCALE=auto: tier low)
- GPU path waiting for every frame, without the overlap (diagnostic A/B): median 23.617969ms, worst 36.748125ms, budget 16.666668ms (240 samples)
  - memory: PSS 146 MB
  - start-up: before run 547 ms | app setup 0.2 | event loop started 0.0 | resumed 29.5 | window created 0.1 | present ready 69.6 | accessibility and show 0.0 | tree built 15.2 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 14.3 | first build and layout 7.9 | first raster and present 242.2 = 926.6 ms from process start
  - stages: medians ms: scale 0.08 plan 0.84 cpu 0.00 gpu_upload 1.08 gpu_record 0.79 gpu_submit 1.19 gpu_wait 15.48 gpu_readback 0.00 acquire 0.37 present 0.37
  - stages: p95 ms: scale 0.20 plan 1.87 cpu 0.00 gpu_upload 2.15 gpu_record 1.53 gpu_submit 2.09 gpu_wait 18.67 gpu_readback 0.00 acquire 0.61 present 0.60
  - GPU time by kind of work: medians ms: draw 14.60 layers 0.00 filters 0.00 shadows 0.00 blit 0.47 total 15.07; p95 ms: draw 17.79 layers 0.00 filters 0.00 shadows 0.00 blit 0.52 total 18.31
  - CPU/GPU overlap: off: every GPU frame waited for before the next (VIEWW_GPU_OVERLAP=0)
  - copies into the swapchain: trusted: 589 partial and 11 full copies, 11.6% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.33 ms per frame over the last 240 frames, every thread (26% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 15 s
  - input to present: input to present ms: median 28.2 p95 35.1 p99 39.4 worst 39.4 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 36.75 ms, outside the renderer 2.20 ms; stages ms: scale 0.21 plan 1.47 cpu 0.00 gpu_upload 1.67 gpu_record 10.41 gpu_submit 2.15 gpu_wait 17.70 gpu_readback 0.00 acquire 0.55 present 0.40
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 23.445885ms / p95 28.619167ms / worst 36.748125ms vs budget 16.666668ms · 240/240 over
- CPU path without raster threads, fill cache or blur cache (diagnostic A/B): median 34.407083ms, worst 52.855573ms, budget 16.666668ms (240 samples)
  - memory: PSS 78 MB
  - start-up: before run 494 ms | app setup 0.2 | event loop started 0.0 | resumed 30.7 | window created 0.1 | present ready 2.0 | accessibility and show 0.0 | tree built 14.6 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 17.5 | first build and layout 5.0 | first raster and present 105.1 = 669.8 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 29.06 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.76
  - stages: p95 ms: scale 0.16 plan 0.00 cpu 35.72 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.49 present 4.68
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.51 rects 24.28 images 0.00 layers 0.00 composite 0.00 total 24.79; p95 ms: text 0.00 shadows 0.00 paths 0.72 rects 28.92 images 0.00 layers 0.00 composite 0.00 total 29.64
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 35.17 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 38.9 -> 37.0 °C (peak 40.5) over 19 s
  - input to present: input to present ms: median 39.7 p95 51.5 p99 53.0 worst 53.0 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 52.86 ms, outside the renderer 1.30 ms, on cpu7; stages ms: scale 0.08 plan 0.00 cpu 46.59 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.34 present 4.54
  - raster by core: cpu4 120 frames 29.01 ms | cpu7 47 frames 29.20 ms | cpu6 33 frames 29.70 ms | cpu5 23 frames 29.06 ms | cpu1 9 frames 28.34 ms | cpu0 7 frames 27.83 ms | cpu3 1 frames 39.57 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier low · median 35.56223ms / p95 41.713699ms / worst 52.855573ms vs budget 16.666668ms · 240/240 over
- CPU path without the row-parallel walk only (diagnostic A/B): median 33.677604ms, worst 461.954792ms, budget 16.666668ms (240 samples)
  - memory: PSS 78 MB
  - start-up: before run 542 ms | app setup 0.2 | event loop started 0.0 | resumed 32.3 | window created 0.1 | present ready 1.7 | accessibility and show 0.0 | tree built 17.7 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 15.5 | first build and layout 7.0 | first raster and present 116.3 = 733.7 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.47 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.38 present 3.75
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 34.40 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.46 present 4.57
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.46 rects 23.61 images 0.00 layers 0.00 composite 0.00 total 24.07; p95 ms: text 0.00 shadows 0.00 paths 0.72 rects 27.70 images 0.00 layers 0.00 composite 0.00 total 28.42
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.38 ms per frame over the last 240 frames, every thread (92% of one core)
  - thermal: hottest zone 37.6 -> 38.9 °C (peak 41.2) over 19 s
  - input to present: input to present ms: median 39.9 p95 50.3 p99 52.0 worst 52.0 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 461.95 ms, outside the renderer 0.91 ms, on cpu4; stages ms: scale 0.07 plan 0.00 cpu 26.81 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 429.50 present 4.67
  - raster by core: cpu6 88 frames 28.69 ms | cpu0 38 frames 28.08 ms | cpu4 37 frames 28.47 ms | cpu2 32 frames 28.49 ms | cpu5 19 frames 29.46 ms | cpu7 19 frames 28.17 ms | cpu1 7 frames 28.14 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier low · median 36.498417ms / p95 40.518021ms / worst 461.954792ms vs budget 16.666668ms · 240/240 over
- CPU path without the path-fill cache only (diagnostic A/B): median 33.945573ms, worst 55.530156ms, budget 16.666668ms (240 samples)
  - memory: PSS 77 MB
  - start-up: before run 540 ms | app setup 0.3 | event loop started 0.0 | resumed 31.1 | window created 0.1 | present ready 2.2 | accessibility and show 0.0 | tree built 18.4 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 14.6 | first build and layout 8.0 | first raster and present 115.1 = 730.2 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.70 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.71
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 33.34 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.43 present 4.55
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.49 rects 23.87 images 0.00 layers 0.00 composite 0.00 total 24.36; p95 ms: text 0.00 shadows 0.00 paths 0.70 rects 27.26 images 0.00 layers 0.00 composite 0.00 total 27.96
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.58 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.6 -> 37.9 °C (peak 39.9) over 18 s
  - input to present: input to present ms: median 39.9 p95 55.7 p99 57.2 worst 57.2 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 55.53 ms, outside the renderer 1.49 ms, on cpu4; stages ms: scale 0.09 plan 0.00 cpu 49.53 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.34 present 4.08
  - raster by core: cpu6 92 frames 28.94 ms | cpu4 44 frames 28.14 ms | cpu5 39 frames 28.55 ms | cpu3 27 frames 29.77 ms | cpu7 19 frames 28.96 ms | cpu0 11 frames 27.35 ms | cpu1 6 frames 28.97 ms | cpu2 2 frames 31.75 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (VIEWW_PATH_CACHE_MB=0), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.873379ms / p95 39.140729ms / worst 55.530156ms vs budget 16.666668ms · 240/240 over
- CPU path without the blur cache only (diagnostic A/B): median 33.73901ms, worst 51.793281ms, budget 16.666668ms (240 samples)
  - memory: PSS 68 MB
  - start-up: before run 524 ms | app setup 0.2 | event loop started 0.0 | resumed 34.7 | window created 0.1 | present ready 2.1 | accessibility and show 0.0 | tree built 14.1 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 15.6 | first build and layout 7.3 | first raster and present 118.2 = 716.3 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.55 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.72
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 34.54 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.45 present 4.30
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.48 rects 23.76 images 0.00 layers 0.00 composite 0.00 total 24.24; p95 ms: text 0.00 shadows 0.00 paths 0.68 rects 28.45 images 0.00 layers 0.00 composite 0.00 total 29.14
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.25 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 38.6 -> 37.9 °C (peak 39.2) over 18 s
  - input to present: input to present ms: median 39.8 p95 52.0 p99 54.9 worst 54.9 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 51.79 ms, outside the renderer 1.34 ms, on cpu0; stages ms: scale 0.08 plan 0.00 cpu 46.07 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.43 present 3.87
  - raster by core: cpu4 104 frames 28.62 ms | cpu6 37 frames 28.79 ms | cpu7 31 frames 28.65 ms | cpu3 26 frames 28.06 ms | cpu0 22 frames 28.33 ms | cpu5 19 frames 28.93 ms | cpu1 1 frames 34.71 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.626337ms / p95 39.917031ms / worst 51.793281ms vs budget 16.666668ms · 240/240 over
- stress screen, CPU path (diagnostic): median 727.61552ms, worst 1.061649374s, budget 16.666668ms (240 samples)
  - memory: PSS 184 MB
  - start-up: before run 540 ms | app setup 0.2 | event loop started 0.0 | resumed 33.2 | window created 0.0 | present ready 1.8 | accessibility and show 0.0 | tree built 17.7 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 15.0 | first build and layout 12.4 | first raster and present 1107.7 = 1728.7 ms from process start
  - stages: medians ms: scale 0.05 plan 0.00 cpu 717.98 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.36 present 7.14
  - stages: p95 ms: scale 0.10 plan 0.00 cpu 805.33 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.48 present 7.63
  - CPU raster by kind of work: medians ms: text 13.58 shadows 144.87 paths 59.98 rects 131.95 images 136.79 layers 123.38 composite 81.45 total 691.99; p95 ms: text 14.17 shadows 200.84 paths 64.18 rects 136.04 images 143.54 layers 130.02 composite 110.46 total 799.26
  - shadows blurred: 870 over 600 frames (1.45 a frame), 292.0 Mpx in all
  - CPU time: process CPU 788.17 ms per frame over the last 240 frames, every thread (106% of one core)
  - thermal: hottest zone 37.9 -> 37.0 °C (peak 39.5) over 447 s
  - input to present: input to present ms: median 731.6 p95 820.8 p99 870.5 worst 893.6 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 1061.65 ms, outside the renderer 3.55 ms, on cpu4; stages ms: scale 0.11 plan 0.00 cpu 1050.73 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 6.87
  - raster by core: cpu4 124 frames 719.69 ms | cpu5 51 frames 714.33 ms | cpu6 31 frames 715.80 ms | cpu7 25 frames 717.08 ms | cpu0 7 frames 708.01 ms | cpu1 2 frames 736.27 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 739.31031ms / p95 816.087916ms / worst 1.061649374s vs budget 16.666668ms · 240/240 over
- stress screen, GPU path (diagnostic): median 1.006057916s, worst 1.159976823s, budget 16.666668ms (240 samples)
  - memory: PSS 514 MB
  - start-up: before run 574 ms | app setup 0.2 | event loop started 0.0 | resumed 32.9 | window created 0.1 | present ready 78.0 | accessibility and show 0.0 | tree built 12.7 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 13.6 | first build and layout 10.9 | first raster and present 668.7 = 1391.2 ms from process start
  - stages: medians ms: scale 0.06 plan 97.15 cpu 0.00 gpu_upload 16.65 gpu_record 5.68 gpu_submit 14.19 gpu_wait 861.70 gpu_readback 0.00 acquire 0.51 present 0.44
  - stages: p95 ms: scale 0.13 plan 162.39 cpu 773.55 gpu_upload 44.99 gpu_record 8.33 gpu_submit 18.88 gpu_wait 906.19 gpu_readback 0.00 acquire 0.67 present 24.54
  - GPU time by kind of work: medians ms: draw 639595.95 layers 43977607152.94 filters 29320118252.64 shadows 19.46 blit 2.08 total 73298365023.07; p95 ms: draw 6953652.50 layers 43980313170.00 filters 43980551443.22 shadows 615324.10 blit 2.15 total 87968433591.98
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 0 partial and 495 full copies, 100.0% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 1057 over 600 frames (1.76 a frame), 356.3 Mpx in all
  - CPU time: process CPU 326.33 ms per frame over the last 240 frames, every thread (35% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.3) over 563 s
  - input to present: input to present ms: median 955.5 p95 995.4 p99 999.9 worst 999.9 over 23 inputs in 23 frames; 59 unanswered within 1 s
  - worst frame: work 1159.98 ms, outside the renderer 3.66 ms, on cpu4; stages ms: scale 0.10 plan 362.22 cpu 769.80 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 24.20
  - raster by core: cpu4 22 frames 773.55 ms | cpu5 17 frames 732.75 ms | cpu6 5 frames 763.37 ms | cpu7 5 frames 721.40 ms
  - clocks at report: cpu0 1.40/1.80 GHz | cpu1 1.40/1.80 GHz | cpu2 1.40/1.80 GHz | cpu3 1.40/1.80 GHz | cpu4 1.40/1.80 GHz | cpu5 1.40/1.80 GHz | cpu6 1.40/1.80 GHz | cpu7 1.40/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 915.012131ms / p95 1.060189792s / worst 1.159976823s vs budget 16.666668ms · 240/240 over
- stress screen for a minute on what auto chose (diagnostic): median 1.005753281s, worst 1.094323802s, budget 16.666668ms (240 samples)
  - memory: PSS 515 MB
  - renderer: gpu (calibrated earlier: cpu 18.84 ms, gpu 11.64 ms)
  - start-up: before run 578 ms | app setup 0.2 | event loop started 0.0 | resumed 34.8 | window created 0.0 | present ready 62.1 | accessibility and show 0.0 | tree built 13.6 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 14.2 | first build and layout 11.8 | first raster and present 668.0 = 1383.7 ms from process start
  - stages: medians ms: scale 0.06 plan 95.18 cpu 0.00 gpu_upload 16.57 gpu_record 5.48 gpu_submit 13.92 gpu_wait 864.19 gpu_readback 0.00 acquire 0.52 present 0.46
  - stages: p95 ms: scale 0.12 plan 165.54 cpu 772.91 gpu_upload 43.42 gpu_record 8.30 gpu_submit 18.50 gpu_wait 909.28 gpu_readback 0.00 acquire 0.70 present 24.46
  - GPU time by kind of work: medians ms: draw 614849.21 layers 43978660218.91 filters 29320120007.36 shadows 19.47 blit 2.09 total 73299395097.04; p95 ms: draw 3051022.10 layers 43980314386.94 filters 43980548532.66 shadows 316446.13 blit 2.17 total 87964230390.00
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 0 partial and 494 full copies, 100.0% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 1070 over 600 frames (1.78 a frame), 362.8 Mpx in all
  - CPU time: process CPU 315.91 ms per frame over the last 543 frames, every thread (34% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 563 s
  - over the run: frames 361-600: median 1005.75 ms, 240 of 240 over budget; last 240 of 600 frames: median 1005.75 ms, 240 over; 563 s
  - PSS over time: 10s:350 20s:456 30s:489 40s:478 50s:490 60s:490 70s:515 80s:497 90s:498 100s:529 110s:521 120s:507 130s:510 140s:534 150s:537 160s:515 170s:541 180s:509 190s:513 200s:511 210s:522 220s:508 230s:511 240s:535 250s:523 260s:543 270s:542 280s:508 290s:513 300s:502 310s:508 320s:514 330s:544 340s:508 350s:508 360s:535 370s:518 380s:507 390s:555 400s:517 410s:510 420s:512 430s:514 440s:533 450s:539 460s:517 470s:529 480s:544 490s:515 500s:538 510s:507 520s:511 530s:516 540s:517 MB (first to last: +166.6 MB)
  - input to present: input to present ms: median 901.0 p95 994.0 p99 999.8 worst 999.8 over 21 inputs in 21 frames; 61 unanswered within 1 s
  - worst frame: work 1094.32 ms, outside the renderer 3.24 ms; stages ms: scale 0.08 plan 132.94 cpu 0.00 gpu_upload 41.01 gpu_record 4.04 gpu_submit 14.68 gpu_wait 897.34 gpu_readback 0.00 acquire 0.51 present 0.48
  - raster by core: cpu4 27 frames 747.13 ms | cpu5 8 frames 771.45 ms | cpu6 4 frames 779.17 ms | cpu0 3 frames 727.35 ms | cpu7 2 frames 771.39 ms | cpu2 1 frames 717.26 ms
  - clocks at report: cpu0 1.04/1.80 GHz | cpu1 1.04/1.80 GHz | cpu2 1.04/1.80 GHz | cpu3 1.04/1.80 GHz | cpu4 1.04/1.80 GHz | cpu5 1.04/1.80 GHz | cpu6 1.04/1.80 GHz | cpu7 1.04/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 915.181133ms / p95 1.064023281s / worst 1.094323802s vs budget 16.666668ms · 240/240 over
- GPU path, every swapchain copy read back and compared with its frame: median 224.954844ms, worst 803.011458ms, budget 16.666668ms (240 samples)
  - memory: PSS 177 MB
  - start-up: before run 543 ms | app setup 0.2 | event loop started 0.0 | resumed 32.4 | window created 0.1 | present ready 58.8 | accessibility and show 0.0 | tree built 14.3 | window ready 0.9 | until first draw 0.1 | app before-frame hooks 18.2 | first build and layout 6.4 | first raster and present 42.9 = 717.0 ms from process start
  - stages: medians ms: scale 0.08 plan 0.79 cpu 0.00 gpu_upload 3.84 gpu_record 0.78 gpu_submit 1.15 gpu_wait 34.16 gpu_readback 0.00 acquire 0.40 present 0.63
  - stages: p95 ms: scale 0.10 plan 1.12 cpu 0.00 gpu_upload 4.34 gpu_record 0.94 gpu_submit 1.36 gpu_wait 38.94 gpu_readback 0.00 acquire 0.49 present 1.00
  - GPU time by kind of work: medians ms: draw 6.63 layers 0.00 filters 0.00 shadows 0.00 blit 30.09 total 36.72; p95 ms: draw 8.05 layers 0.00 filters 0.00 shadows 0.00 blit 34.54 total 42.59
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 589 partial and 11 full copies, 7.5% of the frames' pixels copied; 594 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 256.29 ms per frame over the last 240 frames, every thread (87% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 177 s
  - input to present: input to present ms: median 226.3 p95 792.5 p99 806.1 worst 812.5 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 803.01 ms, outside the renderer 755.10 ms; stages ms: scale 0.07 plan 0.58 cpu 0.00 gpu_upload 3.75 gpu_record 0.77 gpu_submit 1.12 gpu_wait 40.18 gpu_readback 0.00 acquire 0.40 present 1.06
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 292.096321ms / p95 787.545885ms / worst 803.011458ms vs budget 16.666668ms · 240/240 over
- GPU path copying whole frames into the swapchain (diagnostic A/B): median 19.712812ms, worst 26.731146ms, budget 16.666668ms (240 samples)
  - memory: PSS 127 MB
  - start-up: before run 536 ms | app setup 0.2 | event loop started 0.0 | resumed 34.7 | window created 0.1 | present ready 71.9 | accessibility and show 0.0 | tree built 14.9 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 14.1 | first build and layout 5.4 | first raster and present 42.9 = 721.0 ms from process start
  - stages: medians ms: scale 0.09 plan 0.88 cpu 0.00 gpu_upload 0.99 gpu_record 0.67 gpu_submit 1.17 gpu_wait 13.30 gpu_readback 0.00 acquire 0.33 present 0.56
  - stages: p95 ms: scale 0.20 plan 1.92 cpu 0.00 gpu_upload 1.75 gpu_record 1.25 gpu_submit 1.99 gpu_wait 14.17 gpu_readback 0.00 acquire 0.50 present 0.85
  - GPU time by kind of work: medians ms: draw 13.18 layers 0.00 filters 0.00 shadows 0.00 blit 2.95 total 16.13; p95 ms: draw 14.17 layers 0.00 filters 0.00 shadows 0.00 blit 3.01 total 17.17
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.46 ms per frame over the last 240 frames, every thread (32% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 12 s
  - input to present: input to present ms: median 21.4 p95 24.8 p99 26.9 worst 26.9 over 14 inputs in 14 frames; 0 unanswered within 1 s
  - worst frame: work 26.73 ms, outside the renderer 1.80 ms; stages ms: scale 0.08 plan 0.71 cpu 0.00 gpu_upload 2.51 gpu_record 0.72 gpu_submit 1.52 gpu_wait 18.58 gpu_readback 0.00 acquire 0.33 present 0.49
  - clocks at report: cpu0 0.65/1.80 GHz | cpu1 0.65/1.80 GHz | cpu2 0.65/1.80 GHz | cpu3 0.65/1.80 GHz | cpu4 0.65/1.80 GHz | cpu5 0.65/1.80 GHz | cpu6 0.65/1.80 GHz | cpu7 0.65/1.80 GHz
  - pipeline cache: started from 37671 bytes; saved 37671 bytes after the first GPU frame
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 19.749404ms / p95 22.876719ms / worst 26.731146ms vs budget 16.666668ms · 232/240 over
- failed checks: frames_inside_their_budget the_typical_frame_is_inside_its_budget frames_inside_their_budget frames_inside_their_budget

**Samsung Galaxy A73 5G**
- gating passes: auto cpu gpu (auto chose: gpu)
- CPU path: median 7.274115ms, worst 23.485312ms, budget 16.666668ms (240 samples)
  - **passed despite a known-failures.txt entry**: listed in known-failures.txt but passed: delete the entry
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 80 MB
  - start-up: before run 266 ms | app setup 0.1 | event loop started 0.0 | resumed 18.9 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.3 | window ready 4.9 | until first draw 0.0 | app before-frame hooks 7.9 | first build and layout 2.0 | first raster and present 52.3 = 356.7 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 5.92 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.10 present 0.97
  - stages: p95 ms: scale 0.02 plan 0.00 cpu 7.69 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.14 present 1.33
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.14 rects 4.78 images 0.00 layers 0.00 composite 0.00 total 4.92; p95 ms: text 0.00 shadows 0.00 paths 0.19 rects 6.07 images 0.00 layers 0.00 composite 0.00 total 6.26
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 8.46 ms per frame over the last 240 frames, every thread (94% of one core)
  - thermal: status none -> none (worst none); headroom 0.59 -> 0.59 (highest 0.59; 1.0 is severe throttling); hottest zone 46.7 -> 45.9 °C (peak 46.7) over 5 s
  - input to present: input to present ms: median 10.4 p95 12.8 p99 12.8 worst 12.8 over 10 inputs in 10 frames; 0 unanswered within 1 s
  - worst frame: work 23.49 ms, outside the renderer 0.76 ms, on cpu5; stages ms: scale 0.02 plan 0.00 cpu 19.70 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.15 present 2.86
  - raster by core: cpu6 96 frames 5.80 ms | cpu5 74 frames 5.96 ms | cpu7 66 frames 5.98 ms | cpu4 4 frames 8.36 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 2.40/2.40 GHz | cpu5 2.40/2.40 GHz | cpu6 2.40/2.40 GHz | cpu7 2.40/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 7.590547ms / p95 9.385468ms / worst 23.485312ms vs budget 16.666668ms · 1/240 over
- GPU path: median 2.379376ms, worst 6.159427ms, budget 8.333333ms (240 samples)
  - memory: PSS 101 MB
  - start-up: before run 273 ms | app setup 0.1 | event loop started 0.0 | resumed 17.9 | window created 0.1 | present ready 34.8 | accessibility and show 0.0 | tree built 3.8 | window ready 0.7 | until first draw 0.0 | app before-frame hooks 248.2 | first build and layout 2.0 | first raster and present 28.6 = 609.1 ms from process start
  - stages: medians ms: scale 0.05 plan 0.34 cpu 0.00 gpu_upload 0.56 gpu_record 0.14 gpu_submit 0.27 gpu_wait 0.02 gpu_readback 0.00 acquire 0.04 present 0.07
  - stages: p95 ms: scale 0.05 plan 0.45 cpu 0.00 gpu_upload 0.79 gpu_record 0.22 gpu_submit 0.34 gpu_wait 2.85 gpu_readback 0.00 acquire 0.04 present 0.80
  - GPU time by kind of work: medians ms: draw 0.17 layers 0.00 filters 0.00 shadows 0.00 blit 0.06 total 0.23; p95 ms: draw 0.29 layers 0.00 filters 0.00 shadows 0.00 blit 0.08 total 0.37
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 588 partial and 12 full copies, 5.7% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 2.00 ms per frame over the last 240 frames, every thread (74% of one core)
  - thermal: status none -> none (worst none); headroom 0.61 -> 0.61 (highest 0.61; 1.0 is severe throttling); hottest zone 45.9 -> 40.1 °C (peak 45.9) over 2 s
  - input to present: no input reached this window
  - worst frame: work 6.16 ms, outside the renderer 0.37 ms; stages ms: scale 0.03 plan 0.22 cpu 0.00 gpu_upload 0.74 gpu_record 0.11 gpu_submit 1.06 gpu_wait 2.86 gpu_readback 0.00 acquire 0.03 present 0.73
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 1.23/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 2.63588ms / p95 4.322187ms / worst 6.159427ms vs budget 8.333333ms · 0/240 over
- chosen by measurement (auto): median 1.417188ms, worst 3.556771ms, budget 8.333333ms (240 samples)
  - memory: PSS 99 MB
  - renderer: gpu (calibrated earlier: cpu 6.94 ms, gpu 0.97 ms)
  - calibration launches: launch 1: 10-29 04:42:12.516  5986  6010 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer cpu, measuring (first launch with this build here) 10-29 04:42:13.203  5986  6010 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer cpu for now; the gpu is measured next launch (measured: cpu 6.94 ms; budget 8.33 ms) launch 2: 10-29 04:42:21.509  6071  6096 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu, measuring (the cpu path measured 6.94 ms last launch) 10-29 04:42:21.976  6071  6096 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu (measured: cpu 6.94 ms, gpu 0.97 ms; budget 8.33 ms) launch 3: 10-29 04:42:30.426  6149  6172 I vieww   : vieww_platform_winit::rasterizer_choice: vieww: rasterizer gpu (calibrated earlier: cpu 6.94 ms, gpu 0.97 ms)
  - start-up: before run 281 ms | app setup 0.1 | event loop started 0.0 | resumed 20.0 | window created 0.0 | present ready 36.5 | accessibility and show 0.0 | tree built 4.1 | window ready 0.8 | until first draw 0.0 | app before-frame hooks 32.7 | first build and layout 1.9 | first raster and present 40.5 = 417.4 ms from process start
  - stages: medians ms: scale 0.02 plan 0.21 cpu 0.00 gpu_upload 0.47 gpu_record 0.08 gpu_submit 0.07 gpu_wait 0.01 gpu_readback 0.00 acquire 0.02 present 0.05
  - stages: p95 ms: scale 0.03 plan 0.33 cpu 0.00 gpu_upload 0.61 gpu_record 0.12 gpu_submit 0.29 gpu_wait 0.94 gpu_readback 0.00 acquire 0.03 present 0.30
  - GPU time by kind of work: medians ms: draw 0.12 layers 0.00 filters 0.00 shadows 0.00 blit 0.03 total 0.16; p95 ms: draw 0.25 layers 0.00 filters 0.00 shadows 0.00 blit 0.06 total 0.31
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 588 partial and 12 full copies, 4.9% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 1.29 ms per frame over the last 240 frames, every thread (82% of one core)
  - thermal: status none -> none (worst none); headroom 0.63 -> 0.63 (highest 0.63; 1.0 is severe throttling); hottest zone 44.0 -> 41.3 °C (peak 44.0) over 1 s
  - input to present: no input reached this window
  - worst frame: work 3.56 ms, outside the renderer 0.21 ms; stages ms: scale 0.02 plan 0.17 cpu 0.00 gpu_upload 0.29 gpu_record 0.07 gpu_submit 0.06 gpu_wait 2.65 gpu_readback 0.00 acquire 0.03 present 0.06
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.52/2.40 GHz | cpu5 1.52/2.40 GHz | cpu6 1.52/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 1.487581ms / p95 2.319687ms / worst 3.556771ms vs budget 8.333333ms · 0/240 over
- CPU path presented through Vulkan (diagnostic): median 10.619531ms, worst 23.970312ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 143 MB
  - start-up: before run 270 ms | app setup 0.1 | event loop started 0.0 | resumed 18.2 | window created 0.0 | present ready 34.6 | accessibility and show 0.0 | tree built 4.0 | window ready 0.8 | until first draw 0.0 | app before-frame hooks 34.0 | first build and layout 1.9 | first raster and present 52.4 = 416.5 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 6.16 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 4.04
  - stages: p95 ms: scale 0.05 plan 0.00 cpu 9.32 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 4.74
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.17 rects 4.92 images 0.00 layers 0.00 composite 0.00 total 5.09; p95 ms: text 0.00 shadows 0.00 paths 0.35 rects 6.86 images 0.00 layers 0.00 composite 0.00 total 7.20
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 10.38 ms per frame over the last 240 frames, every thread (62% of one core)
  - thermal: status none -> none (worst none); headroom 0.64 -> 0.64 (highest 0.64; 1.0 is severe throttling); hottest zone 44.8 -> 42.8 °C (peak 52.5) over 9 s
  - input to present: input to present ms: median 17.4 p95 19.3 p99 25.5 worst 25.5 over 28 inputs in 19 frames; 0 unanswered within 1 s
  - worst frame: work 23.97 ms, outside the renderer 0.74 ms, on cpu7; stages ms: scale 0.02 plan 0.00 cpu 18.12 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 5.10
  - raster by core: cpu5 128 frames 6.61 ms | cpu6 81 frames 5.75 ms | cpu4 19 frames 6.52 ms | cpu7 11 frames 6.78 ms | cpu3 1 frames 11.52 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 11.298934ms / p95 14.387864ms / worst 23.970312ms vs budget 16.666668ms · 3/240 over
- CPU path with an Android performance hint (diagnostic): median 7.262343ms, worst 23.625208ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 81 MB
  - start-up: before run 286 ms | app setup 0.1 | event loop started 0.0 | resumed 18.6 | window created 0.0 | present ready 0.7 | accessibility and show 0.0 | tree built 4.4 | window ready 1.7 | until first draw 0.0 | app before-frame hooks 18.5 | first build and layout 2.1 | first raster and present 52.6 = 384.3 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 6.01 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.09 present 0.89
  - stages: p95 ms: scale 0.02 plan 0.00 cpu 8.17 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.12 present 1.24
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.14 rects 4.87 images 0.00 layers 0.00 composite 0.00 total 5.01; p95 ms: text 0.00 shadows 0.00 paths 0.20 rects 6.30 images 0.00 layers 0.00 composite 0.00 total 6.50
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 8.83 ms per frame over the last 240 frames, every thread (96% of one core)
  - thermal: status none -> none (worst none); headroom 0.64 -> 0.64 (highest 0.64; 1.0 is severe throttling); hottest zone 52.9 -> 49.4 °C (peak 52.9) over 5 s
  - input to present: input to present ms: median 10.3 p95 17.3 p99 20.4 worst 20.4 over 12 inputs in 11 frames; 0 unanswered within 1 s
  - worst frame: work 23.63 ms, outside the renderer 0.90 ms, on cpu7; stages ms: scale 0.02 plan 0.00 cpu 20.04 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.13 present 2.54
  - raster by core: cpu7 92 frames 6.11 ms | cpu6 81 frames 5.92 ms | cpu5 65 frames 6.02 ms | cpu4 2 frames 7.53 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - performance hint: unavailable (the system declined a session)
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 7.730583ms / p95 9.758334ms / worst 23.625208ms vs budget 16.666668ms · 2/240 over
- GPU path at the device tier's resolution (diagnostic): median 2.4ms, worst 5.947603ms, budget 8.333333ms (240 samples)
  - memory: PSS 80 MB
  - start-up: before run 282 ms | app setup 0.2 | event loop started 0.0 | resumed 20.7 | window created 0.0 | present ready 32.5 | accessibility and show 0.0 | tree built 3.9 | window ready 0.9 | until first draw 0.0 | app before-frame hooks 5.7 | first build and layout 2.2 | first raster and present 23.8 = 372.1 ms from process start
  - stages: medians ms: scale 0.05 plan 0.34 cpu 0.00 gpu_upload 0.55 gpu_record 0.13 gpu_submit 0.16 gpu_wait 0.02 gpu_readback 0.00 acquire 0.04 present 0.08
  - stages: p95 ms: scale 0.05 plan 0.42 cpu 0.00 gpu_upload 0.71 gpu_record 0.15 gpu_submit 0.35 gpu_wait 2.74 gpu_readback 0.00 acquire 0.05 present 0.36
  - GPU time by kind of work: medians ms: draw 0.11 layers 0.00 filters 0.00 shadows 0.00 blit 0.82 total 0.93; p95 ms: draw 0.18 layers 0.00 filters 0.00 shadows 0.00 blit 0.85 total 1.03
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: probing: 0 partial and 600 full copies, 100.0% of the frames' pixels copied; 0 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 2.12 ms per frame over the last 240 frames, every thread (75% of one core)
  - thermal: status none -> none (worst none); headroom 0.65 -> 0.65 (highest 0.65; 1.0 is severe throttling); hottest zone 47.1 -> 41.3 °C (peak 47.1) over 2 s
  - input to present: no input reached this window
  - worst frame: work 5.95 ms, outside the renderer 0.36 ms; stages ms: scale 0.03 plan 0.66 cpu 0.00 gpu_upload 1.77 gpu_record 0.14 gpu_submit 0.31 gpu_wait 2.21 gpu_readback 0.00 acquire 0.04 present 0.42
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.23/2.40 GHz | cpu5 1.23/2.40 GHz | cpu6 1.23/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - gpu resolution: GPU path renders at 0.75x resolution (VIEWW_GPU_SCALE=auto: tier medium)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 2.749535ms / p95 4.499949ms / worst 5.947603ms vs budget 8.333333ms · 0/240 over · GPU path renders at 0.75x resolution (VIEWW_GPU_SCALE=auto: tier medium)
- GPU path waiting for every frame, without the overlap (diagnostic A/B): median 2.391771ms, worst 4.483021ms, budget 8.333333ms (240 samples)
  - memory: PSS 100 MB
  - start-up: before run 272 ms | app setup 0.1 | event loop started 0.0 | resumed 18.0 | window created 0.0 | present ready 32.5 | accessibility and show 0.0 | tree built 4.0 | window ready 1.8 | until first draw 0.0 | app before-frame hooks 6.1 | first build and layout 2.0 | first raster and present 65.0 = 401.3 ms from process start
  - stages: medians ms: scale 0.05 plan 0.33 cpu 0.00 gpu_upload 0.55 gpu_record 0.14 gpu_submit 0.29 gpu_wait 0.37 gpu_readback 0.00 acquire 0.03 present 0.04
  - stages: p95 ms: scale 0.05 plan 0.44 cpu 0.00 gpu_upload 0.66 gpu_record 0.21 gpu_submit 0.33 gpu_wait 0.84 gpu_readback 0.00 acquire 0.04 present 0.42
  - GPU time by kind of work: medians ms: draw 0.13 layers 0.00 filters 0.00 shadows 0.00 blit 0.05 total 0.18; p95 ms: draw 0.25 layers 0.00 filters 0.00 shadows 0.00 blit 0.07 total 0.32
  - CPU/GPU overlap: off: every GPU frame waited for before the next (VIEWW_GPU_OVERLAP=0)
  - copies into the swapchain: trusted: 588 partial and 12 full copies, 5.5% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 2.00 ms per frame over the last 240 frames, every thread (81% of one core)
  - thermal: status none -> none (worst none); headroom 0.66 -> 0.66 (highest 0.66; 1.0 is severe throttling); hottest zone 47.8 -> 42.0 °C (peak 47.8) over 2 s
  - input to present: no input reached this window
  - worst frame: work 4.48 ms, outside the renderer 2.65 ms; stages ms: scale 0.05 plan 0.33 cpu 0.00 gpu_upload 0.60 gpu_record 0.14 gpu_submit 0.30 gpu_wait 0.36 gpu_readback 0.00 acquire 0.02 present 0.04
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.90/2.40 GHz | cpu5 1.90/2.40 GHz | cpu6 1.90/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 2.403397ms / p95 2.977865ms / worst 4.483021ms vs budget 8.333333ms · 0/240 over
- CPU path without raster threads, fill cache or blur cache (diagnostic A/B): median 8.781354ms, worst 23.48401ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 81 MB
  - start-up: before run 274 ms | app setup 0.1 | event loop started 0.0 | resumed 18.0 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.0 | window ready 2.6 | until first draw 0.0 | app before-frame hooks 21.1 | first build and layout 1.8 | first raster and present 47.7 = 370.3 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 6.92 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.12 present 1.12
  - stages: p95 ms: scale 0.05 plan 0.00 cpu 9.74 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.21 present 1.87
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.15 rects 5.50 images 0.00 layers 0.00 composite 0.00 total 5.65; p95 ms: text 0.00 shadows 0.00 paths 0.49 rects 7.15 images 0.00 layers 0.00 composite 0.00 total 7.64
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 10.12 ms per frame over the last 240 frames, every thread (76% of one core)
  - thermal: status none -> none (worst none); headroom 0.67 -> 0.67 (highest 0.67; 1.0 is severe throttling); hottest zone 47.1 -> 43.2 °C (peak 47.1) over 7 s
  - input to present: input to present ms: median 14.3 p95 21.8 p99 27.9 worst 27.9 over 18 inputs in 15 frames; 0 unanswered within 1 s
  - worst frame: work 23.48 ms, outside the renderer 0.92 ms, on cpu7; stages ms: scale 0.02 plan 0.00 cpu 19.85 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.11 present 2.59
  - raster by core: cpu5 84 frames 8.59 ms | cpu6 74 frames 6.16 ms | cpu7 66 frames 6.12 ms | cpu4 16 frames 8.52 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier medium · median 9.322607ms / p95 12.305885ms / worst 23.48401ms vs budget 16.666668ms · 2/240 over
- CPU path without the row-parallel walk only (diagnostic A/B): median 7.377188ms, worst 26.807344ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 80 MB
  - start-up: before run 273 ms | app setup 0.1 | event loop started 0.0 | resumed 18.6 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.1 | window ready 0.7 | until first draw 0.0 | app before-frame hooks 18.5 | first build and layout 4.0 | first raster and present 49.3 = 369.1 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 6.00 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.10 present 1.01
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.10 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.17 present 1.69
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.13 rects 4.88 images 0.00 layers 0.00 composite 0.00 total 5.00; p95 ms: text 0.00 shadows 0.00 paths 0.25 rects 7.14 images 0.00 layers 0.00 composite 0.00 total 7.39
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 9.04 ms per frame over the last 240 frames, every thread (94% of one core)
  - thermal: status none -> none (worst none); headroom 0.67 -> 0.67 (highest 0.67; 1.0 is severe throttling); hottest zone 49.8 -> 49.0 °C (peak 49.8) over 6 s
  - input to present: input to present ms: median 10.3 p95 22.1 p99 28.6 worst 28.6 over 14 inputs in 13 frames; 0 unanswered within 1 s
  - worst frame: work 26.81 ms, outside the renderer 0.93 ms, on cpu7; stages ms: scale 0.02 plan 0.00 cpu 22.85 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.14 present 2.87
  - raster by core: cpu7 126 frames 6.15 ms | cpu5 91 frames 5.77 ms | cpu6 20 frames 5.92 ms | cpu4 3 frames 9.38 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier medium · median 8.034017ms / p95 11.295208ms / worst 26.807344ms vs budget 16.666668ms · 2/240 over
- CPU path without the path-fill cache only (diagnostic A/B): median 7.169636ms, worst 24.272083ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 80 MB
  - start-up: before run 269 ms | app setup 0.1 | event loop started 0.0 | resumed 18.6 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.3 | window ready 10.6 | until first draw 0.0 | app before-frame hooks 260.8 | first build and layout 2.0 | first raster and present 42.1 = 607.6 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 5.80 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.10 present 1.00
  - stages: p95 ms: scale 0.04 plan 0.00 cpu 9.22 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.17 present 1.74
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 4.68 images 0.00 layers 0.00 composite 0.00 total 4.80; p95 ms: text 0.00 shadows 0.00 paths 0.27 rects 7.31 images 0.00 layers 0.00 composite 0.00 total 7.58
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 8.62 ms per frame over the last 240 frames, every thread (80% of one core)
  - thermal: status none -> none (worst none); headroom 0.67 -> 0.68 (highest 0.68; 1.0 is severe throttling); hottest zone 47.1 -> 43.6 °C (peak 47.1) over 6 s
  - input to present: input to present ms: median 10.8 p95 21.1 p99 33.6 worst 33.6 over 16 inputs in 15 frames; 0 unanswered within 1 s
  - worst frame: work 24.27 ms, outside the renderer 0.95 ms, on cpu4; stages ms: scale 0.02 plan 0.00 cpu 19.67 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.20 present 3.44
  - raster by core: cpu6 80 frames 5.63 ms | cpu7 75 frames 6.24 ms | cpu5 54 frames 5.99 ms | cpu4 30 frames 5.66 ms | cpu1 1 frames 6.79 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (tier medium)
  - quality overlay: tier medium · median 8.026861ms / p95 12.000469ms / worst 24.272083ms vs budget 16.666668ms · 2/240 over
- CPU path without the blur cache only (diagnostic A/B): median 8.563802ms, worst 25.779479ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 81 MB
  - start-up: before run 266 ms | app setup 0.1 | event loop started 0.0 | resumed 18.0 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.1 | window ready 5.4 | until first draw 0.1 | app before-frame hooks 11.3 | first build and layout 1.6 | first raster and present 50.5 = 357.6 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.11 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.12 present 1.03
  - stages: p95 ms: scale 0.05 plan 0.00 cpu 9.87 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.23 present 1.69
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.16 rects 5.66 images 0.00 layers 0.00 composite 0.00 total 5.82; p95 ms: text 0.00 shadows 0.00 paths 0.48 rects 7.14 images 0.00 layers 0.00 composite 0.00 total 7.62
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 9.92 ms per frame over the last 240 frames, every thread (66% of one core)
  - thermal: status none -> none (worst none); headroom 0.68 -> 0.68 (highest 0.68; 1.0 is severe throttling); hottest zone 48.2 -> 43.2 °C (peak 57.9) over 7 s
  - input to present: input to present ms: median 14.5 p95 22.4 p99 27.1 worst 27.1 over 18 inputs in 14 frames; 0 unanswered within 1 s
  - worst frame: work 25.78 ms, outside the renderer 0.78 ms, on cpu7; stages ms: scale 0.02 plan 0.00 cpu 22.51 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.12 present 2.35
  - raster by core: cpu6 106 frames 7.93 ms | cpu5 79 frames 7.05 ms | cpu7 35 frames 6.31 ms | cpu4 19 frames 7.23 ms | cpu0 1 frames 10.54 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 1.15/1.80 GHz | cpu2 1.15/1.80 GHz | cpu3 1.15/1.80 GHz | cpu4 1.52/2.40 GHz | cpu5 1.52/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 0.81/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 9.18681ms / p95 11.991562ms / worst 25.779479ms vs budget 16.666668ms · 2/240 over
- stress screen, CPU path (diagnostic): median 146.507812ms, worst 222.827031ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 306 MB
  - start-up: before run 270 ms | app setup 0.1 | event loop started 0.0 | resumed 18.0 | window created 0.0 | present ready 0.6 | accessibility and show 0.0 | tree built 4.2 | window ready 2.0 | until first draw 0.0 | app before-frame hooks 38.5 | first build and layout 3.1 | first raster and present 236.5 = 572.8 ms from process start
  - stages: medians ms: scale 0.01 plan 0.00 cpu 142.04 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.11 present 3.55
  - stages: p95 ms: scale 0.02 plan 0.00 cpu 172.17 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.13 present 4.73
  - CPU raster by kind of work: medians ms: text 3.06 shadows 22.17 paths 12.57 rects 25.79 images 22.95 layers 25.78 composite 20.25 total 132.56; p95 ms: text 3.58 shadows 33.78 paths 13.76 rects 27.89 images 24.48 layers 36.64 composite 23.94 total 164.07
  - shadows blurred: 880 over 600 frames (1.47 a frame), 320.6 Mpx in all
  - CPU time: process CPU 150.79 ms per frame over the last 240 frames, every thread (95% of one core)
  - thermal: status none -> none (worst none); headroom 0.68 -> 0.72 (highest 0.72; 1.0 is severe throttling); hottest zone 48.2 -> 47.8 °C (peak 56.0) over 95 s
  - input to present: input to present ms: median 151.6 p95 200.7 p99 467.3 worst 503.2 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 222.83 ms, outside the renderer 0.57 ms, on cpu7; stages ms: scale 0.01 plan 0.00 cpu 218.25 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.10 present 3.90
  - raster by core: cpu7 239 frames 142.01 ms | cpu6 1 frames 153.87 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.23/2.40 GHz | cpu5 1.23/2.40 GHz | cpu6 1.23/2.40 GHz | cpu7 2.40/2.40 GHz
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 151.20075ms / p95 175.316354ms / worst 222.827031ms vs budget 16.666668ms · 240/240 over
- stress screen, GPU path (diagnostic): median 150.794219ms, worst 209.704843ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 438 MB
  - start-up: before run 265 ms | app setup 0.1 | event loop started 0.0 | resumed 17.8 | window created 0.0 | present ready 51.9 | accessibility and show 0.0 | tree built 4.0 | window ready 1.0 | until first draw 0.0 | app before-frame hooks 21.3 | first build and layout 3.0 | first raster and present 133.4 = 497.8 ms from process start
  - stages: medians ms: scale 0.02 plan 17.07 cpu 0.00 gpu_upload 3.62 gpu_record 0.31 gpu_submit 0.37 gpu_wait 114.92 gpu_readback 0.00 acquire 0.06 present 2.12
  - stages: p95 ms: scale 0.04 plan 29.54 cpu 161.11 gpu_upload 13.86 gpu_record 0.68 gpu_submit 0.60 gpu_wait 155.81 gpu_readback 0.00 acquire 0.14 present 9.80
  - GPU time by kind of work: medians ms: draw 4.63 layers 5.00 filters 131.80 shadows 1.76 blit 0.71 total 143.90; p95 ms: draw 7.43 layers 7.21 filters 159.73 shadows 2.64 blit 1.11 total 178.12
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 0 partial and 457 full copies, 100.0% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 1165 over 600 frames (1.94 a frame), 441.0 Mpx in all
  - CPU time: process CPU 66.92 ms per frame over the last 240 frames, every thread (44% of one core)
  - thermal: status none -> none (worst none); headroom 0.73 -> 0.73 (highest 0.74; 1.0 is severe throttling); hottest zone 50.9 -> 47.3 °C (peak 57.9) over 95 s
  - input to present: input to present ms: median 153.7 p95 249.7 p99 308.2 worst 516.3 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 209.70 ms, outside the renderer 1.33 ms, on cpu7; stages ms: scale 0.03 plan 25.17 cpu 177.06 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 6.11
  - raster by core: cpu7 60 frames 154.54 ms | cpu5 1 frames 140.78 ms | cpu6 1 frames 143.03 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 2.40/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 143.11412ms / p95 191.916615ms / worst 209.704843ms vs budget 16.666668ms · 240/240 over
- stress screen for a minute on what auto chose (diagnostic): median 150.599324ms, worst 234.124166ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 401 MB
  - renderer: gpu (calibrated earlier: cpu 6.94 ms, gpu 0.97 ms)
  - start-up: before run 293 ms | app setup 0.1 | event loop started 0.0 | resumed 18.8 | window created 0.0 | present ready 48.2 | accessibility and show 0.0 | tree built 3.7 | window ready 0.6 | until first draw 0.0 | app before-frame hooks 9.1 | first build and layout 3.7 | first raster and present 126.0 = 503.7 ms from process start
  - stages: medians ms: scale 0.02 plan 18.94 cpu 0.00 gpu_upload 3.62 gpu_record 0.40 gpu_submit 0.44 gpu_wait 115.48 gpu_readback 0.00 acquire 0.08 present 2.30
  - stages: p95 ms: scale 0.04 plan 34.78 cpu 164.28 gpu_upload 14.91 gpu_record 0.70 gpu_submit 0.63 gpu_wait 154.73 gpu_readback 0.00 acquire 0.14 present 9.66
  - GPU time by kind of work: medians ms: draw 4.61 layers 4.98 filters 131.83 shadows 1.73 blit 0.70 total 143.85; p95 ms: draw 7.41 layers 6.96 filters 159.71 shadows 2.53 blit 1.16 total 177.78
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 0 partial and 464 full copies, 100.0% of the frames' pixels copied; 6 image(s) kept their contents, 0 did not
  - shadows blurred: 1154 over 600 frames (1.92 a frame), 436.4 Mpx in all
  - CPU time: process CPU 65.76 ms per frame over the last 243 frames, every thread (42% of one core)
  - thermal: status none -> none (worst none); headroom 0.73 -> 0.74 (highest 0.74; 1.0 is severe throttling); hottest zone 50.9 -> 46.9 °C (peak 53.2) over 93 s
  - over the run: frames 361-600: median 150.60 ms, 240 of 240 over budget; last 240 of 600 frames: median 150.60 ms, 240 over; 93 s
  - PSS over time: 10s:448 20s:408 30s:429 40s:356 50s:427 60s:359 70s:406 80s:339 90s:436 MB (first to last: -11.2 MB)
  - input to present: input to present ms: median 154.0 p95 206.0 p99 232.2 worst 251.7 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 234.12 ms, outside the renderer 1.84 ms, on cpu7; stages ms: scale 0.04 plan 32.99 cpu 189.92 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 9.35
  - raster by core: cpu7 54 frames 155.79 ms | cpu6 1 frames 147.09 ms
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.65/2.40 GHz | cpu5 1.65/2.40 GHz | cpu6 1.65/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 7 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 145.450722ms / p95 198.894428ms / worst 234.124166ms vs budget 16.666668ms · 240/240 over
- GPU path, every swapchain copy read back and compared with its frame: median 37.477187ms, worst 144.187968ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 157 MB
  - start-up: before run 279 ms | app setup 0.1 | event loop started 0.0 | resumed 19.5 | window created 0.0 | present ready 46.1 | accessibility and show 0.0 | tree built 3.6 | window ready 0.7 | until first draw 0.0 | app before-frame hooks 6.5 | first build and layout 2.1 | first raster and present 33.2 = 391.2 ms from process start
  - stages: medians ms: scale 0.02 plan 0.25 cpu 0.00 gpu_upload 1.34 gpu_record 0.13 gpu_submit 0.26 gpu_wait 2.07 gpu_readback 0.00 acquire 0.07 present 1.86
  - stages: p95 ms: scale 0.02 plan 0.47 cpu 0.00 gpu_upload 4.65 gpu_record 0.17 gpu_submit 0.30 gpu_wait 4.36 gpu_readback 0.00 acquire 0.09 present 2.81
  - GPU time by kind of work: medians ms: draw 0.38 layers 0.00 filters 0.00 shadows 0.00 blit 3.38 total 3.77; p95 ms: draw 0.44 layers 0.00 filters 0.00 shadows 0.00 blit 5.82 total 6.26
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - copies into the swapchain: trusted: 588 partial and 12 full copies, 9.5% of the frames' pixels copied; 593 image(s) kept their contents, 0 did not
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 48.38 ms per frame over the last 240 frames, every thread (86% of one core)
  - thermal: status none -> none (worst none); headroom 0.74 -> 0.74 (highest 0.74; 1.0 is severe throttling); hottest zone 50.2 -> 41.3 °C (peak 50.2) over 33 s
  - input to present: input to present ms: median 36.2 p95 136.0 p99 143.9 worst 151.5 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 144.19 ms, outside the renderer 131.49 ms; stages ms: scale 0.02 plan 0.19 cpu 0.00 gpu_upload 4.51 gpu_record 0.14 gpu_submit 0.27 gpu_wait 5.57 gpu_readback 0.00 acquire 0.06 present 1.93
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 0.69/2.40 GHz | cpu5 0.69/2.40 GHz | cpu6 0.69/2.40 GHz | cpu7 2.40/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 49.627521ms / p95 136.405313ms / worst 144.187968ms vs budget 16.666668ms · 240/240 over
- GPU path copying whole frames into the swapchain (diagnostic A/B): median 1.523958ms, worst 3.925156ms, budget 8.333333ms (240 samples)
  - memory: PSS 79 MB
  - start-up: before run 271 ms | app setup 0.1 | event loop started 0.0 | resumed 18.2 | window created 0.0 | present ready 34.0 | accessibility and show 0.0 | tree built 4.0 | window ready 0.9 | until first draw 0.1 | app before-frame hooks 32.2 | first build and layout 2.0 | first raster and present 28.7 = 391.4 ms from process start
  - stages: medians ms: scale 0.02 plan 0.25 cpu 0.00 gpu_upload 0.23 gpu_record 0.07 gpu_submit 0.06 gpu_wait 0.42 gpu_readback 0.00 acquire 0.02 present 0.04
  - stages: p95 ms: scale 0.03 plan 0.33 cpu 0.00 gpu_upload 0.59 gpu_record 0.10 gpu_submit 0.28 gpu_wait 2.01 gpu_readback 0.00 acquire 0.03 present 0.33
  - GPU time by kind of work: medians ms: draw 0.14 layers 0.00 filters 0.00 shadows 0.00 blit 0.78 total 0.92; p95 ms: draw 0.26 layers 0.00 filters 0.00 shadows 0.00 blit 0.86 total 1.13
  - CPU/GPU overlap: on: the app builds the next frame while the GPU draws this one
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 1.17 ms per frame over the last 240 frames, every thread (63% of one core)
  - thermal: status none -> none (worst none); headroom 0.74 -> 0.74 (highest 0.74; 1.0 is severe throttling); hottest zone 47.8 -> 44.4 °C (peak 47.8) over 1 s
  - input to present: no input reached this window
  - worst frame: work 3.93 ms, outside the renderer 0.19 ms; stages ms: scale 0.01 plan 0.17 cpu 0.00 gpu_upload 0.21 gpu_record 0.06 gpu_submit 0.06 gpu_wait 3.16 gpu_readback 0.00 acquire 0.02 present 0.04
  - clocks at report: cpu0 0.69/1.80 GHz | cpu1 0.69/1.80 GHz | cpu2 0.69/1.80 GHz | cpu3 0.69/1.80 GHz | cpu4 1.23/2.40 GHz | cpu5 1.23/2.40 GHz | cpu6 1.23/2.40 GHz | cpu7 0.81/2.40 GHz
  - pipeline cache: started from 37459 bytes; saved 37459 bytes after the first GPU frame
  - device profile: tier medium, 7.1 GB, 8 cores, SM7325 (lahaina) (API 31)
  - cache budgets: blur off (tier medium), fill off (tier medium), raster threads serial (tier medium)
  - quality overlay: tier medium · median 1.772727ms / p95 2.997292ms / worst 3.925156ms vs budget 8.333333ms · 0/240 over

## Audio: focus, interruptions, background playback

**LG Stylo 5**: PASS
- foreground_service: PASS isForeground=true
- in_app: PASS PASS; failed:
- media_session_playing: PASS state=3
- plays_with_screen_off: PASS 61 s of audio callbacks over 61 s with the screen off; after: status t_ms=79786 callbacks=3723776 rate=48000 position_ms=11546 playing=true suspended=false duck=1.000 holding=true
- remote_pause: PASS status t_ms=13564 callbacks=545024 rate=48000 position_ms=7135 playing=false suspended=false duck=1.000 holding=false
- remote_play: PASS status t_ms=17593 callbacks=738304 rate=48000 position_ms=9354 playing=true suspended=false duck=1.000 holding=true

**Samsung Galaxy A73 5G**: FAIL
- foreground_service: PASS isForeground=true
- in_app: PASS PASS; failed:
- media_session_playing: PASS state=3
- plays_with_screen_off: PASS 60 s of audio callbacks over 60 s with the screen off; after: status t_ms=77657 callbacks=3622144 rate=48000 position_ms=13594 playing=true suspended=false duck=1.000 holding=true
- remote_pause: FAIL status t_ms=13503 callbacks=542720 rate=48000 position_ms=9440 playing=true suspended=false duck=1.000 holding=true
- remote_play: PASS status t_ms=16506 callbacks=686848 rate=48000 position_ms=12442 playing=true suspended=false duck=1.000 holding=true

## Raster A/B: the 2026-10-18/19 CPU additions, one at a time

Same phone, same run, same build; only the named feature is off. Negative deltas mean switching it off helped — the feature costs that much.

**LG Stylo 5**

| pass | switched off | frame median | raster stage median | PSS |
|---|---|---|---|---|
| `cpu` | nothing (all three on) | 33.78 ms | 28.52 ms | 77 MB |
| `cpunothreads` | row-parallel walk | 33.68 ms (-0.10) | 28.47 ms (-0.05) | 78 MB (+1) |
| `cpunofill` | path-fill cache | 33.95 ms (+0.17) | 28.70 ms (+0.18) | 77 MB (+0) |
| `cpunoblur` | blur cache | 33.74 ms (-0.04) | 28.55 ms (+0.03) | 68 MB (-9) |
| `cpubare` | all three | 34.41 ms (+0.63) | 29.06 ms (+0.54) | 78 MB (+1) |

**Samsung Galaxy A73 5G**

| pass | switched off | frame median | raster stage median | PSS |
|---|---|---|---|---|
| `cpu` | nothing (all three on) | 7.27 ms | 5.92 ms | 80 MB |
| `cpunothreads` | row-parallel walk | 7.38 ms (+0.10) | 6.00 ms (+0.08) | 80 MB (-0) |
| `cpunofill` | path-fill cache | 7.17 ms (-0.10) | 5.80 ms (-0.12) | 80 MB (+0) |
| `cpunoblur` | blur cache | 8.56 ms (+1.29) | 7.11 ms (+1.19) | 81 MB (+0) |
| `cpubare` | all three | 8.78 ms (+1.51) | 6.92 ms (+1.00) | 81 MB (+0) |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.

## gpuscale promotion verdict

```
gpuscale verdict: KEEP-OPT-IN

  low: no-change — every device of this tier passes at full resolution
  medium: no-change — every device of this tier passes at full resolution

  LMQ7201bd5075a [low]: unneeded — passes at full resolution (16.1 ms <= 16.7 ms)
  R5CT41PZV0R [medium]: unneeded — passes at full resolution (2.4 ms <= 8.3 ms)

The flip a promoted verdict licenses (quality.rs, `gpu_scale()`): when
`VIEWW_GPU_SCALE` is unset, take `(none)`'s profile scale instead of
returning None — `QualityProfile::for_tier(device_facts().tier)
.resolution_scale`, gated to the promoted tiers so a tier the farm has not
tested keeps full resolution. `VIEWW_GPU_SCALE=off` still wins: the
kill-switch outlives the promotion. Commit the verdict JSON beside the
flip (`ci/mobile/`), because the numbers that licensed it are the review.

gpuscale_verdict: wrote /home/runner/work/_temp/device-farm/gpuscale-verdict.json
```
