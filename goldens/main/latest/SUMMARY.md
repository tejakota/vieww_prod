# goldens — SUCCESS

- branch: `main`  sha: `e8f9da976cb8be7d50dac25d7be669627bd52ef8`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40171670535

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
