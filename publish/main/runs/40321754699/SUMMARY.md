# publish — SUCCESS

- branch: `main`  sha: `35c6b3f65b538866b42166a0d1dce21e8cef3a21`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40321754699

| job | verdict | failed steps |
|---|---|---|
| verify | PASS |  |
| scaffold-external | PASS |  |
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

### scaffold-external — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: System packages
- PASS: Run rustup toolchain install
- PASS: Run Swatinem/rust-cache@v2
- PASS: vieww new, outside the checkout
- PASS: Post Run Swatinem/rust-cache@v2
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
