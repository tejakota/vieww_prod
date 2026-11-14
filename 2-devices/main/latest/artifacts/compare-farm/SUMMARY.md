# vieww on AWS Device Farm

run: `arn:aws:devicefarm:us-west-2:278061313767:run:393108fd-d195-4dad-acb4-12581f7a212f/421e0bc4-08f8-4b07-a55e-672d3fea06b5` — FAILED

| phone | tier | Android | SoC / GPU | RAM | APK | cold start cpu / gpu (ms, median of 3) | frame time cpu | frame time gpu | frames drawn by the GPU, cpu pass / gpu pass | PSS cpu / gpu (MB) | suite cpu / gpu / auto |
|---|---|---|---|---|---|---|---|---|---|---|---|
| LG Stylo 5 | low | 9 | msm8953<br>? | 2.7 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S20 (Unlocked) | mid | 13 | SM8250<br>? | 10.4 GB | – | – / – | – | – | – / – | – / – | – / – / – |
| Samsung Galaxy S26 Ultra | high | 16 | SM8850<br>? | 10.9 GB | – | – / – | – | – | – / – | – / – | – / – / – |

Targets from the audit, for reading the table: product UI p95 under 15 ms and cold start under 120 ms on a mid-tier phone; release APK under 6 MB.
