# publish — FAILURE

- branch: `main`  sha: `da72ff4877e792412cd6ce94fced20d871cc3321`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40300393235

| job | verdict | failed steps |
|---|---|---|
| verify | PASS |  |
| scaffold-external | FAIL | vieww new, outside the checkout |
| check | PASS |  |
| publish | skipped |  |

## Steps

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

### check — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Run bash ci/release/publish-check.sh
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### publish — skipped

- artifacts: 
