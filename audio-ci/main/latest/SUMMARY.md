# Audio CI — FAILURE

- branch: `main`  sha: `e37c4ca2069192a1edaab617cc5ecf02635cc41c`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40140174489

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
