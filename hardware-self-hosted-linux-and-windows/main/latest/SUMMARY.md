# Hardware (self-hosted Linux and Windows) — FAILURE

- branch: `main`  sha: `a756d575914bdfb9bfd1b78dd8a503c0efe0f0cc`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/42183262840

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Windows hardware | FAIL | Hardware run |
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

### Linux hardware — skipped

### Publish to ci-runs/hardware-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Keep the text, drop the pictures
- PASS: Push to ci-runs/hardware-<branch>
- PASS: Complete job

- artifacts: 
