# Studio — FAILURE

- branch: `main`  sha: `417f7c492707af104ae423742fc3ed00ae605272`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40674410622

| job | verdict | failed steps |
|---|---|---|
| Studio suite (macos-14) | FAIL | Screenshot, English and Spanish |
| Studio suite (windows-2022) | PASS |  |
| Studio suite (ubuntu-24.04) | PASS |  |

## Steps

### Studio suite (macos-14) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- skipped: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- PASS: The std-only modules with rustc alone
- FAIL: Screenshot, English and Spanish
- PASS: Run actions/upload-artifact@v4
- skipped: Post Run actions/cache@v4
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

- artifacts: studio-screenshots-macos-14 studio-screenshots-ubuntu-24.04 studio-screenshots-windows-2022 
