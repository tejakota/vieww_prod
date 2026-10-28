# Framework checks — SUCCESS

- branch: `main`  sha: `c40a7185c0632c4fc0a25cafaab06a7eee74107b`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40707519839

| job | verdict | failed steps |
|---|---|---|
| Wire check (root ↔ vieww_base) | PASS |  |
| macOS and iOS (Metal) | PASS |  |
| checks.sh (Linux) | skipped |  |
| Android (compile) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Coverage floor | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| Windows (D3D12 on WARP) | skipped |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
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
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### checks.sh (Linux) — skipped

### Android (compile) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Coverage floor — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Android device suite (self-hosted) — skipped

### Windows (D3D12 on WARP) — skipped

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: parity-certificate-metal-paravirtual 
