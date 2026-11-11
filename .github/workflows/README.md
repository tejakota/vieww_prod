# ViewW GitHub Actions Workflows

Extracted from: `github.com/tejakota/vieww_artifacts`
Source branch: `main` (HEAD at time of export: 2026-10-24)
Reason for exclusion: the PAT used for the migration lacked the `workflow`
scope, so all `.github/workflows/` files were stripped from the history that
was pushed to `vieww_prod`. Everything else was pushed and merged normally.

## Contents (9 files — main branch versions)

| File | Size |
|------|------|
| `ci.yml` | 22.3 KB |
| `framework-checks.yml` | 28.5 KB |
| `device-farm.yml` | 16.0 KB |
| `audio-backends.yml` | 12.1 KB |
| `hardware.yml` | 8.0 KB |
| `audio-ci.yml` | 3.8 KB |
| `wright-release.yml` | 8.8 KB |
| `audio-plugins.yml` | 2.2 KB |
| `wright-models.yml` | 6.6 KB |

## Subfolder: `feature_branch_variants/`

Two workflow files differed between `main` and `feature/main_fixes`:

- `audio-backends.yml`
- `wright-release.yml`

The root files are the `main` versions (the ones that existed on the source
repo's main branch). The subfolder contains the `feature/main_fixes` versions
of those two files for reference — notably, the feature branch predates
main's "Increase timeout for Linux test job to 30 minutes" change, so its
`wright-release.yml` does not set `timeout-minutes: 30` for the Linux test job.

## How to use in vieww_prod

1. Place the 9 `.yml` files into `.github/workflows/` in the `vieww_prod` repo.
2. Pushing them requires a token with `workflow` scope
   (classic PAT: `repo` + `workflow`), e.g.:

   ```bash
   git add .github/workflows/
   git commit -m "Restore GitHub Actions workflows"
   git push origin main
   ```

## Feature runs on the self-hosted machine (`feature-runs.yml`)

The release gate proves a release; `feature-runs` proves a branch. It
diffs the branch against `main` and runs only what the change touches, on
the self-hosted Linux machine:

| Changed | Runs |
|---|---|
| `vieww_base/crates/**` | `ci/vieww checks`, then Wright, Studio and the prototype |
| `vieww_base/apps/wright/**` | clippy `-D warnings` (UI included), tests, UI snapshots |
| `vieww_base/apps/wright/prototypes/**` | the Wright Flow walkthrough GIFs; on `full`, its web build |
| `vieww_base/apps/viewwstudio/**` | Studio's headless release check |
| `vieww_base/ci/**`, `.github/**` | the quick framework checks |

- **Run it:** Actions → feature-runs → Run workflow, pick the branch,
  `areas` (`auto`, or e.g. `wright,prototype`) and `depth` (`quick`/`full`).
- **On every push to a branch (opt in):** set the repository variable
  `FEATURE_RUNS_ON_PUSH` to `true`. Unset, pushes start nothing, which keeps
  the manual-only owner rule as the default. Pull requests never trigger it.
- **Which machine:** repository variable `FEATURE_RUNNER`, a JSON list of
  labels; default `["self-hosted","Linux","X64","vieww-linux"]`.
- **Warm builds:** each area keeps its `target/` under
  `~/.cache/vieww-feature-runs/<area>` (override with `FEATURE_CACHE`), so a
  second run of a branch only rebuilds what changed.
- **Web build on `full`:** needs `rustup target add wasm32-unknown-unknown`
  and the `wasm-bindgen-cli` version the prototype's `Cargo.lock` resolves;
  without them the step is recorded as SKIPPED with the install line.
