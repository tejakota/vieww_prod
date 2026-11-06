# Hardware (self-hosted Linux and Windows) — FAILURE

- branch: `main`  sha: `26bbb1f1f04157ab4a05acab3081c416a0615136`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42253300240

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Windows hardware | FAIL | Run actions/checkout@v4 |
| Linux hardware | skipped |  |
| macOS hardware | skipped |  |
| Publish to ci-runs/hardware-<branch> | PASS |  |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Windows hardware — FAIL
- PASS: Set up job
- FAIL: Run actions/checkout@v4
- skipped: Hardware run
- PASS: Upload results
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Linux hardware — skipped

### macOS hardware — skipped

### Publish to ci-runs/hardware-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Keep the text, drop the pictures
- PASS: Push to ci-runs/hardware-<branch>
- PASS: Complete job

- artifacts: 
