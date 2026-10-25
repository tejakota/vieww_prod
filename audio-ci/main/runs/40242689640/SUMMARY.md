# Audio CI — FAILURE

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40242689640

| job | verdict | failed steps |
|---|---|---|
| Audio decoders under cargo-fuzz (nightly, blocking) | FAIL | Fuzz every target |
| Audio and music crates (tests + clippy) | skipped |  |

## Steps

### Audio decoders under cargo-fuzz (nightly, blocking) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- FAIL: Fuzz every target
- PASS: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Audio and music crates (tests + clippy) — skipped

- artifacts: audio-fuzz-findings 
