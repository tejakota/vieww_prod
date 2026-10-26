# Framework checks — FAILURE

- branch: `main`  sha: `da8911154550b036a2822019e64b75e6525ed313`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40339740928

| job | verdict | failed steps |
|---|---|---|
| Windows (D3D12 on WARP) | FAIL | D3D12 suite (WARP when there is no hardware adapter) |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| Coverage floor | skipped |  |
| Android (compile) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Wire check (root ↔ vieww_base) | PASS |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

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

### Android emulator (APK, device and accessibility suites) — skipped

### macOS and iOS (Metal) — skipped

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### checks.sh (Linux) — skipped

### Android device suite (self-hosted) — skipped

### Coverage floor — skipped

### Android (compile) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: 
