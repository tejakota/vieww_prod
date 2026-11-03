# Framework checks — SUCCESS

- branch: `close-open-gaps`  sha: `8cc565fd17faf4978d551b5141a771abf6daa54e`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41665864828

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| Accessibility gallery + AT-SPI dry-run (Linux) | skipped |  |
| Plugin compatibility matrix (FW-17) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Windows (D3D12 on WARP) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Android (compile) | skipped |  |
| Coverage floor | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Accessibility gallery + AT-SPI dry-run (Linux) — skipped

### Plugin compatibility matrix (FW-17) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### macOS and iOS (Metal) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Windows (D3D12 on WARP) — skipped

### checks.sh (Linux) — skipped

### Android (compile) — skipped

### Coverage floor — skipped

### Android device suite (self-hosted) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: 
