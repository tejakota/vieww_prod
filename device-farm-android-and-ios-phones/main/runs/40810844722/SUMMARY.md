# Device Farm (Android and iOS phones) — FAILURE

- branch: `main`  sha: `8f161fdd0106bc05649e573e034b8f12cf9f5193`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40810844722

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Build the arm64 APK | PASS |  |
| Build the iOS .ipa | skipped |  |
| Device suite on Device Farm iPhones | skipped |  |
| Device suite on Device Farm phones | FAIL | Run on phones |
| Publish to ci-runs/device-farm-<branch> | PASS |  |

## Steps

### Only from main — PASS
- PASS: Set up job
- skipped: Refuse other branches
- PASS: Complete job

### Build the arm64 APK — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run Swatinem/rust-cache@v2
- PASS: Toolchain, target and cargo-apk
- PASS: Run taiki-e/install-action@v2
- PASS: Shipping (dist) APK, with the classes.dex device clouds expect
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Build the iOS .ipa — skipped

### Device suite on Device Farm iPhones — skipped

### Device suite on Device Farm phones — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- PASS: Check the secrets are there
- PASS: Show the OIDC claims AWS will check
- PASS: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- FAIL: Run on phones
- PASS: Upload everything
- PASS: Post AWS credentials (OIDC role)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Publish to ci-runs/device-farm-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Run actions/download-artifact@v4
- PASS: Push to ci-runs/device-farm-<branch>
- PASS: Complete job

- artifacts: device-farm-results vieww-devicefarm-apk 
