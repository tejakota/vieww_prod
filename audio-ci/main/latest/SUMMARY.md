# Audio CI — SUCCESS

- branch: `main`  sha: `ec9ab981a17e5ff74ef36217f953b874d2a61da4`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41392592115

| job | verdict | failed steps |
|---|---|---|
| Audio decoders under cargo-fuzz (nightly, blocking) | PASS |  |
| Audio and music crates (tests + clippy) | PASS |  |

## Steps

### Audio decoders under cargo-fuzz (nightly, blocking) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- PASS: Fuzz every target
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
- PASS: Real-time checks with a static std (AU-18)
- PASS: Notation export, validated (MusicXML XSD, LilyPond)
- PASS: Without the AAC decoder (AU-07, the distributor opt-out)
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
