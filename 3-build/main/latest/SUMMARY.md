# 3 · Build — CANCELLED

- branch: `main`  sha: `b442c6abebae68aa09e654de9d149b554bda00f3`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43570566366

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / wright · installers (linux-x64) | CANCELLED |  |
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

### phase-1 / wright · installers (linux-x64) — CANCELLED
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
- running: wright · installers (linux-x64)
- running: Android emulator
- running: Build provenance (Sigstore, keyless)
- running: After
- running: Run actions/upload-artifact@v4
- running: Run actions/upload-artifact@v4
- running: Run actions/upload-artifact@v4
- running: Run actions/upload-artifact@v4
- running: Post Run actions/cache@v4
- running: Post Python for the runner (self-host machines)
- running: Post Python for the runner (GitHub's machines)
- running: Post Run actions/checkout@v4

### phase-1 / approve — skipped

### phase-1 / matrix.title — skipped

### results / publish — running
- PASS: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Write to the ci-results branch

### phase-2 — skipped

### phase-3 — skipped

- artifacts: 
