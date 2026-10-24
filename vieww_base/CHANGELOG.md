# Changelog

All notable changes to the vieww framework, Studio, audio stack and wright.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow the policy in [docs/release/RELEASING.md](docs/release/RELEASING.md)
(pre-1.0: a minor bump may break, a patch never does).

The release job (`.github/workflows/release-gate.yml`, `release`) copies the
section whose heading matches the tag into the GitHub Release notes, so every
tag needs a `## [x.y.z]` section here before it is pushed.

## [Unreleased]

### Added
- **CI on every push and pull request** (FW-02): `pr-quick` (checks --quick,
  actionlint, wright sandbox escape suite), the release gate restored to the
  root, nightly self-hosted hardware runs, audio CI on audio paths.
- **Golden screenshots** (FW-14): 227 committed captures; `goldens` fails a PR
  on any byte difference and uploads the diff report.
- **Supply chain** (FW-23): nightly cargo-deny + cargo-audit for both
  workspaces, CycloneDX SBOMs (`ci/release/sbom.py`, shipped beside the
  installers), a nightly reproducibility check, `SOURCE_DATE_EPOCH` in
  packaging.
- **Publishing** (FW-01): `repository`/`homepage` on all 52 public crates,
  `ci/release/publish-check.sh` + `publish-order.txt`, a `publish` workflow
  (package check, dry-run verify, scaffold-outside-the-checkout build, and
  `cargo publish --workspace` on tags when a token is configured).
- **Releases** (FW-03, FW-06): tags produce a draft GitHub Release with
  installers, merged SHA256SUMS, SBOM, checklist and Sigstore build provenance.
- **Audio that plays** (AU-01, AU-11): `vieww_audio::Engine`, `EnginePlayer`
  (registered by `vieww-platform-winit` on desktop and Android, device opened on
  first play) and `MediaPlayer` (load / play / pause / seek / position /
  volume fades / looping / ended events).

### Security
- wright sandbox (WR-01, WR-02, WR-10, WR-16): PATH parents no longer expose
  `$HOME`; own PID namespace; seccomp-BPF filter; 0700 scratch; macOS
  `secret set` no longer leaks the value in argv; symlink-race-free writes;
  private wright home.
- wright HTTP tools (WR-07): DNS-rebinding closed by pinning the SSRF-checked
  addresses into the connection.
- `clipboard-win` 5.4.1 (yanked) → 5.4.3; `tokio` → 1.54.1 (RUSTSEC-2026-0384,
  RUSTSEC-2026-0386).

### Fixed
- `vieww new` / Studio export outside a checkout wrote `vieww = "0.0.1"`, a
  version that never existed (checklist B16).
- wright: two processes creating the receipt signing key at once failed one
  of them.
- `84-particles-retarget` captured a different picture on every run.

## [0.1.0] - unreleased

Not yet tagged. The first tag cuts from a commit whose release gate is green
on every leg; see docs/release/RELEASING.md.
