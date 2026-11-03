# Framework checks — FAILURE

- branch: `close-open-gaps`  sha: `32e1431638e708007c7314579f6287173cdbe6bf`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41644669392

| job | verdict | failed steps |
|---|---|---|
| checks.sh (Linux) | PASS |  |
| GPU on software Vulkan (lavapipe) | FAIL | 3D GPU parity suite (headless) |
| Wire check (root ↔ vieww_base) | PASS |  |
| Coverage floor | PASS |  |
| Windows (D3D12 on WARP) | PASS |  |
| Accessibility gallery + AT-SPI dry-run (Linux) | PASS |  |
| Android emulator (APK, device and accessibility suites) | PASS |  |
| Audio decoders under cargo-fuzz (nightly) | PASS |  |
| Android (compile) | PASS |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | PASS |  |
| iOS simulator (tests in the sim, a frame on its screen) | FAIL | XCUITest accessibility suite (generated project, FW-10) |
| Plugin compatibility matrix (FW-17) | PASS |  |
| Web (wasm32; Chromium, Firefox, WebKit) | FAIL | WebGPU scene parity per engine (FW-09) |
| macOS and iOS (Metal) | PASS |  |
| Android device suite (self-hosted) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### checks.sh (Linux) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: System packages
- PASS: Free disk
- PASS: The gate
- PASS: Performance budget (advisory, FW-08)
- PASS: Upload performance figures
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### GPU on software Vulkan (lavapipe) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Mesa Vulkan, Xvfb
- PASS: Adapter
- PASS: Vulkan suites (headless)
- PASS: Parity certificate (vulkan-lavapipe, FW-26)
- PASS: Upload the parity certificate (vulkan-lavapipe)
- FAIL: 3D GPU parity suite (headless)
- skipped: A GPU frame presented to a real window
- skipped: Platform views under a real X server (FW-20)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Coverage floor — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and cargo-llvm-cov
- PASS: System packages
- PASS: Free disk
- PASS: Coverage
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Windows (D3D12 on WARP) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: platform-check windows
- PASS: D3D12 suite (WARP when there is no hardware adapter)
- skipped: HLSL that FXC refused (numbered, for the failure report)
- PASS: Parity certificate (d3d12-warp, FW-26)
- PASS: Upload the parity certificate (d3d12-warp)
- PASS: Clippy for the Windows backends
- PASS: Studio out-of-process preview (PENDING §3.3)
- PASS: Crash symbols (PE/PDB, FW-12)
- PASS: Performance budget (advisory, FW-08)
- PASS: Upload performance figures
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Accessibility gallery + AT-SPI dry-run (Linux) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: The gallery audit (VIEWW_AUDIT over every feature example)
- PASS: AT-SPI harness dry-run
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

### Audio decoders under cargo-fuzz (nightly) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- PASS: Fuzz every target
- skipped: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android (compile) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: platform-check android
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Type-check for macOS, iOS, Windows, Android (from Linux) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain with std sources
- PASS: Cross type-check
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

### Plugin compatibility matrix (FW-17) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: The matrix (path deps, current host, last published)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Web (wasm32; Chromium, Firefox, WebKit) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and target
- PASS: wasm-check
- PASS: wasm-bindgen matching Cargo.lock
- PASS: Build the page and the native baseline
- PASS: Playwright with all three engines
- PASS: Canvas parity per engine
- PASS: Web audio player in Chromium (AU-01)
- PASS: WebGPU backend clippy (native, then wasm)
- FAIL: WebGPU scene parity per engine (FW-09)
- PASS: Readbacks
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### macOS and iOS (Metal) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain and iOS targets
- PASS: platform-check macos
- PASS: platform-check ios
- PASS: NEON compositing loops, bit for bit (aarch64)
- PASS: Audio flush-to-zero on aarch64 (FPCR)
- PASS: Metal suite on the runner's GPU
- PASS: Parity certificate (metal-paravirtual, FW-26)
- PASS: Upload the parity certificate (metal-paravirtual)
- PASS: Clippy for the Apple backends
- PASS: Crash symbols (Mach-O, FW-12)
- PASS: Performance budget (advisory, FW-08)
- PASS: Upload performance figures
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Android device suite (self-hosted) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: perf-macos ios-simulator perf-windows perf-linux parity-certificate-d3d12-warp parity-certificate-vulkan-lavapipe parity-certificate-metal-paravirtual maestro-android web-readbacks 
