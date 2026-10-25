# release-gate — CANCELLED

- branch: `main`  sha: `e8fc1f712e808f44364b6917e7a52f2f98285a4d`  event: push
- run: https://github.com/tejakota/vieww_prod/actions/runs/40204076196

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| gate (windows-cpu) | CANCELLED |  |
| merged checklist | PASS |  |
| draft GitHub Release | CANCELLED |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run python3 - <<'PY' >> "$GITHUB_OUTPUT"
- PASS: Complete job

### gate (windows-cpu) — CANCELLED
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Make disk space
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: cargo-deny
- PASS: WiX (Windows installer)
- PASS: JDK 17 (Android Gradle Plugin 8)
- CANCELLED: Gradle 8.9 (a scaffolded project has no wrapper JAR)
- skipped: cargo-ndk
- skipped: MinGW-w64 (Windows .exe cross-build from Linux)
- skipped: Android environment
- skipped: Vulkan loader + MoltenVK (macOS; stages skip if no device)
- skipped: Release gate
- PASS: Failures with logs
- running: Screenshots
- running: Upload screenshots
- running: Checks only
- running: Upload run folder
- running: Upload installers
- running: Post Gradle 8.9 (a scaffolded project has no wrapper JAR)
- running: Post JDK 17 (Android Gradle Plugin 8)
- running: Post Run actions/checkout@v4

### merged checklist — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- PASS: Fill the checklist from both runs
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### draft GitHub Release — CANCELLED

- artifacts: 
