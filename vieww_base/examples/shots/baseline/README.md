# Golden baseline (FW-14)

Every feature example photographed by `ci/certify/shot-suite.sh` (release
profile, Linux x86_64, vieww's own CPU rasterizer, embedded fonts first).
`docs/RELEASE-CHECK.md`'s claim is that these bytes are the same on every OS,
so the comparison is exact — no tolerance.

* `.github/workflows/goldens.yml` re-captures on every PR that touches
  rendering code and on every push to main, runs
  `shot-suite.sh --against examples/shots/baseline`, and uploads the
  `shot-diff` report (PNG diffs + index.html) when anything differs.
* A deliberate visual change: run the workflow by hand with `update: true`,
  download the `goldens-new-baseline` artifact, replace this folder with it,
  and commit the PNGs in the same PR as the change — reviewers see the
  before/after in the diff.
* Anything driven by the wall clock must be fixed while
  `feature_harness::capturing()` is true, or the baseline is flaky
  (`84-particles-retarget` was, until its live frame counter was frozen).

`manifest.txt` records the machine the baseline came from.
