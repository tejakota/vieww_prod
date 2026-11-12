# 2 · Devices — FAILURE

- branch: `main`  sha: `55c9c2126bb9138c4f24d38d8df38f10ef7a8467`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43271931880

| job | verdict | failed steps |
|---|---|---|
| plan / plan | PASS |  |
| phase-1 / vieww · hardware-macos | FAIL | vieww · hardware-macos |
| phase-1 / approve | skipped |  |
| phase-1 / matrix.title | skipped |  |
| phase-2 / vieww · hardware-report | PASS |  |
| phase-2 / matrix.title | skipped |  |
| phase-2 / approve | skipped |  |
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

### phase-1 / vieww · hardware-macos — FAIL
- PASS: Set up job
- skipped: Long paths (Windows)
- PASS: Run actions/checkout@v4
- skipped: Python for the runner (GitHub's machines)
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
- FAIL: vieww · hardware-macos
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

### phase-2 / vieww · hardware-report — PASS
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
- PASS: Run actions/download-artifact@v4
- skipped: Run actions/download-artifact@v4
- PASS: vieww · hardware-report
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

### phase-2 / matrix.title — skipped

### phase-2 / approve — skipped

### results / publish — running
- running: Set up job

### phase-3 — skipped

- artifacts: hardware-macos 
