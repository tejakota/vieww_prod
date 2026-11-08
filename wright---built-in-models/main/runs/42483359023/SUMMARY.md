# Wright - built-in models — CANCELLED

- branch: `main`  sha: `b2dae67e9841e5274b39eeef0dece58302ed2ffb`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42483359023

| job | verdict | failed steps |
|---|---|---|
| Against llama.cpp and whisper.cpp | FAIL | Make the fixtures |
| Real models on macos-arm64 | CANCELLED |  |
| Real models on windows-x64 | PASS |  |
| Real models on linux-x64 | PASS |  |

## Steps

### Against llama.cpp and whisper.cpp — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Rust 1.98.1
- PASS: Python tools for the test models
- PASS: Run actions/cache@v4
- PASS: Build llama.cpp and whisper.cpp (pinned)
- FAIL: Make the fixtures
- skipped: Tiny models, token for token
- skipped: Download the real models
- skipped: Real models against the references
- skipped: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Real models on macos-arm64 — CANCELLED

### Real models on windows-x64 — PASS
- PASS: Set up job
- PASS: Allow long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Rust 1.98.1
- PASS: Build
- PASS: Doctor (https transport, models)
- PASS: Download and verify the models
- PASS: Listen
- PASS: Ask
- PASS: Tool calls and transcripts (greedy)
- PASS: Timings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Real models on linux-x64 — PASS
- PASS: Set up job
- skipped: Allow long paths (Windows)
- PASS: Run actions/checkout@v4
- PASS: Rust 1.98.1
- PASS: Build
- PASS: Doctor (https transport, models)
- PASS: Download and verify the models
- PASS: Listen
- PASS: Ask
- PASS: Tool calls and transcripts (greedy)
- PASS: Timings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
