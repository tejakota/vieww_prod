# close-open-gaps @ 8cc565fd17faf4978d551b5141a771abf6daa54e

run: https://github.com/tejakota/vieww_prod/actions/runs/41666216725

legs: ios-simulator

- success: Wire check (root ↔ vieww_base)
- failure: iOS simulator (tests in the sim, a frame on its screen)
- skipped: Android device suite (self-hosted)
- skipped: macOS and iOS (Metal)
- skipped: GPU on software Vulkan (lavapipe)
- skipped: Audio decoders under cargo-fuzz (nightly)
- skipped: Android (compile)
- skipped: Coverage floor
- skipped: checks.sh (Linux)
- skipped: Android emulator (APK, device and accessibility suites)
- skipped: Web (wasm32; Chromium, Firefox, WebKit)
- skipped: Windows (D3D12 on WARP)
- skipped: Accessibility gallery + AT-SPI dry-run (Linux)
- skipped: Plugin compatibility matrix (FW-17)
- skipped: Type-check for macOS, iOS, Windows, Android (from Linux)
