# Framework checks — SUCCESS

- branch: `main`  sha: `b359084d3ff19311ea2f1b5d36301a4f16bbc1b2`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40347179209

| job | verdict | failed steps |
|---|---|---|
| Windows (D3D12 on WARP) | PASS |  |
| Wire check (root ↔ vieww_base) | PASS |  |
| iOS simulator (tests in the sim, a frame on its screen) | skipped |  |
| Coverage floor | skipped |  |
| Android emulator (APK, device and accessibility suites) | skipped |  |
| GPU on software Vulkan (lavapipe) | skipped |  |
| Android device suite (self-hosted) | skipped |  |
| macOS and iOS (Metal) | skipped |  |
| Web (wasm32; Chromium, Firefox, WebKit) | skipped |  |
| checks.sh (Linux) | skipped |  |
| Audio decoders under cargo-fuzz (nightly) | skipped |  |
| Type-check for macOS, iOS, Windows, Android (from Linux) | skipped |  |
| Android (compile) | skipped |  |
| Publish results to ci-runs/<branch> | PASS |  |

## Steps

### Windows (D3D12 on WARP) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: platform-check windows
- PASS: D3D12 suite (WARP when there is no hardware adapter)
- skipped: HLSL that FXC refused (numbered, for the failure report)
- PASS: Clippy for the Windows backends
- PASS: Studio out-of-process preview (PENDING §3.3)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Wire check (root ↔ vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: The root file and the vieww_base copy are byte-identical
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### iOS simulator (tests in the sim, a frame on its screen) — skipped

### Coverage floor — skipped

### Android emulator (APK, device and accessibility suites) — skipped

### GPU on software Vulkan (lavapipe) — skipped

### Android device suite (self-hosted) — skipped

### macOS and iOS (Metal) — skipped

### Web (wasm32; Chromium, Firefox, WebKit) — skipped

### checks.sh (Linux) — skipped

### Audio decoders under cargo-fuzz (nightly) — skipped

### Type-check for macOS, iOS, Windows, Android (from Linux) — skipped

### Android (compile) — skipped

### Publish results to ci-runs/<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Collect conclusions and failed logs
- PASS: Push to ci-runs/<branch>
- PASS: Complete job

- artifacts: 
