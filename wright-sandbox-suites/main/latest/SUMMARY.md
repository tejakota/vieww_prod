# Wright sandbox suites — SUCCESS

- branch: `main`  sha: `35c6b3f65b538866b42166a0d1dce21e8cef3a21`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40321681665

| job | verdict | failed steps |
|---|---|---|
| macOS — Seatbelt suite (WR-03) | PASS |  |
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | PASS |  |

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
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
