# 5 · Housekeeping — FAILURE

- branch: `main`  sha: `d0e59cd6766b4a59585b5184ae7f668cd2352043`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/46139735414

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / vieww · goldens-refresh | PASS |  |
| phase-1 / vieww · pgo | FAIL | vieww · pgo |
| phase-1 / U · models | PASS |  |
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

### phase-1 / vieww · goldens-refresh — PASS
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
- PASS: vieww · goldens-refresh
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / vieww · pgo — FAIL
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
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- FAIL: vieww · pgo
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Post Python for the runner (self-host machines)
- skipped: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / U · models — PASS
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
- skipped: Run actions/cache@v4
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: U · models
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Python for the runner (self-host machines)
- PASS: Post Python for the runner (GitHub's machines)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / approve — skipped

### phase-1 / matrix.title — skipped

### results / publish — running
- running: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Per-app scorecard
- running: Write to the ci-results branch

### phase-2 — skipped

### phase-3 — skipped

- artifacts: goldens-new-baseline 
