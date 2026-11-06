# Hardware (self-hosted Linux and Windows) — CANCELLED

- branch: `main`  sha: `8050dd0ba460ee14d854d417cfcfc2a7011de51f`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42243833499

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Windows hardware | CANCELLED |  |
| macOS hardware | skipped |  |
| Linux hardware | skipped |  |
| Publish to ci-runs/hardware-<branch> | PASS |  |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Windows hardware — CANCELLED
- PASS: Set up job
- PASS: Run actions/checkout@v4
- CANCELLED: Hardware run via Interactive SSH Loopback
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

- artifacts: 
