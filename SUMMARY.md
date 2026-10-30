# main @ 7a9e76951688bed17b8424d7173bd503612b9aa2

run: https://github.com/tejakota/vieww_prod/actions/runs/41005586808

legs: all

- success: checks.sh (Linux)
- success: iOS simulator (tests in the sim, a frame on its screen)
- success: Wire check (root ↔ vieww_base)
- success: macOS and iOS (Metal)
- success: Coverage floor
- success: Type-check for macOS, iOS, Windows, Android (from Linux)
- success: Android (compile)
- success: Android emulator (APK, device and accessibility suites)
- success: GPU on software Vulkan (lavapipe)
- success: Web (wasm32; Chromium, Firefox, WebKit)
- success: Windows (D3D12 on WARP)
- success: Audio decoders under cargo-fuzz (nightly)
- skipped: Android device suite (self-hosted)
