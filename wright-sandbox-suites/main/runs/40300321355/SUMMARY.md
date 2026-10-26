# Wright sandbox suites — FAILURE

- branch: `main`  sha: `da72ff4877e792412cd6ce94fced20d871cc3321`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40300321355

| job | verdict | failed steps |
|---|---|---|
| Windows — probe fail-closed (no WSL on the runner) | FAIL | Worker Job objects (WR-05) — tree killed with the job, process cap |
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| macOS — Seatbelt suite (WR-03) | PASS |  |

## Steps

### Windows — probe fail-closed (no WSL on the runner) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: Sandbox refuses cleanly without WSL2
- PASS: Credential Manager round trip (WR-06)
- FAIL: Worker Job objects (WR-05) — tree killed with the job, process cap
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Linux — full escape suite (Landlock + seccomp) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain (from rust-toolchain.toml)
- PASS: Landlock ABI guard
- PASS: Sandbox escape suite
- PASS: The same suite without user namespaces (the fallback path)
- PASS: Escape suite receipts
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### macOS — Seatbelt suite (WR-03) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: Seatbelt is present
- PASS: The Seatbelt suite (policy + runtime)
- PASS: doctor reports the capability
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
