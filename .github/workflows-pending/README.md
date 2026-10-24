# Empty — everything has been placed (FW-02, 2026-10-24)

Every workflow that waited here is in `.github/workflows/`, and the stale
copies (older than the live files) were deleted along with `restore.sh`.
Trigger policy now:

| Workflow | Triggers |
|---|---|
| `pr-quick.yml` | every pull request and non-main branch push: `ci/vieww checks --quick` + actionlint (hosted) |
| `release-gate.yml` | push to main (failed rows), `v*` tag (full gate), manual |
| `audio-ci.yml` | push/PR touching audio or music, nightly fuzz, manual |
| `hardware.yml` | nightly on main + manual; **never** push or PR (self-hosted) |
| everything else | manual |

`ABC.md` and `WAY-AHEAD.md` are planning notes kept for the record.
