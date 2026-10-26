# Framework checks — FAILURE

- branch: `main`  sha: `e39905317ff7342137f688e7b15adc307da59377`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40333211492

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| Windows (D3D12 on WARP) | FAIL | D3D12 suite (WARP when there is no hardware adapter) |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Android (compile) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| Coverage floor | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Windows (D3D12 on WARP) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: platform-check windows
- FAIL: D3D12 suite (WARP when there is no hardware adapter)
- PASS: HLSL that FXC refused (numbered, for the failure report)
- skipped: Clippy for the Windows backends
- skipped: Studio out-of-process preview (PENDING §3.3)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### GPU on software Vulkan (lavapipe) — skipped

### Android (compile) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Android device suite (self-hosted) — skipped

### checks.sh (Linux) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### macOS and iOS (Metal) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### Coverage floor — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: 
