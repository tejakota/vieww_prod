# goldens — SUCCESS

- branch: `close-open-gaps`  sha: `32e1431638e708007c7314579f6287173cdbe6bf`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41644702338

| job | verdict | failed steps |
|---|---|---|
| shots vs baseline (linux-cpu) | PASS |  |

## Steps

### shots vs baseline (linux-cpu) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: System packages
- PASS: Run rustup toolchain install
- PASS: Run Swatinem/rust-cache@v2
- PASS: Capture and compare
- skipped: Capture new baseline
- skipped: Upload diff report
- skipped: Upload new baseline
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
