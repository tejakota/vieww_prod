# Framework checks — FAILURE

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40204349396

| job | verdict | failed steps |
|---|---|---|
| Type-check for macOS, iOS, Windows, Android (from Linux) | PASS |  |
| Windows (D3D12 on WARP) | FAIL | D3D12 suite (WARP when there is no hardware adapter) |
| Wire check (root ↔ vieww_base) | PASS |  |
| Android emulator (APK, device and accessibility suites) | PASS |  |
| Coverage floor | FAIL | Coverage |
| iOS simulator (tests in the sim, a frame on its screen) | PASS |  |
| Audio decoders under cargo-fuzz (nightly) | PASS |  |
| checks.sh (Linux) | FAIL | The gate |
| GPU on software Vulkan (lavapipe) | PASS |  |
| Android (compile) | PASS |  |
| macOS and iOS (Metal) | FAIL | Metal suite on the runner's GPU |
| Web (wasm32; Chromium, Firefox, WebKit) | PASS |  |
| Android device suite (self-hosted) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Type-check for macOS, iOS, Windows, Android (from Linux) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain with std sources
- PASS: Cross type-check
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Windows (D3D12 on WARP) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: platform-check windows
- FAIL: D3D12 suite (WARP when there is no hardware adapter)
- skipped: Clippy for the Windows backends
- skipped: Studio out-of-process preview (PENDING §3.3)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android emulator (APK, device and accessibility suites) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: KVM for the emulator
- PASS: Toolchain, target and cargo-apk
- PASS: Emulator, APK, suites
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Coverage floor — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and cargo-llvm-cov
- PASS: System packages
- PASS: Free disk
- FAIL: Coverage
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### iOS simulator (tests in the sim, a frame on its screen) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and simulator target
- PASS: Boot a simulator
- PASS: Unit tests inside the simulator (simctl-runner)
- PASS: The ios example on the simulator's screen
- PASS: Maestro flows on the simulator (ci/mobile/maestro/)
- PASS: Screenshot
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Audio decoders under cargo-fuzz (nightly) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- PASS: Fuzz every target
- skipped: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### checks.sh (Linux) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: System packages
- PASS: Free disk
- FAIL: The gate
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### GPU on software Vulkan (lavapipe) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Mesa Vulkan, Xvfb
- PASS: Adapter
- PASS: Vulkan suites (headless)
- PASS: 3D GPU parity suite (headless)
- PASS: A GPU frame presented to a real window
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android (compile) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: platform-check android
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### macOS and iOS (Metal) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and iOS targets
- PASS: platform-check macos
- PASS: platform-check ios
- PASS: NEON compositing loops, bit for bit (aarch64)
- PASS: Audio flush-to-zero on aarch64 (FPCR)
- FAIL: Metal suite on the runner's GPU
- skipped: Clippy for the Apple backends
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Web (wasm32; Chromium, Firefox, WebKit) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: wasm-check
- PASS: wasm-bindgen matching Cargo.lock
- PASS: Build the page and the native baseline
- PASS: Playwright with all three engines
- PASS: Canvas parity per engine
- skipped: Readbacks
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android device suite (self-hosted) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: ios-simulator maestro-android 
