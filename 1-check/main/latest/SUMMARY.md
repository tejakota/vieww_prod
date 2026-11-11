# 1 · Check — FAILURE

- branch: `main`  sha: `498b17da854667c935b4365e18ab11b41324d149`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43018857689

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / wright · machine | FAIL | wright · machine |
| phase-1 / approve | skipped |  |
| phase-1 / matrix.title | skipped |  |
| phase-2 | skipped |  |
| results / publish | running |  |
| phase-3 | skipped |  |

## Steps

### plan / plan — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Manifests valid, dropdowns in sync
- PASS: What runs
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / wright · machine — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- skipped: Python for the runner (GitHub's machines)
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
- FAIL: wright · machine
- skipped: Android emulator
- skipped: Build provenance (Sigstore, keyless)
- skipped: After
- PASS: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- skipped: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### phase-1 / approve — skipped

### phase-1 / matrix.title — skipped

### phase-2 — skipped

### results / publish — running
- running: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Write to the ci-results branch

### phase-3 — skipped

- artifacts: 
