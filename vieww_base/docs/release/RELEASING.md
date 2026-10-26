# Releasing vieww

How a version is chosen, cut, checked and shipped (FW-01, FW-03, FW-06).

## Version policy

* One version for the whole workspace (`[workspace.package] version`); every
  public crate (`ci/release/publish-order.txt`) carries it.
* **Pre-1.0:** `0.MINOR.PATCH`. A MINOR bump may break the API; a PATCH never
  does. Pre-releases use `-beta.N` / `-rc.N` (`0.1.0-beta.1`).
* Every release has a `## [x.y.z]` section in `CHANGELOG.md`, written in the
  PR that makes the change (an `[Unreleased]` entry), renamed at release time.
* MSRV (`rust-version`) moves only in a MINOR release and is called out.

## Channels

| Channel | What | Where |
|---|---|---|
| nightly | the last green `release-gate` run on `main` (started by hand) | Actions artifacts, kept 30 days |
| beta | `v0.x.y-beta.N` tags | GitHub Release marked pre-release; crates.io pre-release |
| stable | `v0.x.y` tags | GitHub Release; crates.io |

## Cutting a release

1. Pick the commit: `main`, with `release-gate`, `goldens`, `supply-chain`
   and `hardware` all green on it (every workflow is started by hand —
   Actions → workflow → Run workflow; none runs on push, tag or schedule).
2. Set the version: edit `[workspace.package] version` and the `version =`
   of every `vieww-*` entry in `[workspace.dependencies]`, run
   `ci/release/publish-check.sh`, rename `[Unreleased]` in `CHANGELOG.md`.
   Commit: `release: vX.Y.Z`.
3. Tag and push: `git tag -s vX.Y.Z && git push origin vX.Y.Z`.
4. Start the release workflows on the tag (Actions → workflow → Run
   workflow → "Use workflow from" → the tag; nothing starts on its own):
   * `release-gate`: the full gate on every hosted leg, installers, merged
     checklist, and the `release` job (it acts only when run on a `v*` tag),
     which drafts the GitHub Release with installers + `SHA256SUMS` + SBOM +
     `CHECKLIST-ci.md` and Sigstore build provenance on every file;
   * `publish`: package check, `cargo publish --dry-run`, the
     outside-a-checkout scaffold build, then `cargo publish --workspace` if
     the `CARGO_REGISTRY_TOKEN` secret is set (the tag must equal the version).
5. Run the self-hosted legs on the tag (`hardware.yml` → Run workflow) and
   merge their run folders into the checklist
   (`ci/vieww checklist ... -o CHECKLIST.md`).
6. Read the checklist. Every row PASS or an accepted, written exception →
   publish the draft release.

## Signing

Provenance needs no certificate: `gh attestation verify <file> -R
tejakota/vieww_prod` checks any released file. Platform code signing needs
certificates the owner buys, stored as repository secrets; until they exist,
installers ship unsigned and `MANIFEST.txt` says so on each one.

| Platform | Secrets | Tool |
|---|---|---|
| Windows | `WINDOWS_CERT_PFX` (base64), `WINDOWS_CERT_PASSWORD` | `signtool sign /fd sha256 /tr <rfc3161>` |
| macOS | `APPLE_DEVELOPER_ID_P12`, `APPLE_P12_PASSWORD`, `APPLE_NOTARY_KEY` (API key .p8), `APPLE_NOTARY_KEY_ID`, `APPLE_NOTARY_ISSUER` | `codesign --options runtime`, `xcrun notarytool submit --wait`, `stapler staple` |
| Android | `ANDROID_KEYSTORE` (base64), `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` | `apksigner sign`, AAB via `bundletool` |

`packaging/sign.sh` applies whichever of these are present and verifies the
result (`signtool verify /pa`, `codesign --verify --strict` +
`spctl --assess`, `apksigner verify`); with none present it is a no-op that
says so.
