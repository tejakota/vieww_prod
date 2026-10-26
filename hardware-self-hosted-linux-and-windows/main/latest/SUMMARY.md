# Hardware (self-hosted Linux and Windows) — CANCELLED

- branch: `main`  sha: `f0e7547cea355523b3de5c1c2017de4a432c0bf5`  event: schedule
- run: https://github.com/tejakota/vieww_prod/actions/runs/40241626076

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Linux hardware | CANCELLED |  |
| Windows hardware | CANCELLED |  |
| Publish to ci-runs/hardware-<branch> | FAIL | Keep the text, drop the pictures |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Linux hardware — CANCELLED

### Windows hardware — CANCELLED

### Publish to ci-runs/hardware-<branch> — FAIL
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- FAIL: Keep the text, drop the pictures
- skipped: Push to ci-runs/hardware-<branch>
- PASS: Complete job

- artifacts: 
