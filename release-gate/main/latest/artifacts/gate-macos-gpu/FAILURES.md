## Failures, with logs

### `macos-gpu`

#### ❌ G1.3 — certification (gpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: gpu/vulkan-suite (see target/release-gate/macos-gpu/cert/gpu/vulkan-suite.txt)

FAILED=1
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 7 earlier lines omitted
>>> cargo run --release -p test-gpu-work -- target/release-gate/macos-gpu/cert/gpu/workload
>>> cargo run --release -p fixtures -- target/release-gate/macos-gpu/cert/gpu/census-out --census --receipt target/release-gate/macos-gpu/cert/gpu/census-out/receipt.json --shadow target/release-gate/macos-gpu/cert/gpu/census-out/shadow.jsonl
    skipped: desktop/suite — VIEWW_CERT_NO_DESKTOP=1
Vieww macos certification
===========================
COMPLETE=false (skipped stages: 1)
FAILED=1
git_rev=e8f9da976cb8be7d50dac25d7be669627bd52ef8
git_dirty=0
source_digest=git-tree:5245227a2c390cfca094903abbf072f209925b73

gpu/hal-build                PASS
gpu/metal-build              PASS
gpu/metal-backend            BUILD-ONLY(metal does not execute a ScenePlan yet; not certified)
gpu/adapter                  PASS
gpu/device-class             PASS(virtual GPU: Apple Paravirtual device; pixels are evidence, timings are not)
gpu/vulkan-suite             FAIL(exit=101)
gpu/vulkan-validation        INFO(validation layer not installed: apt install vulkan-validationlayers)
gpu/workload                 PASS
gpu/census                   PASS
desktop/suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate/gpu-unsupported-zero    PASS
gate/census-all-complete     PASS
gate/census-no-flat-region-mismatch PASS
exit=1
```

</details>

- certification stage **gpu/vulkan-suite**: `FAIL(exit=101)`

<details><summary>errors in cert/gpu/vulkan-suite.txt</summary>

```text
test gpu_time_is_split_by_the_kind_of_work_that_spent_it ... FAILED

failures:

---- gpu_time_is_split_by_the_kind_of_work_that_spent_it stdout ----

thread 'gpu_time_is_split_by_the_kind_of_work_that_spent_it' (41627) panicked at crates/vieww-hal/tests/vulkan_damage.rs:436:5:
GpuPassTimes { draw: 0ns, layers: 0ns, filters: 0ns, shadows: 0ns, blit: 0ns }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace



failures:

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s

error: test failed, to rerun pass `-p vieww-hal --test vulkan_damage`
exit=101
seconds=96
```

</details>

<details><summary>end of cert/gpu/vulkan-suite.txt</summary>

```text
… 64 earlier lines omitted
test a_backdrop_filter_inside_the_damage_keeps_it ... ok
test a_backdrop_filter_reaching_outside_the_damage_redraws_everything ... ok
test a_damaged_frame_is_the_full_frame_through_blurs_shadows_and_blends ... ok
test a_new_clear_colour_or_size_redraws_everything ... ok
test gpu_time_is_split_by_the_kind_of_work_that_spent_it ... FAILED
test overlapping_damage_rectangles_draw_each_pixel_once ... ok
test pixels_outside_the_damage_are_last_frames ... ok

failures:

---- gpu_time_is_split_by_the_kind_of_work_that_spent_it stdout ----

thread 'gpu_time_is_split_by_the_kind_of_work_that_spent_it' (41627) panicked at crates/vieww-hal/tests/vulkan_damage.rs:436:5:
GpuPassTimes { draw: 0ns, layers: 0ns, filters: 0ns, shadows: 0ns, blit: 0ns }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    gpu_time_is_split_by_the_kind_of_work_that_spent_it

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s

error: test failed, to rerun pass `-p vieww-hal --test vulkan_damage`
exit=101
seconds=96
```

</details>

#### ❌ G1.5 — Vulkan compositor pixel parity (17 tests)

`FAIL(exit=101)`
