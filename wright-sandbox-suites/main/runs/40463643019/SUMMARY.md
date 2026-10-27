# Wright sandbox suites — SUCCESS

- branch: `main`  sha: `ce6a531a9446250339e84d2e6664a9a6f9c5bee2`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40463643019

| job | verdict | failed steps |
|---|---|---|
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | PASS |  |
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

### Windows — probe fail-closed (no WSL on the runner) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: Sandbox refuses cleanly without WSL2
- PASS: Credential Manager round trip (WR-06)
- PASS: Worker Job objects (WR-05) — tree killed with the job, process cap
- PASS: Worker AppContainer (WR-05 tier 2) — escape suite, env, timeout
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
