# Hardware (self-hosted Linux and Windows) — SUCCESS

- branch: `main`  sha: `e9cff2524beaa6907e7dcf017288c3ab74796a8f`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41849906938

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Linux hardware | PASS |  |
| Windows hardware | skipped |  |
| Publish to ci-runs/hardware-<branch> | PASS |  |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Linux hardware — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Hardware run
- PASS: Upload results
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Windows hardware — skipped

### Publish to ci-runs/hardware-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Keep the text, drop the pictures
- PASS: Push to ci-runs/hardware-<branch>
- PASS: Complete job

- artifacts: hardware-linux 
