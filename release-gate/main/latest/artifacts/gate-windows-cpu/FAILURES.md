## Failures, with logs

### `windows-cpu`

#### ❌ G1.1 — ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.1.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: items after a test module
   --> examples\wright_film\src\socials.rs:499:1
    |
499 | mod tests {
    | ^^^^^^^^^
...
521 | pub(crate) fn p1_question(ctx: &Ctx) -> WidgetNode {

error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
    Checking feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
    Checking feature-rigging v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\83-rigging)
    Checking feature-split-text v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\77-split-text)

    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)

warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: unresolved link to `crate::ios`
 --> crates\vieww-interaction\src\drag_out_windows.rs:6:28
  |
6 | //! The same standing as [`crate::ios`] on the other end of the platform
  |                            ^^^^^^^^^^ no item named `ios` in module `vieww_interaction`
  |
  = note: `-D rustdoc::broken-intra-doc-links` implied by `-D warnings`

error: could not document `vieww-interaction`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
    Checking vieww-platform-winit v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-platform-winit)
    Checking anstyle-query v1.1.5
    Checking anstyle-wincon v3.0.11

error: unresolved link to `wright_app`
 --> examples\wright_film\src\m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
  |                                                         ^^^^^^^^^^ no item named `wright_app` in scope
  |
  = help: to escape `[` and `]` characters, add '\' before them like `\[` or `\]`

error: could not document `wright_film`
 Documenting feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
 Documenting feature-easing-presets v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\63-easing-presets)
 Documenting feature-switch v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\27-switch)
 Documenting feature-keyframes v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\56-keyframes)
 Documenting feature-locale-formatting v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\51-locale-formatting)
 Documenting feature-grayscale v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\33-grayscale)

error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates\vieww-render-planner\src\policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
   |
   = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates\vieww-render-planner\src\probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
    |                                        ^^^^^^^^^^ this item is private
    |
    = note: this link will resolve properly if you pass `--document-private-items`

… more errors in the full log
```

</details>

<details><summary>end of logs/G1.1.txt</summary>

```text
… 2892 earlier lines omitted
error: unresolved link to `PassKind`
error: unresolved link to `PassKind::Blit`
error: could not document `vieww-hal`

--- tests (workspace): its diagnostics ---
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.pdb
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
warning: `wright-tools` (lib) generated 1 warning
thread 'main' (4240) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`

--- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow): its diagnostics ---

4 stage(s) failed:
  - clippy (workspace, all targets, warnings are errors) (exit 101)
  - docs (workspace, broken intra-doc links are errors) (exit 101)
  - tests (workspace) (exit 101)
  - raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
exit=1
```

</details>

Failed stages in `checks`:

- clippy (workspace, all targets, warnings are errors) (exit 101)
- docs (workspace, broken intra-doc links are errors) (exit 101)
- tests (workspace) (exit 101)
- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)

<details><summary>errors in stage: clippy (workspace, all targets, warnings are errors)</summary>

```text
error: items after a test module
   --> examples\wright_film\src\socials.rs:499:1
    |
499 | mod tests {
error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
    Checking feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)
```

</details>

<details><summary>errors in stage: docs (workspace, broken intra-doc links are errors)</summary>

```text
error: unresolved link to `crate::ios`
 --> crates\vieww-interaction\src\drag_out_windows.rs:6:28
  |
6 | //! The same standing as [`crate::ios`] on the other end of the platform
error: could not document `vieww-interaction`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
    Checking vieww-platform-winit v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-platform-winit)
error: unresolved link to `wright_app`
 --> examples\wright_film\src\m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
error: could not document `wright_film`
 Documenting feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
 Documenting feature-easing-presets v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\63-easing-presets)
 Documenting feature-switch v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\27-switch)
error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates\vieww-render-planner\src\policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates\vieww-render-planner\src\probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates\vieww-render-planner\src\shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
error: could not document `vieww-render-planner`
 Documenting vieww-render-graph v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-render-graph)
 Documenting vieww-hal v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-hal)
 Documenting vector-probe v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\vector-probe)
error: unresolved link to `Step`
 --> crates\vieww-hal\src\vulkan\scene.rs:9:59
  |
9 | //! records **one** command buffer for the plan's whole [`Step`] program, and
error: unresolved link to `vieww_3d::scene::Texture::texels`
    --> crates\vieww-hal\src\vulkan\mod.rs:2656:19
     |
2656 |     /// texels ([`vieww_3d::scene::Texture::texels`]), uploaded as f16.
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `SceneOp`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `PostPush`
  |
  = note: the link appears in this line:
error: unresolved link to `mode`
  |
  = note: the link appears in this line:
error: unresolved link to `blend_index`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind::Blit`
  |
  = note: the link appears in this line:
error: could not document `vieww-hal`
 Documenting vieww-shaders v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-shaders)
 Documenting keynote v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\keynote)
 Documenting screenshot v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\screenshot)
    FAILED: docs (workspace, broken intra-doc links are errors) (exit 101)
```

</details>

<details><summary>errors in stage: tests (workspace)</summary>

```text
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

</details>

<details><summary>errors in stage: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow)</summary>

```text
ci/check/raster-identity.sh: line 51: cd: /tmp/tmp.FL6sf3gxSS/base//d/a/vieww_prod/vieww_prod/vieww_base: No such file or directory
raster-identity: the gallery failed to render in /tmp/tmp.FL6sf3gxSS/base//d/a/vieww_prod/vieww_prod/vieww_base (see /tmp/tmp.FL6sf3gxSS/out-base.log)
tail: cannot open '/tmp/tmp.FL6sf3gxSS/out-base.log' for reading: No such file or directory
    FAILED: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
```

</details>

#### ❌ G1.3 — certification (cpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: tests\workspace

FAILED=True
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 20 earlier lines omitted
gpu\hal-build-vulkan         PASS
gpu\device-class             SKIPPED(CPU-only run requested)
gpu\adapter                  SKIPPED(CPU-only run requested)
gpu\vulkan-suite             SKIPPED(CPU-only run requested)
gpu\workload                 SKIPPED(CPU-only run requested)
gpu\census                   SKIPPED(CPU-only run requested)
gpu\d3d12-build              PASS
gpu\d3d12-backend            BUILD-ONLY(d3d12 does not execute a ScenePlan yet; not certified)
visual\premium               PASS
visual\fixtures              PASS
suites\layout-stress         PASS
suites\animation-stress      PASS
suites\text-fidelity         PASS
suites\image-effects         PASS
suites\scroll-stress         PASS
suites\native-surface        PASS
standard\vieww-standard      PASS
web\baseline                 PASS
web\build                    SKIPPED(wasm32 target, wasm-bindgen or bash unavailable)
web\verify                   SKIPPED(no freshly built wasm to verify)
desktop\suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate\premium-overflows-zero  PASS
gate\standard-passed         PASS
gate\standard-allocations-measured PASS
exit=1
```

</details>

- certification stage **tests/workspace**: `FAIL(exit=101)`

<details><summary>errors in cert/tests/workspace.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

thread 'main' (1760) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
assertion `left == right` failed: parallel rendering allocated
  left: 9
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`
     Running tests\tags.rs (target\debug\deps\tags-d4da70959570a1b1.exe)

error: could not find `Cargo.toml` in `C:\Users\RUNNER~1\AppData\Local\Temp\vieww-cli-cmd-cargo-test` or any parent directory
test tests::cmd_cargo_runs_in_the_given_directory_and_propagates_failure ... ok
test tests::a_nonzero_exit_status_is_propagated ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s

     Running tests\cli.rs (target\debug\deps\cli-26c50449557a57bd.exe)

thread '<unnamed>' (4656) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3333\preview-0001.rs:5:5:
this is what user code does at three in the morning
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test a_panicking_screen_is_caught_at_the_boundary_rather_than_taking_the_host_down ... ok

thread '<unnamed>' (7956) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3334\preview-0001.rs:11:9:
this is what user code does at three in the morning

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=379
```

</details>

<details><summary>end of cert/tests/workspace.txt</summary>

```text
… 8303 earlier lines omitted
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwstudio

running 1 test
test apps\viewwstudio\src\lib.rs - (line 35) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

   Doc-tests wrightsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=379
```

</details>
