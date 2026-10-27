# Wright sandbox suites — SUCCESS

- branch: `main`  sha: `7cd114dc558ef6eca3feea1d0a9b4d418ec73b85`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40532711825

| job | verdict | failed steps |
|---|---|---|
| macOS — Seatbelt suite (WR-03) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | PASS |  |
| Linux — full escape suite (Landlock + seccomp) | PASS |  |

## Steps

### macOS — Seatbelt suite (WR-03) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: Seatbelt is present
- PASS: The Seatbelt suite (policy + runtime)
- PASS: doctor reports the capability
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

- artifacts: 
