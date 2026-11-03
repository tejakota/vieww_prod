# Framework checks — SUCCESS

- branch: `close-open-gaps`  sha: `8cc565fd17faf4978d551b5141a771abf6daa54e`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41666216725

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| iOS simulator (tests in the sim, a frame on its screen) | FAIL | XCUITest accessibility suite (generated project, FW-10) |
| Android device suite (self-hosted) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Android (compile) | skipped |  |
| Coverage floor | skipped |  |
| checks.sh (Linux) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| Windows (D3D12 on WARP) | skipped |  |
| Accessibility gallery + AT-SPI dry-run (Linux) | skipped |  |
| Plugin compatibility matrix (FW-17) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### iOS simulator (tests in the sim, a frame on its screen) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and simulator target
- PASS: Boot a simulator
- PASS: Unit tests inside the simulator (simctl-runner)
- PASS: The ios example on the simulator's screen
- FAIL: XCUITest accessibility suite (generated project, FW-10)
- skipped: Maestro flows on the simulator (ci/mobile/maestro/)
- PASS: Screenshot
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android device suite (self-hosted) — skipped

### macOS and iOS (Metal) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Android (compile) — skipped

### Coverage floor — skipped

### checks.sh (Linux) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Windows (D3D12 on WARP) — skipped

### Accessibility gallery + AT-SPI dry-run (Linux) — skipped

### Plugin compatibility matrix (FW-17) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: ios-simulator 
