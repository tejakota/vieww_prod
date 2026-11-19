# 3 · Build — SUCCESS

- branch: `main`  sha: `8fd6d063cca7129d1e89164cba85ea7a5c693a04`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/44495347627

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / soma · android | PASS |  |
| phase-1 / matrix.title | skipped |  |
| phase-1 / approve | skipped |  |
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

### phase-1 / soma · android — PASS
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
- PASS: soma · android
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

### phase-1 / matrix.title — skipped

### phase-1 / approve — skipped

### results / publish — running
- running: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Write to the ci-results branch

### phase-2 — skipped

### phase-3 — skipped

- artifacts: soma-android-apk 
