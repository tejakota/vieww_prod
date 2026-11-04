# Wright sandbox suites — FAILURE

- branch: `main`  sha: `c001ad2bf72684e16de21adf50a55d4ac673e284`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41832133674

| job | verdict | failed steps |
|---|---|---|
| Linux — full escape suite (Landlock + seccomp) | PASS |  |
| Windows — probe fail-closed (no WSL on the runner) | FAIL | Worker tier report (WR-05) — .cmd shims say tier 1, never silently unsandboxed |
| macOS — Seatbelt suite (WR-03) | PASS |  |
| Windows — doctor --fix machinery + degraded tiers (WR-14) | PASS |  |

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
- PASS: Worker AppContainer (WR-05 tier 2) — escape suite, env, timeout
- FAIL: Worker tier report (WR-05) — .cmd shims say tier 1, never silently unsandboxed
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

- artifacts: 
