# 3 · Build — CANCELLED

- branch: `main`  sha: `93e4e8e4f6718d22b319c3ecf4d8fa3fd36418de`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43426720553

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / three · android | FAIL | three · android |
| phase-1 / wright · installers (windows-x64) | FAIL | wright · installers (windows-x64) |
| phase-1 / three · ios | FAIL | Run actions/upload-artifact@v4 |
| phase-1 / viewwstudio · build (linux) | PASS |  |
| phase-1 / snapsearch · android | FAIL | snapsearch · android |
| phase-1 / wright · installers (macos-universal) | CANCELLED |  |
| phase-1 / viewwstudio · build (macos) | CANCELLED |  |
| phase-1 / vavlt · android | FAIL | vavlt · android |
| phase-1 / mytlabs · site | PASS |  |
| phase-1 / upxcale · ios | FAIL | Run actions/upload-artifact@v4 |
| phase-1 / snapsearch · ios | FAIL | Run actions/upload-artifact@v4 |
| phase-1 / viewwsite · site | PASS |  |
| phase-1 / viewwstudio · build (windows) | FAIL | viewwstudio · build (windows) |
| phase-1 / wright · installers (linux-x64) | FAIL | wright · installers (linux-x64) |
| phase-1 / U · apk | PASS |  |
| phase-1 / wrightsite · site | PASS |  |
| phase-1 / vavlt · ios | FAIL | Run actions/upload-artifact@v4 |
| phase-1 / upxcale · android | FAIL | upxcale · android |
| phase-1 / approve | skipped |  |
| phase-1 / matrix.title | skipped |  |
| results / publish | running |  |
| phase-2 | skipped |  |
| phase-3 | skipped |  |

## Steps

### plan / plan — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Manifests valid, dropdowns in sync
- PASS: What runs
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / three · android — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- PASS: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: three · android
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/setup-java@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / wright · installers (windows-x64) — FAIL
- PASS: Set up job
- PASS: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- PASS: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: wright · installers (windows-x64)
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Run actions/cache@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / three · ios — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- skipped: Python for the runner (self-host machines)
- PASS: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: three · ios
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- FAIL: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / viewwstudio · build (linux) — PASS
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- PASS: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: viewwstudio · build (linux)
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / snapsearch · android — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- PASS: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: snapsearch · android
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/setup-java@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / wright · installers (macos-universal) — CANCELLED

### phase-1 / viewwstudio · build (macos) — CANCELLED

### phase-1 / vavlt · android — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- PASS: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: vavlt · android
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/setup-java@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / mytlabs · site — PASS
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- PASS: Run taiki-e/install-action@v2
- PASS: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: mytlabs · site
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / upxcale · ios — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- skipped: Python for the runner (self-host machines)
- PASS: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: upxcale · ios
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- FAIL: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / snapsearch · ios — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- skipped: Python for the runner (self-host machines)
- PASS: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: snapsearch · ios
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- FAIL: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / viewwsite · site — PASS
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- PASS: Run taiki-e/install-action@v2
- PASS: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: viewwsite · site
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / viewwstudio · build (windows) — FAIL
- PASS: Set up job
- PASS: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- PASS: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: viewwstudio · build (windows)
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Run Swatinem/rust-cache@v2
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / wright · installers (linux-x64) — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- PASS: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: wright · installers (linux-x64)
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Run actions/cache@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / U · apk — PASS
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- PASS: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: U · apk
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/setup-java@v4
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / wrightsite · site — PASS
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- PASS: Run taiki-e/install-action@v2
- PASS: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: wrightsite · site
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / vavlt · ios — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- skipped: Python for the runner (self-host machines)
- PASS: Python for the runner (macOS)
- PASS: Plan this job
- skipped: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: vavlt · ios
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- FAIL: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / upxcale · android — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Python for the runner (GitHub's machines)
- PASS: Python for the runner (self-host machines)
- skipped: Python for the runner (macOS)
- PASS: Plan this job
- PASS: Run actions/setup-java@v4
- skipped: Run actions/setup-node@v4
- skipped: Run gradle/actions/setup-gradle@v4
- skipped: Run taiki-e/install-action@v2
- skipped: Run Swatinem/rust-cache@v2
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: upxcale · android
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/setup-java@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / approve — skipped

### phase-1 / matrix.title — skipped

### results / publish — running
- PASS: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Write to the ci-results branch

### phase-2 — skipped

### phase-3 — skipped

- artifacts: U-android-apk 
