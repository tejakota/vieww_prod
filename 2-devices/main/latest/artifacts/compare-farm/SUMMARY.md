# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/ea89e647-4eb2-4fa7-8525-fb15910b0e07` — PASSED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | low | 9 | msm8953<br>? | 2.7 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S20 (Unlocked) | mid | 13 | SM8250<br>? | 10.4 GB | – | – / – | – | – | – / – | – / – | – / – / – |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.
