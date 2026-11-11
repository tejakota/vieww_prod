# Device · Machines — FAILURE

- branch: `main`  sha: `4b530c1742752defb91ede88805dd0794b567bb8`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43011466669

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| hardware / Only from main | PASS |  |
| feature run | skipped |  |
| release gate (your machines) | skipped |  |
| hardware / Linux hardware | FAIL | Hardware run |
| hardware / Windows hardware | skipped |  |
| hardware / macOS hardware | skipped |  |
| hardware / Publish to ci-runs/hardware-<branch> | PASS |  |
| save results / publish | running |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run python3 .github/entry/plan.py device-machines
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### hardware / Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### feature run — skipped

### release gate (your machines) — skipped

### hardware / Linux hardware — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- FAIL: Hardware run
- PASS: Upload results
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### hardware / Windows hardware — skipped

### hardware / macOS hardware — skipped

### hardware / Publish to ci-runs/hardware-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Keep the text, drop the pictures
- PASS: Push to ci-runs/hardware-<branch>
- PASS: Complete job

### save results / publish — running
- PASS: Set up job
- running: Collect the run
- running: Run actions/checkout@v4
- running: Write to the ci-results branch

- artifacts: hardware-linux 
