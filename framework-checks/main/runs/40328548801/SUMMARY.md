# Framework checks — FAILURE

- branch: `main`  sha: `46bfa47614b82d4aa641e84f525ede84d9310241`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40328548801

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| Windows (D3D12 on WARP) | FAIL | D3D12 suite (WARP when there is no hardware adapter) |
| GPU on software Vulkan (lavapipe) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Coverage floor | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| Android (compile) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
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

### checks.sh (Linux) — skipped

### Coverage floor — skipped

### Android device suite (self-hosted) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### Android (compile) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### macOS and iOS (Metal) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: 
