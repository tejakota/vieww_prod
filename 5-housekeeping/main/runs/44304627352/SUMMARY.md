# 5 · Housekeeping — FAILURE

- branch: `main`  sha: `ee5a473927c2afb59c895056b1e0ac209a3e18c3`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/44304627352

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / vieww · pgo | FAIL | vieww · pgo |
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

### phase-1 / approve — skipped

### phase-1 / matrix.title — skipped

### results / publish — running
- running: Set up job

### phase-2 — skipped

### phase-3 — skipped

- artifacts: 
