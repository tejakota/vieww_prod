# Empty — everything has been placed (FW-02, 2026-10-24)

Every workflow that waited here is in `.github/workflows/`, and the stale
copies (older than the live files) were deleted along with `restore.sh`.
Trigger policy now:

Every workflow is started by hand (Actions → workflow → Run workflow), by
the owner's rule of 2026-10-26: no push, pull_request, tag or schedule
trigger anywhere. The one exception is `ci-results.yml`, which runs when a
(manual) run finishes, to write its results to the `ci-results` branch.

`ABC.md` and `WAY-AHEAD.md` are planning notes kept for the record.
