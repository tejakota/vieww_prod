# main @ e9cff2524beaa6907e7dcf017288c3ab74796a8f

run: https://github.com/tejakota/vieww_prod/actions/runs/41841412883

legs: ios-simulator

- success: Wire check (root ↔ vieww_base)
- failure: iOS simulator (tests in the sim, a frame on its screen)
- skipped: Windows (D3D12 on WARP)
- skipped: Plugin compatibility matrix (FW-17)
- skipped: Accessibility gallery + AT-SPI dry-run (Linux)
- skipped: Coverage floor
- skipped: Audio decoders under cargo-fuzz (nightly)
- skipped: Android device suite (self-hosted)
- skipped: Web (wasm32; Chromium, Firefox, WebKit)
- skipped: Android emulator (APK, device and accessibility suites)
- skipped: Type-check for macOS, iOS, Windows, Android (from Linux)
- skipped: GPU on software Vulkan (lavapipe)
- skipped: Android (compile)
- skipped: checks.sh (Linux)
- skipped: macOS and iOS (Metal)
