# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/73d98520-6c1b-4a50-b853-ed629d898e53` — PASSED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | low | 9 | msm8953<br>? | 2.7 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 5 (Unlocked) | mid | 11 | lito<br>? | 7.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 7 | mid | 13 | GS201<br>? | 7.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 8 Pro | mid | 15 | Tensor G3<br>? | 11.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 9a | mid | 15 | Tensor G4<br>? | 7.4 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy A35 | mid | 14 | s5e8835<br>? | 5.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S25 | mid | 15 | SM8750<br>? | 10.9 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S26 Ultra | high | 16 | SM8850<br>? | 10.9 GB | – | – / – | – | – | – / – | – / – | – / – / – |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.
