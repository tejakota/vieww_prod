# supply-chain — SUCCESS

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40249282660

| job | verdict | failed steps |
|---|---|---|
| repro | PASS |  |
| deny + audit (vieww_base/apps/wright) | PASS |  |
| sbom | PASS |  |
| deny + audit (vieww_base) | PASS |  |

## Steps

### repro — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Run bash ci/release/repro-check.sh feature-rectangle
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

### sbom — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run rustup toolchain install
- PASS: Generate
- PASS: Run actions/upload-artifact@v4
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

- artifacts: sbom 
