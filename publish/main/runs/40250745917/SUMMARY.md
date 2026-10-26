# publish — FAILURE

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40250745917

| job | verdict | failed steps |
|---|---|---|
| check | PASS |  |
| scaffold-external | FAIL | vieww new, outside the checkout |
| verify | PASS |  |
| publish | skipped |  |

## Steps

### check — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Run bash ci/release/publish-check.sh
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### scaffold-external — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: System packages
- PASS: Run rustup toolchain install
- PASS: Run Swatinem/rust-cache@v2
- FAIL: vieww new, outside the checkout
- skipped: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### verify — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: System packages
- PASS: Run rustup toolchain install
- PASS: Run Swatinem/rust-cache@v2
- PASS: Run bash ci/release/publish-check.sh --verify
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### publish — skipped

- artifacts: 
