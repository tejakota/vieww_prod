## Failures, with logs

### `gate-windows-cpu`

#### ❌ G1.1 — ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.1.txt</summary>

```text
FAILED: formatting (exit 1)

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

    FAILED: docs (workspace, broken intra-doc links are errors) (exit 101)

warning[duplicate]: found 2 duplicate entries for crate 'bitflags'
   ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:51:1
51 │ ╭ bitflags 1.3.2 registry+https://github.com/rust-lang/crates.io-index
52 │ │ bitflags 2.13.2 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'font-types'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:214:1
214 │ ╭ font-types 0.11.3 registry+https://github.com/rust-lang/crates.io-index
215 │ │ font-types 0.12.5 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'getrandom'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:229:1
229 │ ╭ getrandom 0.3.4 registry+https://github.com/rust-lang/crates.io-index
230 │ │ getrandom 0.4.3 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'jni'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:243:1
243 │ ╭ jni 0.21.1 registry+https://github.com/rust-lang/crates.io-index
244 │ │ jni 0.22.4 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'jni-sys'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:246:1
246 │ ╭ jni-sys 0.3.1 registry+https://github.com/rust-lang/crates.io-index
247 │ │ jni-sys 0.4.1 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'linux-raw-sys'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:255:1
255 │ ╭ linux-raw-sys 0.4.15 registry+https://github.com/rust-lang/crates.io-index
256 │ │ linux-raw-sys 0.12.1 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'miniz_oxide'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:266:1
266 │ ╭ miniz_oxide 0.8.9 registry+https://github.com/rust-lang/crates.io-index
267 │ │ miniz_oxide 0.9.1 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'objc2'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:279:1
279 │ ╭ objc2 0.5.2 registry+https://github.com/rust-lang/crates.io-index
280 │ │ objc2 0.6.4 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'objc2-app-kit'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:281:1
281 │ ╭ objc2-app-kit 0.2.2 registry+https://github.com/rust-lang/crates.io-index
282 │ │ objc2-app-kit 0.3.2 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'objc2-foundation'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:286:1
286 │ ╭ objc2-foundation 0.2.2 registry+https://github.com/rust-lang/crates.io-index
287 │ │ objc2-foundation 0.3.2 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'read-fonts'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:318:1
318 │ ╭ read-fonts 0.37.0 registry+https://github.com/rust-lang/crates.io-index
319 │ │ read-fonts 0.41.0 registry+https://github.com/rust-lang/crates.io-index


warning[duplicate]: found 2 duplicate entries for crate 'rustc-hash'
    ┌─ D:\a\vieww_prod\vieww_prod\vieww_base/Cargo.lock:327:1
327 │ ╭ rustc-hash 1.1.0 registry+https://github.com/rust-lang/crates.io-index
328 │ │ rustc-hash 2.1.3 registry+https://github.com/rust-lang/crates.io-index


… more errors in the full log
```

</details>

<details><summary>end of logs/G1.1.txt</summary>

```text
… 2684 earlier lines omitted
--- docs (workspace, broken intra-doc links are errors): its diagnostics ---
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\doc\vieww\index.html
error: unresolved link to `crate::ios`
 --> crates\vieww-interaction\src\drag_out_windows.rs:6:28
error: could not document `vieww-interaction`
warning: build failed, waiting for other jobs to finish...

--- tests (workspace): its diagnostics ---
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.pdb
thread 'main' (5272) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`

--- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow): its diagnostics ---

4 stage(s) failed:
  - formatting (exit 1)
  - docs (workspace, broken intra-doc links are errors) (exit 101)
  - tests (workspace) (exit 101)
  - raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
exit=1
```

</details>

Failed stages in `checks`:

- formatting (exit 1)
- docs (workspace, broken intra-doc links are errors) (exit 101)
- tests (workspace) (exit 101)
- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)

<details><summary>errors in stage: formatting</summary>

```text
    FAILED: formatting (exit 1)
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
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
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
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

</details>

<details><summary>errors in stage: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow)</summary>

```text
ci/check/raster-identity.sh: line 51: cd: /tmp/tmp.Q14gALKxB7/base//d/a/vieww_prod/vieww_prod/vieww_base: No such file or directory
raster-identity: the gallery failed to render in /tmp/tmp.Q14gALKxB7/base//d/a/vieww_prod/vieww_prod/vieww_base (see /tmp/tmp.Q14gALKxB7/out-base.log)
tail: cannot open '/tmp/tmp.Q14gALKxB7/out-base.log' for reading: No such file or directory
    FAILED: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
```

</details>

#### ❌ G1.3 — certification (cpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: build\fmt

    FAILED: tests\workspace

FAILED=True
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 21 earlier lines omitted
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

- certification stage **build/fmt**: `FAIL(exit=1)`

<details><summary>end of cert/build/fmt.txt</summary>

```text
… 8 earlier lines omitted
+        "clear" => library::CLEAR_WGSL,
+        _ => library::SCENE_WGSL,
+    };
     let p = ParsedShader::parse(src).unwrap();
     print!("{}", p.to_hlsl("fs_main", ShaderStage::Fragment).unwrap());
 }
Diff in \\?\D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-shaders\src\library.rs:297:
         ] {
             let parsed = ParsedShader::parse(wgsl).expect("library shaders parse");
             for (entry, stage) in parsed.entry_points() {
-                let hlsl = parsed.to_hlsl(&entry, stage).expect("library shaders translate");
+                let hlsl = parsed
+                    .to_hlsl(&entry, stage)
+                    .expect("library shaders translate");
                 let mut at = 0;
                 while let Some(found) = hlsl[at..].find("switch(") {
                     let start = at + found;
Diff in \\?\D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-shaders\src\library.rs:318:
         );
     }
 }
-
 
exit=1
seconds=12
```

</details>

- certification stage **tests/workspace**: `FAIL(exit=101)`

<details><summary>errors in cert/tests/workspace.txt</summary>

```text
thread 'main' (6776) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
assertion `left == right` failed: parallel rendering allocated
  left: 13
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`
     Running tests\tags.rs (target\debug\deps\tags-d4da70959570a1b1.exe)

error: could not find `Cargo.toml` in `C:\Users\RUNNER~1\AppData\Local\Temp\vieww-cli-cmd-cargo-test` or any parent directory
test tests::cmd_cargo_runs_in_the_given_directory_and_propagates_failure ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running tests\cli.rs (target\debug\deps\cli-26c50449557a57bd.exe)


thread '<unnamed>' (8336) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3334\preview-0001.rs:11:9:
this is what user code does at three in the morning
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test a_panicking_build_is_caught_too_rather_than_aborting_the_process ... ok

thread '<unnamed>' (7264) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3333\preview-0001.rs:5:5:
this is what user code does at three in the morning

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=390
```

</details>

<details><summary>end of cert/tests/workspace.txt</summary>

```text
… 8295 earlier lines omitted
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwstudio

running 1 test
test apps\viewwstudio\src\lib.rs - (line 35) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s

   Doc-tests wrightsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=390
```

</details>
