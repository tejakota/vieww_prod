# Wright sandbox suites — FAILURE

- branch: `main`  sha: `5deb9a10861a26a978d4ba6dc3ce33ab9aa3def2`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40463009428

| job | verdict | failed steps |
|---|---|---|
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | FAIL | Worker AppContainer (WR-05 tier 2) — escape suite, env, timeout |
| macOS — Seatbelt suite (WR-03) | PASS |  |

## Steps

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

### Windows — probe fail-closed (no WSL on the runner) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: Sandbox refuses cleanly without WSL2
- PASS: Credential Manager round trip (WR-06)
- PASS: Worker Job objects (WR-05) — tree killed with the job, process cap
- FAIL: Worker AppContainer (WR-05 tier 2) — escape suite, env, timeout
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
