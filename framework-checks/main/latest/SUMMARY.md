# Framework checks — SUCCESS

- branch: `main`  sha: `c001ad2bf72684e16de21adf50a55d4ac673e284`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41825281151

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| iOS simulator (tests in the sim, a frame on its screen) | FAIL | XCUITest accessibility suite (generated project, FW-10) |
| GPU on software Vulkan (lavapipe) | PASS |  |
| Coverage floor | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Plugin compatibility matrix (FW-17) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| Windows (D3D12 on WARP) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| Android (compile) | skipped |  |
| Accessibility gallery + AT-SPI dry-run (Linux) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
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

### GPU on software Vulkan (lavapipe) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Mesa Vulkan, Xvfb
- PASS: Adapter
- PASS: Vulkan suites (headless)
- PASS: Parity certificate (vulkan-lavapipe, FW-26)
- PASS: Upload the parity certificate (vulkan-lavapipe)
- PASS: 3D GPU parity suite (headless)
- PASS: A GPU frame presented to a real window
- PASS: Platform views under a real X server (FW-20)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Coverage floor — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Plugin compatibility matrix (FW-17) — skipped

### checks.sh (Linux) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Windows (D3D12 on WARP) — skipped

### Android device suite (self-hosted) — skipped

### macOS and iOS (Metal) — skipped

### Android (compile) — skipped

### Accessibility gallery + AT-SPI dry-run (Linux) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: ios-simulator parity-certificate-vulkan-lavapipe 
