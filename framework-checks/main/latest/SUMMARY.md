# Framework checks — SUCCESS

- branch: `main`  sha: `e9cff2524beaa6907e7dcf017288c3ab74796a8f`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41841412883

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| iOS simulator (tests in the sim, a frame on its screen) | FAIL | XCUITest accessibility suite (generated project, FW-10) |
| Windows (D3D12 on WARP) | skipped |  |
| Plugin compatibility matrix (FW-17) | skipped |  |
| Accessibility gallery + AT-SPI dry-run (Linux) | skipped |  |
| Coverage floor | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Android (compile) | skipped |  |
| checks.sh (Linux) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
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

### Windows (D3D12 on WARP) — skipped

### Plugin compatibility matrix (FW-17) — skipped

### Accessibility gallery + AT-SPI dry-run (Linux) — skipped

### Coverage floor — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Android device suite (self-hosted) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Android (compile) — skipped

### checks.sh (Linux) — skipped

### macOS and iOS (Metal) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: ios-simulator 
