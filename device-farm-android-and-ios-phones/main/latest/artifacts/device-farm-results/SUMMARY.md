# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/4df62499-04fe-44c0-9612-a1e2775fcf77` — FAILED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | low | 9 | msm8953<br>Qualcomm, Adreno (TM) 506, OpenGL ES 3.2 V@331.0 (GIT@50f1b2 | 2.7 GB | 2.8 MB | 603 / 576 | median 33.698647ms, worst 51.10026ms, budget 16.666668ms (240 samples) | – | 0 of 600 / – | 79 / 20 | FAIL / FAIL / FAIL |
| Samsung Galaxy S20 (Unlocked) | mid | 13 | SM8250 (kona)<br>Qualcomm, Adreno (TM) 650, OpenGL ES 3.2 V@0502.0 (GIT@193b2 | 10.4 GB | 2.8 MB | 299 / 256 | median 10.734219ms, worst 20.016458ms, budget 16.666666ms (240 samples) | – | 0 of 600 / – | 133 / 22 | PASS / FAIL / FAIL |
| Samsung Galaxy S26 Ultra | high | 16 | SM8850 (canoe)<br>Qualcomm, Adreno (TM) 840, OpenGL ES 3.2 V@0842.19.3 (GIT@ba | 10.9 GB | 2.8 MB | 171 / 171 | median 5.468125ms, worst 17.281406ms, budget 8.333333ms (240 samples) | – | 0 of 600 / – | 75 / 25 | PASS / FAIL / FAIL |

**LG Stylo 5**
- gating passes: auto cpu (auto chose: not reported)
- CPU path: median 33.698647ms, worst 51.10026ms, budget 16.666668ms (240 samples)
  - known failure (known-failures.txt, not failing the run): frames_inside_their_budget the_typical_frame_is_inside_its_budget — RC5, PENDING.md §4.7: the CPU path on an msm8953 is 35 ms against 16.7 ms (run 39594824376), with the raster stage about 30 ms whatever the damage. The GPU path now passes here - 11.7 ms with frames overlapping, 16.4 ms without - and `auto` ships it; this is the fallback's bar.
  - memory: PSS 79 MB
  - start-up: before run 532 ms | app setup 0.1 | event loop started 0.0 | resumed 35.0 | window created 0.1 | present ready 1.7 | accessibility and show 0.0 | tree built 15.7 | window ready 0.8 | until first draw 0.1 | app before-frame hooks 15.9 | first build and layout 7.0 | first raster and present 113.2 = 721.4 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.56 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.65
  - stages: p95 ms: scale 0.10 plan 0.00 cpu 34.12 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.46 present 4.50
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.50 rects 23.75 images 0.00 layers 0.00 composite 0.00 total 24.25; p95 ms: text 0.00 shadows 0.00 paths 0.68 rects 27.66 images 0.00 layers 0.00 composite 0.00 total 28.34
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.12 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 18 s
  - input to present: input to present ms: median 39.3 p95 50.9 p99 51.2 worst 51.2 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 51.10 ms, outside the renderer 1.32 ms, on cpu4; stages ms: scale 0.07 plan 0.00 cpu 45.16 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.33 present 4.22
  - raster by core: cpu6 74 frames 28.35 ms | cpu5 69 frames 28.26 ms | cpu7 50 frames 28.56 ms | cpu4 24 frames 29.30 ms | cpu1 23 frames 29.78 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.549448ms / p95 39.079687ms / worst 51.10026ms vs budget 16.666668ms · 240/240 over
- CPU path presented through Vulkan (diagnostic): median 60.384896ms, worst 101.182135ms, budget 16.666668ms (240 samples)
  - memory: PSS 131 MB
  - start-up: before run 544 ms | app setup 0.2 | event loop started 0.0 | resumed 37.6 | window created 0.1 | present ready 62.1 | accessibility and show 0.0 | tree built 16.0 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 15.5 | first build and layout 4.7 | first raster and present 140.6 = 821.0 ms from process start
  - stages: medians ms: scale 0.08 plan 0.00 cpu 15.98 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 43.89
  - stages: p95 ms: scale 0.20 plan 0.00 cpu 31.26 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 45.19
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.54 rects 13.93 images 0.00 layers 0.00 composite 0.00 total 14.47; p95 ms: text 0.00 shadows 0.00 paths 1.42 rects 28.59 images 0.00 layers 0.00 composite 0.00 total 30.01
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 26.79 ms per frame over the last 240 frames, every thread (43% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 37 s
  - input to present: input to present ms: median 79.4 p95 100.1 p99 108.5 worst 108.5 over 50 inputs in 50 frames; 0 unanswered within 1 s
  - worst frame: work 101.18 ms, outside the renderer 2.74 ms, on cpu5; stages ms: scale 0.20 plan 0.00 cpu 53.60 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 44.64
  - raster by core: cpu4 87 frames 18.17 ms | cpu7 62 frames 15.39 ms | cpu6 45 frames 15.69 ms | cpu5 43 frames 15.67 ms | cpu0 3 frames 16.36 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 61.476014ms / p95 80.344792ms / worst 101.182135ms vs budget 16.666668ms · 240/240 over
- CPU path with an Android performance hint (diagnostic): median 33.739634ms, worst 53.350937ms, budget 16.666668ms (240 samples)
  - memory: PSS 80 MB
  - start-up: before run 535 ms | app setup 0.2 | event loop started 0.0 | resumed 29.5 | window created 0.1 | present ready 1.6 | accessibility and show 0.0 | tree built 17.3 | window ready 0.5 | until first draw 0.1 | app before-frame hooks 16.3 | first build and layout 6.4 | first raster and present 116.0 = 722.4 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.52 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.72
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 33.80 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.46 present 4.57
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.48 rects 23.75 images 0.00 layers 0.00 composite 0.00 total 24.23; p95 ms: text 0.00 shadows 0.00 paths 0.68 rects 27.91 images 0.00 layers 0.00 composite 0.00 total 28.59
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.25 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 18 s
  - input to present: input to present ms: median 40.1 p95 51.4 p99 53.5 worst 53.5 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 53.35 ms, outside the renderer 1.32 ms, on cpu5; stages ms: scale 0.07 plan 0.00 cpu 47.92 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.49 present 3.55
  - raster by core: cpu5 101 frames 28.54 ms | cpu7 53 frames 28.79 ms | cpu1 32 frames 28.19 ms | cpu6 27 frames 28.76 ms | cpu4 24 frames 28.18 ms | cpu0 3 frames 31.65 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - performance hint: unavailable (Android 12 or older: no APerformanceHint)
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.655201ms / p95 38.979479ms / worst 53.350937ms vs budget 16.666668ms · 240/240 over
- CPU path without raster threads, fill cache or blur cache (diagnostic A/B): median 33.534479ms, worst 50.299115ms, budget 16.666668ms (240 samples)
  - memory: PSS 79 MB
  - start-up: before run 532 ms | app setup 0.2 | event loop started 0.0 | resumed 32.5 | window created 0.0 | present ready 1.6 | accessibility and show 0.0 | tree built 15.1 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 18.6 | first build and layout 5.5 | first raster and present 115.0 = 721.1 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.41 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.58
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 34.37 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.44 present 4.30
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.48 rects 23.64 images 0.00 layers 0.00 composite 0.00 total 24.12; p95 ms: text 0.00 shadows 0.00 paths 0.68 rects 27.62 images 0.00 layers 0.00 composite 0.00 total 28.30
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.04 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 37.0) over 18 s
  - input to present: input to present ms: median 39.8 p95 50.4 p99 52.8 worst 52.8 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 50.30 ms, outside the renderer 1.28 ms, on cpu5; stages ms: scale 0.08 plan 0.00 cpu 44.89 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.41 present 3.64
  - raster by core: cpu5 95 frames 28.41 ms | cpu6 75 frames 28.40 ms | cpu7 44 frames 28.67 ms | cpu4 15 frames 27.89 ms | cpu0 11 frames 28.08 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier low · median 34.420831ms / p95 39.309792ms / worst 50.299115ms vs budget 16.666668ms · 240/240 over
- CPU path without the row-parallel walk only (diagnostic A/B): median 33.750625ms, worst 57.259168ms, budget 16.666668ms (240 samples)
  - memory: PSS 77 MB
  - start-up: before run 537 ms | app setup 0.2 | event loop started 0.0 | resumed 34.2 | window created 0.1 | present ready 1.8 | accessibility and show 0.0 | tree built 14.7 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 15.7 | first build and layout 8.3 | first raster and present 111.4 = 723.6 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.60 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.38 present 3.62
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 34.21 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.44 present 4.49
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.52 rects 23.82 images 0.00 layers 0.00 composite 0.00 total 24.34; p95 ms: text 0.00 shadows 0.00 paths 0.68 rects 27.24 images 0.00 layers 0.00 composite 0.00 total 27.92
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.42 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.9 °C (peak 38.2) over 19 s
  - input to present: input to present ms: median 41.8 p95 52.3 p99 57.4 worst 57.4 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 57.26 ms, outside the renderer 1.54 ms, on cpu2; stages ms: scale 0.07 plan 0.00 cpu 50.93 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.43 present 4.30
  - raster by core: cpu7 65 frames 28.45 ms | cpu6 59 frames 28.65 ms | cpu0 45 frames 28.89 ms | cpu5 39 frames 28.40 ms | cpu4 26 frames 28.91 ms | cpu2 4 frames 38.61 ms | cpu1 1 frames 30.64 ms | cpu3 1 frames 30.10 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier low · median 34.785064ms / p95 39.811875ms / worst 57.259168ms vs budget 16.666668ms · 240/240 over
- CPU path without the path-fill cache only (diagnostic A/B): median 33.657605ms, worst 51.989114ms, budget 16.666668ms (240 samples)
  - memory: PSS 78 MB
  - start-up: before run 524 ms | app setup 0.2 | event loop started 0.0 | resumed 28.7 | window created 0.1 | present ready 2.3 | accessibility and show 0.0 | tree built 14.6 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 18.0 | first build and layout 7.6 | first raster and present 120.8 = 716.4 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.48 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.39 present 3.61
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 33.91 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.44 present 4.37
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.50 rects 23.75 images 0.00 layers 0.00 composite 0.00 total 24.25; p95 ms: text 0.00 shadows 0.00 paths 0.69 rects 27.71 images 0.00 layers 0.00 composite 0.00 total 28.40
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.21 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.0 -> 37.0 °C (peak 38.9) over 18 s
  - input to present: input to present ms: median 39.2 p95 51.7 p99 52.1 worst 52.1 over 20 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 51.99 ms, outside the renderer 1.33 ms, on cpu6; stages ms: scale 0.07 plan 0.00 cpu 46.48 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.40 present 3.71
  - raster by core: cpu7 81 frames 28.63 ms | cpu5 78 frames 28.40 ms | cpu6 52 frames 28.44 ms | cpu4 26 frames 29.00 ms | cpu0 3 frames 29.86 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (VIEWW_PATH_CACHE_MB=0), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.579353ms / p95 39.253593ms / worst 51.989114ms vs budget 16.666668ms · 240/240 over
- CPU path without the blur cache only (diagnostic A/B): median 33.689114ms, worst 57.266614ms, budget 16.666668ms (240 samples)
  - memory: PSS 77 MB
  - start-up: before run 547 ms | app setup 0.2 | event loop started 0.0 | resumed 33.3 | window created 0.1 | present ready 3.4 | accessibility and show 0.0 | tree built 14.3 | window ready 0.3 | until first draw 0.1 | app before-frame hooks 15.2 | first build and layout 6.4 | first raster and present 117.1 = 737.9 ms from process start
  - stages: medians ms: scale 0.07 plan 0.00 cpu 28.52 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.38 present 3.68
  - stages: p95 ms: scale 0.09 plan 0.00 cpu 33.90 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.45 present 4.32
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.51 rects 23.82 images 0.00 layers 0.00 composite 0.00 total 24.33; p95 ms: text 0.00 shadows 0.00 paths 0.71 rects 27.13 images 0.00 layers 0.00 composite 0.00 total 27.84
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 34.25 ms per frame over the last 240 frames, every thread (97% of one core)
  - thermal: hottest zone 37.6 -> 37.6 °C (peak 38.2) over 18 s
  - input to present: input to present ms: median 40.7 p95 54.3 p99 57.4 worst 57.4 over 22 inputs in 22 frames; 0 unanswered within 1 s
  - worst frame: work 57.27 ms, outside the renderer 1.39 ms, on cpu4; stages ms: scale 0.07 plan 0.00 cpu 51.80 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.40 present 3.61
  - raster by core: cpu4 58 frames 28.34 ms | cpu7 54 frames 29.27 ms | cpu5 38 frames 28.52 ms | cpu6 35 frames 28.33 ms | cpu0 31 frames 28.35 ms | cpu1 24 frames 28.59 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 1.80/1.80 GHz | cpu5 1.80/1.80 GHz | cpu6 1.80/1.80 GHz | cpu7 1.80/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 34.622642ms / p95 39.189374ms / worst 57.266614ms vs budget 16.666668ms · 240/240 over
- stress screen, CPU path (diagnostic): median 730.411145ms, worst 1.019288333s, budget 16.666668ms (240 samples)
  - memory: PSS 187 MB
  - start-up: before run 537 ms | app setup 0.1 | event loop started 0.0 | resumed 33.1 | window created 0.1 | present ready 2.4 | accessibility and show 0.0 | tree built 23.8 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 17.1 | first build and layout 13.0 | first raster and present 1108.1 = 1735.1 ms from process start
  - stages: medians ms: scale 0.05 plan 0.00 cpu 720.31 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.37 present 7.11
  - stages: p95 ms: scale 0.10 plan 0.00 cpu 807.66 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.45 present 7.57
  - CPU raster by kind of work: medians ms: text 13.51 shadows 145.26 paths 59.76 rects 132.15 images 138.35 layers 123.11 composite 81.18 total 693.32; p95 ms: text 14.06 shadows 201.63 paths 64.29 rects 135.92 images 145.10 layers 130.33 composite 110.20 total 801.52
  - shadows blurred: 870 over 600 frames (1.45 a frame), 292.1 Mpx in all
  - CPU time: process CPU 789.58 ms per frame over the last 240 frames, every thread (106% of one core)
  - thermal: hottest zone 37.9 -> 37.0 °C (peak 37.9) over 448 s
  - input to present: input to present ms: median 733.0 p95 815.4 p99 850.6 worst 858.0 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 1019.29 ms, outside the renderer 3.30 ms, on cpu6; stages ms: scale 0.12 plan 0.00 cpu 1008.45 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.37 present 7.04
  - raster by core: cpu4 113 frames 719.78 ms | cpu5 62 frames 725.08 ms | cpu6 36 frames 710.04 ms | cpu7 14 frames 718.52 ms | cpu0 11 frames 725.61 ms | cpu1 4 frames 780.09 ms
  - clocks at report: cpu0 1.40/1.80 GHz | cpu1 1.40/1.80 GHz | cpu2 1.40/1.80 GHz | cpu3 1.40/1.80 GHz | cpu4 1.40/1.80 GHz | cpu5 1.40/1.80 GHz | cpu6 1.40/1.80 GHz | cpu7 1.40/1.80 GHz
  - device profile: tier low, 2.7 GB, 8 cores, msm8953 (API 28)
  - cache budgets: blur off (tier low), fill off (tier low), raster threads up to 8 workers per large mask
  - quality overlay: tier low · median 740.589211ms / p95 816.789271ms / worst 1.019288333s vs budget 16.666668ms · 240/240 over
- failed checks: frames_inside_their_budget the_typical_frame_is_inside_its_budget

**Samsung Galaxy S20 (Unlocked)**
- gating passes: auto cpu (auto chose: not reported)
- CPU path: median 10.734219ms, worst 20.016458ms, budget 16.666666ms (240 samples)
  - memory: PSS 133 MB
  - start-up: before run 236 ms | app setup 0.1 | event loop started 0.0 | resumed 14.2 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 4.0 | window ready 4.6 | until first draw 0.0 | app before-frame hooks 492.2 | first build and layout 2.0 | first raster and present 34.5 = 787.8 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.65 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.03 present 1.59
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.59 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.40 present 2.10
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 5.81 images 0.00 layers 0.00 composite 0.00 total 5.92; p95 ms: text 0.00 shadows 0.00 paths 0.20 rects 6.93 images 0.00 layers 0.00 composite 0.00 total 7.13
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 11.83 ms per frame over the last 240 frames, every thread (49% of one core)
  - thermal: status none -> none (worst none); headroom 0.33 -> 0.39 (highest 0.39; 1.0 is severe throttling); hottest zone 46.7 -> 42.0 °C (peak 53.7) over 16 s
  - input to present: input to present ms: median 18.5 p95 22.0 p99 22.0 worst 22.5 over 52 inputs in 33 frames; 0 unanswered within 1 s
  - worst frame: work 20.02 ms, outside the renderer 0.76 ms, on cpu5; stages ms: scale 0.07 plan 0.00 cpu 15.85 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.59 present 1.75
  - raster by core: cpu4 149 frames 7.77 ms | cpu5 65 frames 7.47 ms | cpu6 26 frames 7.54 ms
  - clocks at report: cpu0 1.08/1.80 GHz | cpu1 1.08/1.80 GHz | cpu2 1.08/1.80 GHz | cpu3 1.08/1.80 GHz | cpu4 2.25/2.42 GHz | cpu5 2.25/2.42 GHz | cpu6 2.25/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 10.960413ms / p95 12.875417ms / worst 20.016458ms vs budget 16.666666ms · 3/240 over
- CPU path presented through Vulkan (diagnostic): median 10.049426ms, worst 19.347448ms, budget 16.666666ms (240 samples)
  - memory: PSS 262 MB
  - start-up: before run 225 ms | app setup 0.1 | event loop started 0.0 | resumed 17.8 | window created 0.0 | present ready 23.6 | accessibility and show 0.0 | tree built 3.9 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 149.4 | first build and layout 1.3 | first raster and present 37.8 = 459.3 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 6.58 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 3.10
  - stages: p95 ms: scale 0.02 plan 0.00 cpu 7.96 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 3.57
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.11 rects 4.93 images 0.00 layers 0.00 composite 0.00 total 5.04; p95 ms: text 0.00 shadows 0.00 paths 0.16 rects 5.82 images 0.00 layers 0.00 composite 0.00 total 5.97
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 9.71 ms per frame over the last 240 frames, every thread (78% of one core)
  - thermal: status none -> none (worst none); headroom 0.39 -> 0.39 (highest 0.39; 1.0 is severe throttling); hottest zone 44.4 -> 48.3 °C (peak 60.7) over 7 s
  - input to present: input to present ms: median 12.1 p95 15.2 p99 15.9 worst 15.9 over 18 inputs in 18 frames; 0 unanswered within 1 s
  - worst frame: work 19.35 ms, outside the renderer 0.57 ms, on cpu4; stages ms: scale 0.03 plan 0.00 cpu 10.50 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 8.24
  - raster by core: cpu4 116 frames 6.54 ms | cpu6 54 frames 6.58 ms | cpu5 37 frames 6.91 ms | cpu7 33 frames 6.54 ms
  - clocks at report: cpu0 1.08/1.80 GHz | cpu1 1.08/1.80 GHz | cpu2 1.08/1.80 GHz | cpu3 1.08/1.80 GHz | cpu4 2.42/2.42 GHz | cpu5 2.42/2.42 GHz | cpu6 2.42/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 10.263658ms / p95 11.981249ms / worst 19.347448ms vs budget 16.666666ms · 1/240 over
- CPU path with an Android performance hint (diagnostic): median 10.395468ms, worst 19.072292ms, budget 16.666666ms (240 samples)
  - memory: PSS 134 MB
  - start-up: before run 225 ms | app setup 0.1 | event loop started 0.0 | resumed 14.7 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 7.8 | window ready 0.7 | until first draw 0.0 | app before-frame hooks 489.7 | first build and layout 1.9 | first raster and present 35.6 = 775.9 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.66 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.65 present 1.39
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.68 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.30 present 2.10
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 5.76 images 0.00 layers 0.00 composite 0.00 total 5.88; p95 ms: text 0.00 shadows 0.00 paths 0.19 rects 7.12 images 0.00 layers 0.00 composite 0.00 total 7.31
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 11.25 ms per frame over the last 240 frames, every thread (46% of one core)
  - thermal: status none -> none (worst none); headroom 0.42 -> 0.45 (highest 0.45; 1.0 is severe throttling); hottest zone 45.5 -> 40.1 °C (peak 45.5) over 16 s
  - input to present: input to present ms: median 18.8 p95 21.6 p99 22.3 worst 22.3 over 48 inputs in 30 frames; 0 unanswered within 1 s
  - worst frame: work 19.07 ms, outside the renderer 0.79 ms, on cpu4; stages ms: scale 0.07 plan 0.00 cpu 15.20 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.93 present 2.07
  - raster by core: cpu4 173 frames 7.62 ms | cpu5 67 frames 7.74 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 2.42/2.42 GHz | cpu5 2.42/2.42 GHz | cpu6 2.42/2.42 GHz | cpu7 0.84/2.84 GHz
  - performance hint: unavailable (the system declined a session)
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 10.461644ms / p95 12.424896ms / worst 19.072292ms vs budget 16.666666ms · 1/240 over
- CPU path without raster threads, fill cache or blur cache (diagnostic A/B): median 11.28724ms, worst 20.746666ms, budget 16.666666ms (240 samples)
  - memory: PSS 134 MB
  - start-up: before run 229 ms | app setup 0.1 | event loop started 0.0 | resumed 16.4 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 3.9 | window ready 0.5 | until first draw 0.0 | app before-frame hooks 463.9 | first build and layout 2.0 | first raster and present 34.4 = 750.3 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 8.11 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.04 present 1.66
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 10.13 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.45 present 2.11
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.13 rects 6.23 images 0.00 layers 0.00 composite 0.00 total 6.36; p95 ms: text 0.00 shadows 0.00 paths 0.17 rects 7.34 images 0.00 layers 0.00 composite 0.00 total 7.52
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 12.33 ms per frame over the last 240 frames, every thread (44% of one core)
  - thermal: status none -> none (worst none); headroom 0.41 -> 0.41 (highest 0.41; 1.0 is severe throttling); hottest zone 41.3 -> 38.5 °C (peak 41.3) over 16 s
  - input to present: input to present ms: median 17.5 p95 23.7 p99 194.5 worst 194.5 over 48 inputs in 36 frames; 0 unanswered within 1 s
  - worst frame: work 20.75 ms, outside the renderer 0.83 ms, on cpu6; stages ms: scale 0.07 plan 0.00 cpu 16.05 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.64 present 2.14
  - raster by core: cpu4 196 frames 8.13 ms | cpu6 44 frames 8.06 ms
  - clocks at report: cpu0 1.25/1.80 GHz | cpu1 1.25/1.80 GHz | cpu2 1.25/1.80 GHz | cpu3 1.25/1.80 GHz | cpu4 2.42/2.42 GHz | cpu5 2.42/2.42 GHz | cpu6 2.42/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier high · median 11.404052ms / p95 13.403906ms / worst 20.746666ms vs budget 16.666666ms · 5/240 over
- CPU path without the row-parallel walk only (diagnostic A/B): median 10.245052ms, worst 17.345364ms, budget 16.666666ms (240 samples)
  - memory: PSS 133 MB
  - start-up: before run 227 ms | app setup 0.1 | event loop started 0.0 | resumed 15.8 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 3.9 | window ready 0.5 | until first draw 0.0 | app before-frame hooks 143.0 | first build and layout 1.5 | first raster and present 36.0 = 428.7 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.58 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.68 present 1.41
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.21 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.28 present 2.00
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.13 rects 5.73 images 0.00 layers 0.00 composite 0.00 total 5.86; p95 ms: text 0.00 shadows 0.00 paths 0.19 rects 6.96 images 0.00 layers 0.00 composite 0.00 total 7.15
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 11.08 ms per frame over the last 240 frames, every thread (57% of one core)
  - thermal: status none -> none (worst none); headroom 0.43 -> 0.44 (highest 0.44; 1.0 is severe throttling); hottest zone 43.2 -> 42.4 °C (peak 47.5) over 12 s
  - input to present: input to present ms: median 18.9 p95 21.9 p99 22.9 worst 22.9 over 40 inputs in 27 frames; 0 unanswered within 1 s
  - worst frame: work 17.35 ms, outside the renderer 0.77 ms, on cpu6; stages ms: scale 0.07 plan 0.00 cpu 14.15 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.76 present 1.59
  - raster by core: cpu4 164 frames 7.54 ms | cpu6 76 frames 7.70 ms
  - clocks at report: cpu0 1.25/1.80 GHz | cpu1 1.25/1.80 GHz | cpu2 1.25/1.80 GHz | cpu3 1.25/1.80 GHz | cpu4 2.25/2.42 GHz | cpu5 2.25/2.42 GHz | cpu6 2.25/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier high · median 10.405941ms / p95 12.148334ms / worst 17.345364ms vs budget 16.666666ms · 3/240 over
- CPU path without the path-fill cache only (diagnostic A/B): median 10.814635ms, worst 22.335364ms, budget 16.666666ms (240 samples)
  - memory: PSS 134 MB
  - start-up: before run 222 ms | app setup 0.1 | event loop started 0.0 | resumed 17.7 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 6.4 | window ready 0.7 | until first draw 0.1 | app before-frame hooks 154.3 | first build and layout 3.6 | first raster and present 34.1 = 439.9 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.79 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.99 present 1.59
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.50 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.32 present 2.02
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 5.91 images 0.00 layers 0.00 composite 0.00 total 6.03; p95 ms: text 0.00 shadows 0.00 paths 0.18 rects 7.07 images 0.00 layers 0.00 composite 0.00 total 7.25
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 11.88 ms per frame over the last 240 frames, every thread (49% of one core)
  - thermal: status none -> none (worst none); headroom 0.46 -> 0.48 (highest 0.48; 1.0 is severe throttling); hottest zone 46.3 -> 41.3 °C (peak 49.8) over 15 s
  - input to present: input to present ms: median 17.6 p95 22.0 p99 475.8 worst 475.8 over 42 inputs in 28 frames; 0 unanswered within 1 s
  - worst frame: work 22.34 ms, outside the renderer 1.26 ms, on cpu5; stages ms: scale 0.21 plan 0.00 cpu 17.93 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.05 present 1.88
  - raster by core: cpu4 165 frames 7.89 ms | cpu5 75 frames 7.50 ms
  - clocks at report: cpu0 1.80/1.80 GHz | cpu1 1.80/1.80 GHz | cpu2 1.80/1.80 GHz | cpu3 1.80/1.80 GHz | cpu4 2.42/2.42 GHz | cpu5 2.42/2.42 GHz | cpu6 2.42/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (tier high)
  - quality overlay: tier high · median 11.044407ms / p95 12.905104ms / worst 22.335364ms vs budget 16.666666ms · 3/240 over
- CPU path without the blur cache only (diagnostic A/B): median 10.809375ms, worst 18.984426ms, budget 16.666666ms (240 samples)
  - memory: PSS 133 MB
  - start-up: before run 216 ms | app setup 0.1 | event loop started 0.0 | resumed 16.3 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 4.9 | window ready 0.6 | until first draw 0.0 | app before-frame hooks 483.0 | first build and layout 2.2 | first raster and present 36.0 = 759.4 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.71 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.01 present 1.59
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 9.48 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.35 present 2.08
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 5.79 images 0.00 layers 0.00 composite 0.00 total 5.91; p95 ms: text 0.06 shadows 0.00 paths 0.20 rects 7.00 images 0.00 layers 0.00 composite 0.00 total 7.26
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 11.88 ms per frame over the last 240 frames, every thread (47% of one core)
  - thermal: status none -> none (worst none); headroom 0.48 -> 0.49 (highest 0.49; 1.0 is severe throttling); hottest zone 45.1 -> 41.3 °C (peak 55.3) over 16 s
  - input to present: input to present ms: median 18.5 p95 22.4 p99 22.7 worst 22.9 over 54 inputs in 37 frames; 0 unanswered within 1 s
  - worst frame: work 18.98 ms, outside the renderer 0.80 ms, on cpu6; stages ms: scale 0.07 plan 0.00 cpu 14.53 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.67 present 1.91
  - raster by core: cpu5 170 frames 7.87 ms | cpu6 31 frames 7.66 ms | cpu7 23 frames 7.23 ms | cpu4 16 frames 7.11 ms
  - clocks at report: cpu0 1.25/1.80 GHz | cpu1 1.25/1.80 GHz | cpu2 1.25/1.80 GHz | cpu3 1.25/1.80 GHz | cpu4 2.42/2.42 GHz | cpu5 2.42/2.42 GHz | cpu6 2.42/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 10.992341ms / p95 12.544792ms / worst 18.984426ms vs budget 16.666666ms · 3/240 over
- stress screen, CPU path (diagnostic): median 243.842136ms, worst 285.652292ms, budget 16.666666ms (240 samples)
  - memory: PSS 317 MB
  - start-up: before run 230 ms | app setup 0.1 | event loop started 0.0 | resumed 15.7 | window created 0.0 | present ready 0.5 | accessibility and show 0.0 | tree built 3.8 | window ready 0.5 | until first draw 0.0 | app before-frame hooks 136.9 | first build and layout 3.5 | first raster and present 244.8 = 635.4 ms from process start
  - stages: medians ms: scale 0.01 plan 0.00 cpu 239.42 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.79 present 3.03
  - stages: p95 ms: scale 0.07 plan 0.00 cpu 262.35 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.93 present 3.36
  - CPU raster by kind of work: medians ms: text 2.76 shadows 29.90 paths 18.41 rects 40.78 images 37.93 layers 70.15 composite 24.86 total 224.80; p95 ms: text 2.88 shadows 36.52 paths 19.75 rects 54.36 images 38.23 layers 73.98 composite 30.46 total 256.18
  - shadows blurred: 839 over 600 frames (1.40 a frame), 416.3 Mpx in all
  - CPU time: process CPU 249.29 ms per frame over the last 240 frames, every thread (81% of one core)
  - thermal: status none -> none (worst none); headroom 0.49 -> 0.59 (highest 0.59; 1.0 is severe throttling); hottest zone 46.3 -> 55.7 °C (peak 64.2) over 192 s
  - input to present: input to present ms: median 245.0 p95 553.0 p99 721.4 worst 731.0 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 285.65 ms, outside the renderer 1.67 ms, on cpu7; stages ms: scale 0.04 plan 0.00 cpu 278.39 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 1.01 present 4.55
  - raster by core: cpu7 240 frames 239.42 ms
  - clocks at report: cpu0 1.17/1.80 GHz | cpu1 1.17/1.80 GHz | cpu2 1.17/1.80 GHz | cpu3 1.17/1.80 GHz | cpu4 0.71/2.42 GHz | cpu5 0.71/2.42 GHz | cpu6 0.71/2.42 GHz | cpu7 0.84/2.84 GHz
  - device profile: tier high, 10.4 GB, 8 cores, SM8250 (kona) (API 33)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 246.456237ms / p95 267.902083ms / worst 285.652292ms vs budget 16.666666ms · 240/240 over

**Samsung Galaxy S26 Ultra**
- gating passes: auto cpu (auto chose: not reported)
- CPU path: median 5.468125ms, worst 17.281406ms, budget 8.333333ms (240 samples)
  - **passed despite a known-failures.txt entry**: listed in known-failures.txt but passed: delete the entry
  - memory: PSS 75 MB
  - start-up: before run 148 ms | app setup 0.1 | event loop started 0.0 | resumed 13.0 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 3.7 | window ready 0.4 | until first draw 0.0 | app before-frame hooks 70.1 | first build and layout 1.0 | first raster and present 13.7 = 250.1 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 4.92 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.03 present 0.17
  - stages: p95 ms: scale 0.02 plan 0.00 cpu 6.28 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 3.38 present 0.30
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.11 rects 4.29 images 0.00 layers 0.00 composite 0.00 total 4.40; p95 ms: text 0.00 shadows 0.00 paths 0.20 rects 5.43 images 0.00 layers 0.00 composite 0.00 total 5.63
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.00 ms per frame over the last 240 frames, every thread (64% of one core)
  - thermal: status none -> none (worst none); headroom 0.32 -> 0.32 (highest 0.32; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 6 s
  - input to present: input to present ms: median 9.5 p95 118.1 p99 118.5 worst 118.5 over 16 inputs in 11 frames; 0 unanswered within 1 s
  - worst frame: work 17.28 ms, outside the renderer 0.71 ms, on cpu5; stages ms: scale 0.04 plan 0.00 cpu 16.12 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.08 present 0.33
  - raster by core: cpu5 66 frames 4.96 ms | cpu4 56 frames 4.96 ms | cpu1 32 frames 4.85 ms | cpu0 30 frames 4.90 ms | cpu2 25 frames 5.30 ms | cpu6 21 frames 3.49 ms | cpu3 8 frames 5.12 ms | cpu7 2 frames 4.15 ms
  - clocks at report: cpu0 2.50/3.63 GHz | cpu1 2.50/3.63 GHz | cpu2 2.50/3.63 GHz | cpu3 2.50/3.63 GHz | cpu4 2.50/3.63 GHz | cpu5 2.50/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 6.089315ms / p95 8.138802ms / worst 17.281406ms vs budget 8.333333ms · 5/240 over
- CPU path presented through Vulkan (diagnostic): median 3.502865ms, worst 6.693907ms, budget 8.333333ms (240 samples)
  - memory: PSS 156 MB
  - start-up: before run 138 ms | app setup 0.1 | event loop started 0.0 | resumed 16.4 | window created 0.0 | present ready 7.8 | accessibility and show 0.0 | tree built 2.6 | window ready 0.4 | until first draw 0.1 | app before-frame hooks 6.6 | first build and layout 0.9 | first raster and present 15.4 = 188.1 ms from process start
  - stages: medians ms: scale 0.01 plan 0.00 cpu 2.29 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 1.08
  - stages: p95 ms: scale 0.01 plan 0.00 cpu 4.10 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 2.20
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.05 rects 2.16 images 0.00 layers 0.00 composite 0.00 total 2.20; p95 ms: text 0.00 shadows 0.00 paths 0.08 rects 3.79 images 0.00 layers 0.00 composite 0.00 total 3.88
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 3.04 ms per frame over the last 240 frames, every thread (80% of one core)
  - thermal: status none -> none (worst none); headroom 0.34 -> 0.34 (highest 0.34; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 2 s
  - input to present: no input reached this window
  - worst frame: work 6.69 ms, outside the renderer 0.18 ms, on cpu5; stages ms: scale 0.01 plan 0.00 cpu 4.00 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.00 present 2.51
  - raster by core: cpu7 167 frames 2.12 ms | cpu6 41 frames 2.49 ms | cpu0 12 frames 3.94 ms | cpu4 8 frames 4.17 ms | cpu5 8 frames 4.00 ms | cpu1 4 frames 4.34 ms
  - clocks at report: cpu0 1.23/3.63 GHz | cpu1 1.23/3.63 GHz | cpu2 1.23/3.63 GHz | cpu3 1.23/3.63 GHz | cpu4 1.23/3.63 GHz | cpu5 1.23/3.63 GHz | cpu6 4.51/4.74 GHz | cpu7 4.51/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 3.764638ms / p95 5.582448ms / worst 6.693907ms vs budget 8.333333ms · 0/240 over
- CPU path with an Android performance hint (diagnostic): median 9.71526ms, worst 16.866719ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 75 MB
  - start-up: before run 152 ms | app setup 0.1 | event loop started 0.0 | resumed 14.8 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 3.3 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 92.8 | first build and layout 1.1 | first raster and present 15.6 = 280.6 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 8.87 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.04 present 0.24
  - stages: p95 ms: scale 0.04 plan 0.00 cpu 13.27 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 5.21 present 0.42
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.20 rects 7.82 images 0.00 layers 0.00 composite 0.00 total 8.02; p95 ms: text 0.07 shadows 0.00 paths 0.39 rects 11.90 images 0.00 layers 0.00 composite 0.00 total 12.36
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 10.25 ms per frame over the last 240 frames, every thread (54% of one core)
  - thermal: status none -> none (worst none); headroom 0.36 -> 0.36 (highest 0.36; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 8 s
  - input to present: input to present ms: median 15.6 p95 22.0 p99 187.3 worst 187.3 over 30 inputs in 20 frames; 0 unanswered within 1 s
  - worst frame: work 16.87 ms, outside the renderer 0.25 ms, on cpu1; stages ms: scale 0.02 plan 0.00 cpu 6.89 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 9.31 present 0.39
  - raster by core: cpu4 83 frames 8.97 ms | cpu5 81 frames 9.32 ms | cpu0 29 frames 8.00 ms | cpu1 29 frames 8.97 ms | cpu3 14 frames 6.10 ms | cpu2 3 frames 6.18 ms | cpu6 1 frames 10.29 ms
  - clocks at report: cpu0 1.44/3.63 GHz | cpu1 1.44/3.63 GHz | cpu2 1.44/3.63 GHz | cpu3 1.44/3.63 GHz | cpu4 1.44/3.63 GHz | cpu5 1.44/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - performance hint: on (target 8.33 ms)
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 10.055358ms / p95 16.519688ms / worst 16.866719ms vs budget 16.666668ms · 5/240 over
- CPU path without raster threads, fill cache or blur cache (diagnostic A/B): median 5.394583ms, worst 12.248958ms, budget 8.333333ms (240 samples)
  - memory: PSS 74 MB
  - start-up: before run 151 ms | app setup 0.0 | event loop started 0.0 | resumed 10.2 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 4.0 | window ready 0.5 | until first draw 0.0 | app before-frame hooks 43.3 | first build and layout 1.9 | first raster and present 16.9 = 227.8 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 4.99 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.03 present 0.16
  - stages: p95 ms: scale 0.03 plan 0.00 cpu 6.13 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.05 present 0.28
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.12 rects 4.34 images 0.00 layers 0.00 composite 0.00 total 4.46; p95 ms: text 0.00 shadows 0.00 paths 0.20 rects 5.28 images 0.00 layers 0.00 composite 0.00 total 5.48
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 6.08 ms per frame over the last 240 frames, every thread (67% of one core)
  - thermal: status none -> none (worst none); headroom 0.34 -> 0.34 (highest 0.34; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 6 s
  - input to present: input to present ms: median 9.4 p95 13.4 p99 15.2 worst 15.2 over 16 inputs in 8 frames; 0 unanswered within 1 s
  - worst frame: work 12.25 ms, outside the renderer 0.51 ms, on cpu0; stages ms: scale 0.04 plan 0.00 cpu 11.35 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.08 present 0.27
  - raster by core: cpu1 66 frames 5.00 ms | cpu5 58 frames 5.02 ms | cpu0 51 frames 4.94 ms | cpu4 29 frames 5.09 ms | cpu7 18 frames 3.95 ms | cpu6 10 frames 3.98 ms | cpu2 5 frames 5.30 ms | cpu3 3 frames 6.20 ms
  - clocks at report: cpu0 2.23/3.63 GHz | cpu1 2.23/3.63 GHz | cpu2 2.23/3.63 GHz | cpu3 2.23/3.63 GHz | cpu4 2.23/3.63 GHz | cpu5 2.23/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier high · median 5.389612ms / p95 6.65401ms / worst 12.248958ms vs budget 8.333333ms · 2/240 over
- CPU path without the row-parallel walk only (diagnostic A/B): median 8.274063ms, worst 21.462187ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 84 MB
  - start-up: before run 143 ms | app setup 0.1 | event loop started 0.0 | resumed 13.6 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 2.8 | window ready 1.6 | until first draw 0.0 | app before-frame hooks 67.7 | first build and layout 1.1 | first raster and present 18.5 = 249.2 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.33 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.04 present 0.20
  - stages: p95 ms: scale 0.04 plan 0.00 cpu 12.57 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 9.72 present 0.42
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.18 rects 6.37 images 0.00 layers 0.00 composite 0.00 total 6.55; p95 ms: text 0.00 shadows 0.00 paths 0.35 rects 11.60 images 0.00 layers 0.00 composite 0.00 total 11.94
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 8.79 ms per frame over the last 240 frames, every thread (54% of one core)
  - thermal: status none -> none (worst none); headroom 0.35 -> 0.35 (highest 0.35; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 7 s
  - input to present: input to present ms: median 13.5 p95 21.9 p99 22.1 worst 22.1 over 24 inputs in 15 frames; 0 unanswered within 1 s
  - worst frame: work 21.46 ms, outside the renderer 0.27 ms, on cpu1; stages ms: scale 0.02 plan 0.00 cpu 8.54 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 12.18 present 0.44
  - raster by core: cpu5 82 frames 8.42 ms | cpu4 53 frames 7.99 ms | cpu1 40 frames 7.31 ms | cpu0 21 frames 4.65 ms | cpu3 19 frames 8.37 ms | cpu2 10 frames 5.19 ms | cpu7 9 frames 3.75 ms | cpu6 6 frames 4.18 ms
  - clocks at report: cpu0 0.79/3.63 GHz | cpu1 0.79/3.63 GHz | cpu2 0.79/3.63 GHz | cpu3 0.79/3.63 GHz | cpu4 0.79/3.63 GHz | cpu5 0.79/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (VIEWW_RASTER_THREADS=0)
  - quality overlay: tier high · median 9.303944ms / p95 20.774115ms / worst 21.462187ms vs budget 16.666668ms · 25/240 over
- CPU path without the path-fill cache only (diagnostic A/B): median 10.155574ms, worst 17.455259ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 75 MB
  - start-up: before run 147 ms | app setup 0.1 | event loop started 0.0 | resumed 18.8 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 4.0 | window ready 0.6 | until first draw 0.0 | app before-frame hooks 26.9 | first build and layout 1.1 | first raster and present 19.9 = 218.8 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 9.28 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.04 present 0.23
  - stages: p95 ms: scale 0.04 plan 0.00 cpu 13.44 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 6.77 present 0.38
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.20 rects 8.40 images 0.00 layers 0.00 composite 0.00 total 8.60; p95 ms: text 0.07 shadows 0.00 paths 0.40 rects 12.17 images 0.00 layers 0.00 composite 0.00 total 12.64
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 10.67 ms per frame over the last 240 frames, every thread (55% of one core)
  - thermal: status none -> none (worst none); headroom 0.36 -> 0.36 (highest 0.36; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 8 s
  - input to present: input to present ms: median 16.6 p95 25.8 p99 157.9 worst 157.9 over 28 inputs in 19 frames; 0 unanswered within 1 s
  - worst frame: work 17.46 ms, outside the renderer 0.83 ms, on cpu0; stages ms: scale 0.04 plan 0.00 cpu 16.17 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.09 present 0.33
  - raster by core: cpu5 101 frames 9.95 ms | cpu4 55 frames 9.26 ms | cpu1 48 frames 8.89 ms | cpu0 21 frames 8.98 ms | cpu2 7 frames 10.46 ms | cpu3 6 frames 9.73 ms | cpu7 2 frames 7.02 ms
  - clocks at report: cpu0 2.36/3.63 GHz | cpu1 2.36/3.63 GHz | cpu2 2.36/3.63 GHz | cpu3 2.36/3.63 GHz | cpu4 2.36/3.63 GHz | cpu5 2.36/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (VIEWW_PATH_CACHE_MB=0), raster threads serial (tier high)
  - quality overlay: tier high · median 10.650129ms / p95 16.663072ms / worst 17.455259ms vs budget 16.666668ms · 12/240 over
- CPU path without the blur cache only (diagnostic A/B): median 8.333593ms, worst 21.293124ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 75 MB
  - start-up: before run 147 ms | app setup 0.1 | event loop started 0.0 | resumed 13.1 | window created 0.0 | present ready 0.4 | accessibility and show 0.0 | tree built 3.1 | window ready 0.3 | until first draw 0.0 | app before-frame hooks 91.7 | first build and layout 1.9 | first raster and present 20.8 = 278.3 ms from process start
  - stages: medians ms: scale 0.02 plan 0.00 cpu 7.65 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.04 present 0.22
  - stages: p95 ms: scale 0.04 plan 0.00 cpu 13.06 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 10.22 present 0.49
  - CPU raster by kind of work: medians ms: text 0.00 shadows 0.00 paths 0.18 rects 6.67 images 0.00 layers 0.00 composite 0.00 total 6.84; p95 ms: text 0.00 shadows 0.00 paths 0.33 rects 12.07 images 0.00 layers 0.00 composite 0.00 total 12.40
  - shadows blurred: 0 over 600 frames (0.00 a frame), 0.0 Mpx in all
  - CPU time: process CPU 9.00 ms per frame over the last 240 frames, every thread (54% of one core)
  - thermal: status none -> none (worst none); headroom 0.36 -> 0.36 (highest 0.36; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 7 s
  - input to present: input to present ms: median 13.0 p95 20.8 p99 20.9 worst 20.9 over 24 inputs in 16 frames; 0 unanswered within 1 s
  - worst frame: work 21.29 ms, outside the renderer 0.29 ms, on cpu5; stages ms: scale 0.03 plan 0.00 cpu 9.53 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 10.89 present 0.56
  - raster by core: cpu4 59 frames 8.79 ms | cpu3 57 frames 4.83 ms | cpu5 48 frames 8.80 ms | cpu1 40 frames 9.51 ms | cpu0 15 frames 8.57 ms | cpu2 10 frames 10.96 ms | cpu7 8 frames 4.05 ms | cpu6 3 frames 6.13 ms
  - clocks at report: cpu0 1.44/3.63 GHz | cpu1 1.44/3.63 GHz | cpu2 1.44/3.63 GHz | cpu3 1.44/3.63 GHz | cpu4 1.44/3.63 GHz | cpu5 1.44/3.63 GHz | cpu6 0.88/4.74 GHz | cpu7 0.88/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur off (VIEWW_BLUR_CACHE_MB=0), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 9.742352ms / p95 20.75901ms / worst 21.293124ms vs budget 16.666668ms · 32/240 over
- stress screen, CPU path (diagnostic): median 56.330624ms, worst 67.369634ms, budget 16.666668ms (240 samples)
  - frame rate: frame rate changed 1 time(s), ending at 60 Hz
  - memory: PSS 212 MB
  - start-up: before run 140 ms | app setup 0.1 | event loop started 0.0 | resumed 14.1 | window created 0.0 | present ready 0.3 | accessibility and show 0.0 | tree built 2.6 | window ready 0.8 | until first draw 0.1 | app before-frame hooks 53.5 | first build and layout 1.9 | first raster and present 74.3 = 288.1 ms from process start
  - stages: medians ms: scale 0.00 plan 0.00 cpu 55.65 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.02 present 0.48
  - stages: p95 ms: scale 0.00 plan 0.00 cpu 61.84 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.03 present 0.50
  - CPU raster by kind of work: medians ms: text 0.80 shadows 5.85 paths 3.49 rects 7.79 images 6.85 layers 22.88 composite 5.75 total 53.40; p95 ms: text 1.00 shadows 10.85 paths 3.86 rects 8.22 images 7.19 layers 24.49 composite 6.78 total 62.38
  - shadows blurred: 883 over 600 frames (1.47 a frame), 323.7 Mpx in all
  - CPU time: process CPU 56.92 ms per frame over the last 240 frames, every thread (94% of one core)
  - thermal: status none -> none (worst none); headroom 0.36 -> 0.55 (highest 0.55; 1.0 is severe throttling); hottest zone 105.0 -> 105.0 °C (peak 105.0) over 36 s
  - input to present: input to present ms: median 56.5 p95 64.5 p99 118.7 worst 123.5 over 82 inputs in 82 frames; 0 unanswered within 1 s
  - worst frame: work 67.37 ms, outside the renderer 0.26 ms, on cpu6; stages ms: scale 0.00 plan 0.00 cpu 66.60 gpu_upload 0.00 gpu_record 0.00 gpu_submit 0.00 gpu_wait 0.00 gpu_readback 0.00 acquire 0.02 present 0.49
  - raster by core: cpu6 225 frames 55.74 ms | cpu7 15 frames 54.41 ms
  - clocks at report: cpu0 2.61/3.63 GHz | cpu1 2.61/3.63 GHz | cpu2 2.61/3.63 GHz | cpu3 2.61/3.63 GHz | cpu4 2.61/3.63 GHz | cpu5 2.61/3.63 GHz | cpu6 4.74/4.74 GHz | cpu7 4.74/4.74 GHz
  - device profile: tier high, 10.9 GB, 8 cores, SM8850 (canoe) (API 36)
  - cache budgets: blur 8 MiB (tier high), fill off (tier high), raster threads serial (tier high)
  - quality overlay: tier high · median 56.990634ms / p95 62.51151ms / worst 67.369634ms vs budget 16.666668ms · 240/240 over

## Audio: focus, interruptions, background playback

**LG Stylo 5**: FAIL
- foreground_service: PASS isForeground=true
- in_app: PASS PASS; failed:
- media_session_playing: PASS state=3
- plays_with_screen_off: PASS 61 s of audio callbacks over 61 s with the screen off; after: status t_ms=77720 callbacks=3624960 rate=48000 position_ms=13642 playing=true suspended=false duck=1.000 holding=true
- remote_pause: FAIL status t_ms=13515 callbacks=543488 rate=48000 position_ms=9445 playing=true suspended=false duck=1.000 holding=true
- remote_play: PASS status t_ms=16526 callbacks=687872 rate=48000 position_ms=12453 playing=true suspended=false duck=1.000 holding=true

**Samsung Galaxy S20 (Unlocked)**: FAIL
- foreground_service: PASS isForeground=true
- in_app: PASS PASS; failed:
- media_session_playing: FAIL state=absent
- plays_with_screen_off: PASS 61 s of audio callbacks over 61 s with the screen off; after: status t_ms=77531 callbacks=3619328 rate=48000 position_ms=10239 playing=true suspended=false duck=1.000 holding=true
- remote_pause: PASS status t_ms=13409 callbacks=541184 rate=48000 position_ms=6517 playing=false suspended=false duck=1.000 holding=false
- remote_play: PASS status t_ms=16419 callbacks=685824 rate=48000 position_ms=9125 playing=true suspended=false duck=1.000 holding=true

**Samsung Galaxy S26 Ultra**: FAIL
- foreground_service: PASS isForeground=true
- in_app: PASS PASS; failed:
- media_session_playing: FAIL state=absent
- plays_with_screen_off: PASS 60 s of audio callbacks over 60 s with the screen off; after: status t_ms=76443 callbacks=3568896 rate=48000 position_ms=9317 playing=true suspended=false duck=1.000 holding=true
- remote_pause: PASS status t_ms=12338 callbacks=491776 rate=48000 position_ms=6314 playing=false suspended=false duck=1.000 holding=false
- remote_play: PASS status t_ms=16350 callbacks=684544 rate=48000 position_ms=9226 playing=true suspended=false duck=1.000 holding=true

## Raster A/B: the 2026-10-18/19 CPU additions, one at a time

Same phone, same run, same build; only the named feature is off. Negative deltas mean switching it off helped — the feature costs that much.

**LG Stylo 5**

| pass | switched off | frame median | raster stage median | PSS |
|---|---|---|---|---|
| `cpu` | nothing (all three on) | 33.70 ms | 28.56 ms | 79 MB |
| `cpunothreads` | row-parallel walk | 33.75 ms (+0.05) | 28.60 ms (+0.04) | 77 MB (-2) |
| `cpunofill` | path-fill cache | 33.66 ms (-0.04) | 28.48 ms (-0.08) | 78 MB (-1) |
| `cpunoblur` | blur cache | 33.69 ms (-0.01) | 28.52 ms (-0.04) | 77 MB (-2) |
| `cpubare` | all three | 33.53 ms (-0.16) | 28.41 ms (-0.15) | 79 MB (+0) |

**Samsung Galaxy S20 (Unlocked)**

| pass | switched off | frame median | raster stage median | PSS |
|---|---|---|---|---|
| `cpu` | nothing (all three on) | 10.73 ms | 7.65 ms | 133 MB |
| `cpunothreads` | row-parallel walk | 10.25 ms (-0.49) | 7.58 ms (-0.07) | 133 MB (+0) |
| `cpunofill` | path-fill cache | 10.81 ms (+0.08) | 7.79 ms (+0.14) | 134 MB (+1) |
| `cpunoblur` | blur cache | 10.81 ms (+0.08) | 7.71 ms (+0.06) | 133 MB (-0) |
| `cpubare` | all three | 11.29 ms (+0.55) | 8.11 ms (+0.46) | 134 MB (+1) |

**Samsung Galaxy S26 Ultra**

| pass | switched off | frame median | raster stage median | PSS |
|---|---|---|---|---|
| `cpu` | nothing (all three on) | 5.47 ms | 4.92 ms | 75 MB |
| `cpunothreads` | row-parallel walk | 8.27 ms (+2.81) | 7.33 ms (+2.41) | 84 MB (+9) |
| `cpunofill` | path-fill cache | 10.16 ms (+4.69) | 9.28 ms (+4.36) | 75 MB (+0) |
| `cpunoblur` | blur cache | 8.33 ms (+2.87) | 7.65 ms (+2.73) | 75 MB (+0) |
| `cpubare` | all three | 5.39 ms (-0.07) | 4.99 ms (+0.07) | 74 MB (-0) |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.
