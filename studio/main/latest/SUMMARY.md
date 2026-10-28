# Studio — FAILURE

- branch: `main`  sha: `38a389d52a037763fd50a3222aa439db08389849`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40666748935

| job | verdict | failed steps |
|---|---|---|
| Studio suite (macos-14) | FAIL | The std-only modules with rustc alone |
| Studio suite (ubuntu-24.04) | FAIL | The std-only modules with rustc alone |
| Studio suite (windows-2022) | PASS |  |

## Steps

### Studio suite (macos-14) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- skipped: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- FAIL: The std-only modules with rustc alone
- skipped: Screenshot, English and Spanish
- PASS: Run actions/upload-artifact@v4
- skipped: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Studio suite (ubuntu-24.04) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Linux libraries the window code links
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Unit and integration tests
- FAIL: The std-only modules with rustc alone
- skipped: Screenshot, English and Spanish
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

- artifacts: studio-screenshots-windows-2022 
