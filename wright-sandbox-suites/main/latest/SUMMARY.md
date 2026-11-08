# Wright sandbox suites — CANCELLED

- branch: `main`  sha: `b2dae67e9841e5274b39eeef0dece58302ed2ffb`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42483848200

| job | verdict | failed steps |
|---|---|---|
| Windows — doctor --fix machinery + degraded tiers (WR-14) | PASS |  |
| macOS — Seatbelt suite (WR-03) | CANCELLED |  |
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | FAIL | Worker tier report (WR-05) — .cmd shims say tier 1, never silently unsandboxed |

## Steps

### Windows — doctor --fix machinery + degraded tiers (WR-14) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust toolchain
- PASS: The fix plan, matrix and script pin (WR-14a/c)
- PASS: The degraded native tiers (WR-14b) — resolution + vocabulary
- PASS: The bootstrap script parses under Windows PowerShell
- PASS: doctor prints the isolation matrix (WR-14c)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### macOS — Seatbelt suite (WR-03) — CANCELLED

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
- PASS: Worker AppContainer (WR-05 tier 2) — escape suite, env, timeout
- FAIL: Worker tier report (WR-05) — .cmd shims say tier 1, never silently unsandboxed
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
