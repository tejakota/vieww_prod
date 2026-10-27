# Audio CI — CANCELLED

- branch: `main`  sha: `7cd114dc558ef6eca3feea1d0a9b4d418ec73b85`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40532623497

| job | verdict | failed steps |
|---|---|---|
| Audio decoders under cargo-fuzz (nightly, blocking) | CANCELLED |  |
| Audio and music crates (tests + clippy) | PASS |  |

## Steps

### Audio decoders under cargo-fuzz (nightly, blocking) — CANCELLED
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- CANCELLED: Fuzz every target
- skipped: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Audio and music crates (tests + clippy) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Tests (release)
- PASS: Clippy
- PASS: Without the AAC decoder (AU-07, the distributor opt-out)
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
