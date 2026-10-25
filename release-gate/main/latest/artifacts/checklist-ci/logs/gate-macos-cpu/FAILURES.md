## Failures, with logs

### `macos-cpu`

#### ❌ G1.1 — ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.1.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: items after a test module
   --> examples/wright_film/src/socials.rs:499:1
    |
499 | mod tests {
    | ^^^^^^^^^
...
521 | pub(crate) fn p1_question(ctx: &Ctx) -> WidgetNode {

error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
    Checking criterion v0.8.2
    Checking feature-canvas-dataviz v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/features/87-canvas-dataviz)
    Checking feature-graph-game-collab-lottie v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/features/88-graph-game-collab-lottie)

    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)

warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: unresolved link to `wright_app`
 --> examples/wright_film/src/m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
  |                                                         ^^^^^^^^^^ no item named `wright_app` in scope
  |
  = help: to escape `[` and `]` characters, add '\' before them like `\[` or `\]`

error: could not document `wright_film`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
 Documenting vieww-plugin-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-plugin-macros)
 Documenting vieww-widget-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-widget-macros)
 Documenting vieww-build v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-build)

error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates/vieww-render-planner/src/policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
   |
   = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates/vieww-render-planner/src/probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
    |                                        ^^^^^^^^^^ this item is private
    |
    = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates/vieww-render-planner/src/shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
     |                                   ^^^^^^^^^^^^^^^^^^^ this item is private
     |
     = note: this link will resolve properly if you pass `--document-private-items`

error: could not document `vieww-render-planner`
 Documenting vieww-hardware v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hardware)
 Documenting vieww-hal v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hal)
 Documenting vieww-render-graph v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-render-graph)
 Documenting audit-sheets v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/audit-sheets)
error: unresolved link to `Step`
 --> crates/vieww-hal/src/vulkan/scene.rs:9:59

… more errors in the full log
```

</details>

<details><summary>end of logs/G1.1.txt</summary>

```text
… 2834 earlier lines omitted

--- tests (workspace): its diagnostics ---
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/baseline
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/baseline.dSYM
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/bench
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/bench.dSYM
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/render
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/render.dSYM
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
warning: `wright-tools` (lib) generated 1 warning
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
test rt::tests::promotion_reports_what_the_os_did ... FAILED
---- rt::tests::promotion_reports_what_the_os_did stdout ----
thread 'rt::tests::promotion_reports_what_the_os_did' (92291) panicked at crates/vieww-audio/src/rt.rs:523:9:
error: test failed, to rerun pass `-p vieww-audio --lib`

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
   --> examples/wright_film/src/socials.rs:499:1
    |
499 | mod tests {
error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
    Checking criterion v0.8.2
    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)
```

</details>

<details><summary>errors in stage: docs (workspace, broken intra-doc links are errors)</summary>

```text
error: unresolved link to `wright_app`
 --> examples/wright_film/src/m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
error: could not document `wright_film`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
 Documenting vieww-plugin-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-plugin-macros)
error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates/vieww-render-planner/src/policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates/vieww-render-planner/src/probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates/vieww-render-planner/src/shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
error: could not document `vieww-render-planner`
 Documenting vieww-hardware v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hardware)
 Documenting vieww-hal v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hal)
 Documenting vieww-render-graph v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-render-graph)
error: unresolved link to `Step`
 --> crates/vieww-hal/src/vulkan/scene.rs:9:59
  |
9 | //! records **one** command buffer for the plan's whole [`Step`] program, and
error: unresolved link to `vieww_3d::scene::Texture::texels`
    --> crates/vieww-hal/src/vulkan/mod.rs:2656:19
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
 Documenting vector-probe v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/vector-probe)
 Documenting full-showcase v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/full-showcase)
 Documenting keynote v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/keynote)
    FAILED: docs (workspace, broken intra-doc links are errors) (exit 101)
```

</details>

<details><summary>errors in stage: tests (workspace)</summary>

```text
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/main.rs (target/debug/deps/animation_showcase-7b70550038a68567)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
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
```

</details>

<details><summary>errors in stage: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow)</summary>

```text
    FAILED: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
```

</details>

#### ❌ G1.3 — certification (cpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: tests/workspace (see target/release-gate/macos-cpu/cert/tests/workspace.txt)

FAILED=1
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 51 earlier lines omitted
gpu/metal-build              PASS
gpu/metal-backend            BUILD-ONLY(metal does not execute a ScenePlan yet; not certified)
gpu/device-class             SKIPPED(CPU-only run requested)
gpu/adapter                  SKIPPED(CPU-only run requested)
gpu/vulkan-suite             SKIPPED(CPU-only run requested)
gpu/workload                 SKIPPED(CPU-only run requested)
gpu/census                   SKIPPED(CPU-only run requested)
visual/premium               PASS
visual/fixtures              PASS
suites/layout-stress         PASS
suites/animation-stress      PASS
suites/text-fidelity         PASS
suites/image-effects         PASS
suites/scroll-stress         PASS
suites/native-surface        PASS
standard/vieww-standard      PASS
web/wasm-check               PASS
web/build                    SKIPPED(wasm-bindgen not on PATH)
web/baseline                 PASS
web/verify                   SKIPPED(no freshly built wasm to verify)
desktop/suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate/premium-overflows-zero  PASS
gate/standard-passed         PASS
gate/standard-allocations-measured PASS
exit=1
```

</details>

- certification stage **tests/workspace**: `FAIL(exit=101)`

<details><summary>errors in cert/tests/workspace.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

test shared::tests::animation_advances_while_the_calling_thread_is_blocked ... FAILED

failures:

---- shared::tests::animation_advances_while_the_calling_thread_is_blocked stdout ----

thread 'shared::tests::animation_advances_while_the_calling_thread_is_blocked' (150104) panicked at crates/vieww-animation/src/shared.rs:317:9:
moved while blocked: 100


failures:

test result: FAILED. 324 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.96s

error: test failed, to rerun pass `-p vieww-animation --lib`
     Running unittests src/lib.rs (target/debug/deps/vieww_asset-d74ed72ed0aaa70e)

running 17 tests
test bundle::tests::a_missing_asset_is_not_found_rather_than_unreadable ... ok
test bundle::tests::a_path_that_climbs_out_of_the_bundle_is_refused ... ok
test bundle::tests::a_bundle_error_becomes_a_service_failure_rather_than_unsupported ... ok

test rt::tests::promotion_reports_what_the_os_did ... FAILED

failures:

---- rt::tests::promotion_reports_what_the_os_did stdout ----

thread 'rt::tests::promotion_reports_what_the_os_did' (152043) panicked at crates/vieww-audio/src/rt.rs:523:9:
assertion `left == right` failed
  left: Elevated
 right: Normal


failures:

test result: FAILED. 502 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.88s

error: test failed, to rerun pass `-p vieww-audio --lib`
     Running tests/audit_probes.rs (target/debug/deps/audit_probes-a2dcd1066be22050)

running 7 tests
test probe_cpu_headroom ... ignored, benchmark: timing-dependent, run with --ignored --nocapture
test probe_mix_is_order_independent ... ok
test probe_nan_is_not_propagated_by_mixer ... ok

error: could not find `Cargo.toml` in `/private/var/folders/n6/hn37q6pj7sj_9dmqqc2113800000gn/T/vieww-cli-cmd-cargo-test` or any parent directory
test tests::cmd_cargo_runs_in_the_given_directory_and_propagates_failure ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running tests/cli.rs (target/debug/deps/cli-688d7e8fc8ca6a52)


error: 2 targets failed:
    `-p vieww-animation --lib`
    `-p vieww-audio --lib`
exit=101
seconds=435
```

</details>

<details><summary>end of cert/tests/workspace.txt</summary>

```text
… 8343 earlier lines omitted

   Doc-tests viewwsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwstudio

running 1 test
test apps/viewwstudio/src/lib.rs - (line 35) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s

   Doc-tests wrightsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 2 targets failed:
    `-p vieww-animation --lib`
    `-p vieww-audio --lib`
exit=101
seconds=435
```

</details>

#### ❌ G2.7 — scaffolded Rust + Say projects compile for every target

`FAIL(compile/ios-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | [1m;compile/ios-sim-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | )`

#### ❌ G2.11 — Studio export: iOS simulator .app

`FAIL(export/ios-sim FAIL step 'Build (iOS simulator)' (cargo build --release --target aarch64-apple-ios-sim): exit exit status: 101; last lines:     [1m[94m|[0m | [1m[94m201[0m [1m[94m|[0m             l)`
