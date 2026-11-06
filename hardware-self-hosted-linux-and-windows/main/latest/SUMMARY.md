# Hardware (self-hosted Linux and Windows) — FAILURE

- branch: `main`  sha: `b22e7c57ba9579a5f549cbfd4f6a89b4cb9a7c47`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42194835380

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Windows hardware | FAIL | Hardware run |
| macOS hardware | skipped |  |
| Linux hardware | skipped |  |
| Publish to ci-runs/hardware-<branch> | PASS |  |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Windows hardware — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- FAIL: Hardware run
- PASS: Upload results
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### macOS hardware — skipped

### Linux hardware — skipped

### Publish to ci-runs/hardware-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Keep the text, drop the pictures
- PASS: Push to ci-runs/hardware-<branch>
- PASS: Complete job

- artifacts: hardware-windows 
