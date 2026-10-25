# supply-chain — SUCCESS

- branch: `main`  sha: `e37c4ca2069192a1edaab617cc5ecf02635cc41c`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40146382067

| job | verdict | failed steps |
|---|---|---|
| sbom | PASS |  |
| repro | PASS |  |
| deny + audit (vieww_base) | PASS |  |
| deny + audit (vieww_base/apps/wright) | PASS |  |

## Steps

### sbom — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Generate
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### repro — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Run bash ci/release/repro-check.sh feature-rectangle
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### deny + audit (vieww_base) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run taiki-e/install-action@v2
- PASS: cargo deny (the gate)
- PASS: cargo audit (report)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### deny + audit (vieww_base/apps/wright) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run taiki-e/install-action@v2
- PASS: cargo deny (the gate)
- PASS: cargo audit (report)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: sbom 
