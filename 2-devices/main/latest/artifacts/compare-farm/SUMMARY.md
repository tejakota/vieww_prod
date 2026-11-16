# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/ac336b01-4f8f-4247-8c65-8ecb1a3156e7` — PASSED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | low | 9 | msm8953<br>? | 2.7 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 5 (Unlocked) | mid | 11 | lito<br>? | 7.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 7 Pro | mid | 13 | GS201<br>? | 11.2 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 8a | mid | 17 | Tensor G3<br>? | 7.4 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Google Pixel 9a | mid | 15 | Tensor G4<br>? | 7.4 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy A26 | mid | 15 | s5e8835<br>? | 5.3 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S25 | mid | 16 | SM8750<br>? | 10.9 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S26 Ultra | high | 16 | SM8850<br>? | 10.9 GB | – | – / – | – | – | – / – | – / – | – / – / – |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.
