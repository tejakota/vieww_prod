# goldens — FAILURE

- branch: `main`  sha: `092c1c2d15aee360789a4b508a2822edb2de7fdd`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41432658093

| job | verdict | failed steps |
|---|---|---|
| shots vs baseline (linux-cpu) | FAIL | Capture and compare |

## Steps

### shots vs baseline (linux-cpu) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: System packages
- PASS: Run rustup toolchain install
- PASS: Run Swatinem/rust-cache@v2
- FAIL: Capture and compare
- skipped: Capture new baseline
- PASS: Upload diff report
- skipped: Upload new baseline
- skipped: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: goldens-diff 
