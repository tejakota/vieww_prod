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
