# release-gate — FAILURE

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: push
- run: https://github.com/tejakota/vieww_prod/actions/runs/40204301835

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| gate (windows-cpu) | FAIL | Release gate |
| merged checklist | PASS |  |
| draft GitHub Release | skipped |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run python3 - <<'PY' >> "$GITHUB_OUTPUT"
- PASS: Complete job

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

### merged checklist — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- PASS: Fill the checklist from both runs
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### draft GitHub Release — skipped

- artifacts: checklist-ci gate-windows-cpu screenshots-windows-cpu 
