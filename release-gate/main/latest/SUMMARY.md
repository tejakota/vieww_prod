# release-gate — SUCCESS

- branch: `main`  sha: `da72ff4877e792412cd6ce94fced20d871cc3321`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40300437716

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| gate (windows-cpu) | PASS |  |
| merged checklist | PASS |  |
| draft GitHub Release | skipped |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run python3 - <<'PY' >> "$GITHUB_OUTPUT"
- PASS: Complete job

### gate (windows-cpu) — PASS
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
- PASS: Release gate
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

- artifacts: checklist-ci screenshots-windows-cpu gate-windows-cpu 
