# Wright - desktop installers — FAILURE

- branch: `main`  sha: `66e6cbf7fd52b95c5224d58f3bed3d4c6ce53a99`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42839439904

| job | verdict | failed steps |
|---|---|---|
| linux-x64 | FAIL |  |
| macos-universal | CANCELLED |  |
| windows-x64 | PASS |  |
| Update channel tests (WR-04) | FAIL | The signed-manifest suite |
| Publish release | skipped |  |

## Steps

### linux-x64 — FAIL
- PASS: Set up job
- skipped: Allow long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Rust 1.98.1
- PASS: Linux build libraries
- PASS: Run actions/cache@v4
- running: Test (Linux)
- running: Build (Windows, Linux)
- running: Build (macOS, both architectures)
- running: Smoke test the shipped binary
- running: Package (Windows)
- running: Sign (Windows, Authenticode — WR-04)
- running: Package (macOS)
- running: Sign and notarize (macOS, Developer ID — WR-04)
- running: Package (Linux)
- running: Run actions/upload-artifact@v4
- running: Run actions/upload-artifact@v4
- running: Post Run actions/cache@v4
- running: Post Run actions/checkout@v4

### macos-universal — CANCELLED

### windows-x64 — PASS
- PASS: Set up job
- PASS: Allow long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Rust 1.98.1
- skipped: Linux build libraries
- PASS: Run actions/cache@v4
- skipped: Test (Linux)
- PASS: Build (Windows, Linux)
- skipped: Build (macOS, both architectures)
- PASS: Smoke test the shipped binary
- PASS: Package (Windows)
- PASS: Sign (Windows, Authenticode — WR-04)
- skipped: Package (macOS)
- skipped: Sign and notarize (macOS, Developer ID — WR-04)
- skipped: Package (Linux)
- PASS: Run actions/upload-artifact@v4
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Update channel tests (WR-04) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain (from rust-toolchain.toml)
- FAIL: The signed-manifest suite
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Publish release — skipped

- artifacts: screens-windows-x64 wright-1.0.0-windows-x64 
