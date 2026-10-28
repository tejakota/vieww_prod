# Device Farm (Android and iOS phones) — FAILURE

- branch: `main`  sha: `068e1efb012e54accf313079739c2053debf8e81`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/40720367260

| job | verdict | failed steps |
|---|---|---|
| Only from main | PASS |  |
| Build the arm64 APK | PASS |  |
| Build the iOS .ipa | PASS |  |
| Device suite on Device Farm phones | FAIL | Check the secrets are there |
| Device suite on Device Farm iPhones | FAIL | AWS credentials (access keys) |
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

### Build the iOS .ipa — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run Swatinem/rust-cache@v2
- PASS: Toolchain and the iOS device target
- PASS: MoltenVK (device slice)
- skipped: Signing identity into a temporary keychain (when the secrets exist)
- PASS: Device .ipa
- PASS: Run actions/upload-artifact@v4
- PASS: Post Run Swatinem/rust-cache@v2
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Device suite on Device Farm phones — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- FAIL: Check the secrets are there
- skipped: Show the OIDC claims AWS will check
- skipped: AWS credentials (OIDC role)
- skipped: AWS credentials (access keys)
- skipped: Run on phones
- PASS: Upload everything
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Device suite on Device Farm iPhones — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run actions/download-artifact@v4
- skipped: AWS credentials (OIDC role)
- FAIL: AWS credentials (access keys)
- skipped: Run on iPhones
- PASS: Upload everything
- PASS: Post AWS credentials (access keys)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Publish to ci-runs/device-farm-<branch> — PASS
- PASS: Set up job
- PASS: Run actions/download-artifact@v4
- PASS: Run actions/download-artifact@v4
- PASS: Push to ci-runs/device-farm-<branch>
- PASS: Complete job

- artifacts: vieww-devicefarm-ipa vieww-devicefarm-apk 
