# release-gate — CANCELLED

- branch: `main`  sha: `e8f9da976cb8be7d50dac25d7be669627bd52ef8`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40171640088

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| gate (linux-gpu) | CANCELLED |  |
| gate (windows-cpu) | FAIL | Release gate |
| gate (linux-cpu) | CANCELLED |  |
| gate (windows-gpu) | CANCELLED |  |
| gate (macos-cpu) | FAIL | Release gate |
| gate (macos-gpu) | FAIL | Release gate |
| gate (linux-export) | PASS |  |
| merged checklist | PASS |  |
| draft GitHub Release | CANCELLED |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run python3 - <<'PY' >> "$GITHUB_OUTPUT"
- PASS: Complete job

### gate (linux-gpu) — CANCELLED

### gate (windows-cpu) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Make disk space
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: cargo-deny
- PASS: WiX (Windows installer)
- PASS: JDK 17 (Android Gradle Plugin 8)
- PASS: Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: cargo-ndk
- skipped: MinGW-w64 (Windows .exe cross-build from Linux)
- PASS: Android environment
- skipped: Vulkan loader + MoltenVK (macOS; stages skip if no device)
- FAIL: Release gate
- PASS: Failures with logs
- PASS: Screenshots
- PASS: Upload screenshots
- skipped: Checks only
- PASS: Upload run folder
- PASS: Upload installers
- PASS: Post Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: Post JDK 17 (Android Gradle Plugin 8)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### gate (linux-cpu) — CANCELLED

### gate (windows-gpu) — CANCELLED

### gate (macos-cpu) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Make disk space
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: cargo-deny
- skipped: WiX (Windows installer)
- PASS: JDK 17 (Android Gradle Plugin 8)
- PASS: Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: cargo-ndk
- skipped: MinGW-w64 (Windows .exe cross-build from Linux)
- PASS: Android environment
- PASS: Vulkan loader + MoltenVK (macOS; stages skip if no device)
- FAIL: Release gate
- PASS: Failures with logs
- PASS: Screenshots
- PASS: Upload screenshots
- skipped: Checks only
- PASS: Upload run folder
- PASS: Upload installers
- PASS: Post Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: Post JDK 17 (Android Gradle Plugin 8)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### gate (macos-gpu) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Make disk space
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: cargo-deny
- skipped: WiX (Windows installer)
- skipped: JDK 17 (Android Gradle Plugin 8)
- skipped: Gradle 8.9 (a scaffolded project has no wrapper JAR)
- skipped: cargo-ndk
- skipped: MinGW-w64 (Windows .exe cross-build from Linux)
- skipped: Android environment
- PASS: Vulkan loader + MoltenVK (macOS; stages skip if no device)
- FAIL: Release gate
- PASS: Failures with logs
- skipped: Screenshots
- skipped: Upload screenshots
- skipped: Checks only
- PASS: Upload run folder
- skipped: Upload installers
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### gate (linux-export) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Make disk space
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: cargo-deny
- skipped: WiX (Windows installer)
- PASS: JDK 17 (Android Gradle Plugin 8)
- PASS: Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: cargo-ndk
- PASS: MinGW-w64 (Windows .exe cross-build from Linux)
- PASS: Android environment
- skipped: Vulkan loader + MoltenVK (macOS; stages skip if no device)
- PASS: Release gate
- PASS: Failures with logs
- skipped: Screenshots
- skipped: Upload screenshots
- skipped: Checks only
- PASS: Upload run folder
- skipped: Upload installers
- PASS: Post Gradle 8.9 (a scaffolded project has no wrapper JAR)
- PASS: Post JDK 17 (Android Gradle Plugin 8)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### merged checklist — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- PASS: Fill the checklist from both runs
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### draft GitHub Release — CANCELLED

- artifacts: checklist-ci screenshots-windows-cpu gate-windows-cpu gate-linux-export gate-macos-cpu screenshots-macos-cpu gate-macos-gpu 
