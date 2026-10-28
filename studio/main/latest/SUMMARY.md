# Studio — SUCCESS

- branch: `main`  sha: `c40a7185c0632c4fc0a25cafaab06a7eee74107b`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40697873837

| job | verdict | failed steps |
|---|---|---|
| Studio suite (ubuntu-24.04) | PASS |  |
| Studio suite (macos-14) | PASS |  |
| Studio suite (windows-2022) | PASS |  |

## Steps

### Studio suite (ubuntu-24.04) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- PASS: The std-only modules with rustc alone
- PASS: Screenshot, English and Spanish
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Studio suite (macos-14) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- skipped: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- PASS: The std-only modules with rustc alone
- PASS: Screenshot, English and Spanish
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Studio suite (windows-2022) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- skipped: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- skipped: The std-only modules with rustc alone
- PASS: Screenshot, English and Spanish
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: studio-screenshots-windows-2022 studio-screenshots-macos-14 studio-screenshots-ubuntu-24.04 
