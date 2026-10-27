# Audio CI — SUCCESS

- branch: `main`  sha: `eb08375abb17ad0151059f14462cd465df62b265`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40514923801

| job | verdict | failed steps |
|---|---|---|
| Audio and music crates (tests + clippy) | PASS |  |
| Audio decoders under cargo-fuzz (nightly, blocking) | PASS |  |

## Steps

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

### Audio decoders under cargo-fuzz (nightly, blocking) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- PASS: Fuzz every target
- skipped: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
