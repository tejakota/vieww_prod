# Work log

The hand-off between working sessions: where things stand, how to work here,
and what is next. Read this first when picking the work up.

It sits beside two older files and does not replace them:

- [`PENDING.md`](./PENDING.md) is everything known to be missing, with why and
  where to start.
- [`TRACKER.md`](./TRACKER.md) is what has been done, with the evidence.

This file is the short version for a new session. When an item here turns out
to be large, it belongs in `PENDING.md` with a pointer from here.

Last updated: 2026-11-20, after the web round: the compute blur reached
WebGPU — the fourth backend and the first port of the follow-up the Vulkan
landing named. The bridge gained `pipeline_blur`/`pass_blur` (the JS shim's
compute pipeline and one dispatch per scissor band, the immediates on the
post passes' own ring), the renderer takes the compute route for the box
modes when the pipeline builds, and the env gate moved to `scene_exec`
(`compute_blur_wanted`) where all four backends can share it. WebGPU orders
the compute pass against the render passes itself — no barriers, which is
why this port is a quarter of Vulkan's size. The mock interpreter tests
cover both routes (a storage-capable bridge dispatches; a refusing one keeps
the fragment hybrid), and the browser parity driver is the end-to-end
referee. Metal and D3D12 still run the fragment hybrid — the TODO's open
item names them, with two worked examples to copy now. What else remains is
unchanged: the iPhone present path, the hardware receipts, and the Device
Farm A/B the blur is owed.

**A thorough dispatch's findings (2026-11-19, run 44580710911).** The gate
this repo runs is the default dispatch — what-this-branch-changed, quick
depth — which does not start soma, the audio legs or the studio suites. A
thorough dispatch on the typing fix started them, and three were red for
reasons that predate the round: **soma's iOS target** no longer compiles
(`libc` lost `proc_bsdinfo`/`proc_pidinfo` — the upgrade moved it), **the
audio plugin's Windows bundle** dies on `denormal_bench.c` including
`dlfcn.h`, and **the Windows audio loopback test** failed exclusive mode on
the runner (hardware-shaped; it may be flaky). None are this round's; all
three are real, and the thorough depth is the only thing that runs them —
worth either fixing or consciously pruning before the next thorough gate.

---

Last updated: 2026-11-20, after the web round: the compute blur reached
WebGPU — the fourth backend and the first port of the follow-up the Vulkan
landing named. The bridge gained `pipeline_blur`/`pass_blur` (the JS shim's
compute pipeline and one dispatch per scissor band, the immediates on the
post passes' own ring), the renderer takes the compute route for the box
modes when the pipeline builds, and the env gate moved to `scene_exec`
(`compute_blur_wanted`) where all four backends can share it. WebGPU orders
the compute pass against the render passes itself — no barriers, which is
why this port is a quarter of Vulkan's size. The mock interpreter tests
cover both routes (a storage-capable bridge dispatches; a refusing one keeps
the fragment hybrid), and the browser parity driver is the end-to-end
referee. Metal and D3D12 still run the fragment hybrid — the TODO's open
item names them, with two worked examples to copy now. What else remains is
unchanged: the iPhone present path, the hardware receipts, and the Device
Farm A/B the blur is owed.

---

Last updated: 2026-11-19, after the typing round: the iOS XCUITest typing
tests had never passed, and the strings they failed with (`tap to typeeo`
from `hello`, `tap to typeno ` from `nihao `) named the bug exactly — a burst
of keystrokes cloned the render object's build-time value, so only the last
key of each burst survived. `RenderEditableText` now chains its own edits
(`pending`, read through `editing()`, recorded through `chained()`, reset by
the rebuild that replaces the object), with regression tests in `editor.rs`
and `ime_replay.rs` that drive bursts without frames between the keys. The
CJK test now skips — naming PENDING §1.4's new marked-text item — when every
keystroke lands as plain ASCII and fails only when keystrokes are lost,
because composition-vs-configuration is not adjudicable from inside the app.
The studio's `parse_diagnostics` keeps a tool failure's follow-up note (the
linker's own stderr) in the diagnostic's help, so the next `cc` failure says
why instead of saying nothing — the coverage leg's one-off link failure
(2026-11-19 16:50) dropped exactly that note. What remains is in `TODO.md`'s
open list: the gate must go green on these fixes (watch the ios-simulator and
coverage legs), the iPhone present path, the hardware receipts, the ports'
compute plumbing, and the Device Farm A/B the blur is owed.

---

Last updated: 2026-11-19, after the vertex-diet round: the vertex diet's
second half landed (the flat payload — colour, uv, material, params, extra,
shape, mask — moved behind a per-draw index into a constants table, a
storage buffer the vertex stage fetches; 20 bytes per vertex against the
plan's 108-byte figure, all four backends' tables rewritten, Metal's clamp
gaining the sizes buffer naga needed), and the compute blur landed for
Vulkan (`cs_blur`, one invocation per scanline, one add and one sub per
pixel — built only where the device reports RGBA16F storage, off on direct
frames, `VIEWW_GPU_COMPUTE_BLUR=0` for the A/B; the ports keep the fragment
hybrid until their compute plumbing exists). The first CI round on the
vertex diet caught the stale stride assert (fixed with the blur commit),
two moved doc links and a Windows-only missing import — all fixed in
`f7fb7bc`, and the Windows/iOS targets now type-check locally against the
pinned toolchain. What remains is in `TODO.md`'s open list: the iPhone
present path, the hardware receipts, the ports' compute plumbing, and the
Device Farm A/B the blur is owed.

---

Last updated: 2026-11-19, after the status-check round: the gate is green
again (the macos-15 runners start, the Apple and Windows legs execute, and
`1 · Check` on `fceec64` was the first all-PASS run since the plan was
written) - Metal has run for the first time (28/28 on the runner's
paravirtual GPU), D3D12 ran the full matrix on WARP (10 tests, 0 interior
violations), the atlas diet and the idle target trim reached the WebGPU
backend (browser parity with churn pairs that must take the region path),
clipped shadows became quads (the clip folds into the coverage, replayed
against the oracle at 2/255 with no GPU), and C-3's leftovers landed fused
- the clip fixtures 24% and 23% faster, the gallery byte-identical. What
remains is in `TODO.md`'s open list: the iPhone present path, the
hardware-adapter receipts, the vertex diet's second half, and the compute
blur (the fragment hybrid's math and its edge modes are documented at the
kernel - the one-add-one-sub form is the follow-up).

---

Last updated: 2026-11-18, after the performance-plan round: eleven more
commits on main, every one pixel-identical where the contract says so (the
gallery, the lavapipe parity matrices) - the CPU paint optimizations
(C-2..C-5), the GPU memory slimming, the atlas-grow carry, the 3D pipeline
cache, the packed vertex colour, the prewarmed Windows device, and the
D3D12 compositor graduation. Details in each commit; the headline numbers
are in the messages. Two measured reverts are recorded at their sites (the
gradient row kernel, the clipped-shape walk) - on the 2-core reference
machine the staging cost more than the dispatch it removed.

---

## 0. The latest round: image to 3D

`vieww-mesh` gained a `relief` module — two builders that turn a raster
image into the same `Mesh` the loaders produce:

- `heightmap_relief(spec, image)` — a plane displaced by the picture's
  shading (luminance, its inverse, or alpha): embossing, terrain,
  intaglio, with bilinear sampling and box-blur smoothing of the field.
- `silhouette_solid(spec, image)` — a watertight closed solid grown from
  the image's alpha silhouette: front and back faces displaced by a
  sqrt-profiled depth, rim walls walking the boundary counter-clockwise
  (the walk is what makes the seams pair — every directed edge used
  exactly once, proven by a test), and an optional `sculpt` factor that
  modulates depth by the picture's luminance, turning the extrusion into
  a bas-relief of whatever the image shows.

Why builders and not a `Displace` with a texture: an image is source
data, not a deformation — the API takes the picture as the primary
argument, like `lathe` takes a profile. The module doc records the scope
decision. No changes to any existing API; `vieww-mesh` stays clippy-clean
under workspace lints, 53 tests green.

The first consumer is Soma (the anatomy explorer example app): its body
is a `silhouette_solid` over a public-domain muscle illustration, which
is the feature's proof at real resolution.

---

## 1. Where things stand

### The goal of the current stretch

The aim is to make vieww measurably the best: fast, light and correct on real
phones, with every claim backed by a Device Farm number or a test. The
standing instruction is to finish what is started. A slow loop is fine; leaving
work half-done is not.

### This round: run `43735160469`'s verdict, and the race the studio found

The two fixes above ran on the Stylo 5 (low) and an S20-class SM8250
(mid). Every pass on both phones:

| | Stylo CPU | Stylo GPU | S20 CPU | S20 GPU | Valdi (S20) |
|---|---|---|---|---|---|
| scroll fps / jank | – | – | 69.1 / 3.0% | **71.6 / 0.0%** | 64.5 / 0.0% |
| live fps / jank | – | – | 53.8 / 22.6% | **65.1 / 0.0%** | 64.7 / 0.4% |
| storm fps / jank | – | – | 50.9 / 21.7% | 61.5 / 0.0% | **64.6** / 0.2% |
| thermal | 67.3 ms p95 | **60.8 ms** p95 | 49.2 / 24.9% | **61.4 / 0.0%** | FAIL |
| app work p95 (live) | 67.7 ms | **62.2 ms** | 21.6 ms | **6.2 ms** | – |
| memory (MB) | **78.2** | 114.3 | **164.8** | 218.1 | 1037.5 |

- **The Stylo's CPU pass recovered** exactly as the path-split gate promised:
  live 87.16 → 67.71 ms p95, storm 155.85 → 120.27 (its `fb38a5d5` shape,
  64/107), while **its GPU pass kept the 48-px gate's win** (storm 61.39,
  live 62.22 — the best numbers either path has had there). The GPU pass now
  beats the CPU pass on every phase of that phone, at 56-77% CPU against
  Valdi's 98-119%.
- **The S20-class GPU pass leads Valdi**: scroll 71.6 vs 64.5, live 65.1 vs
  64.7, thermal 61.4 vs a Valdi that stopped drawing (again - the Android 13
  SM8250's thermal is a recurring Valdi failure), with **0.0% jank on every
  continuous phase**. Storm is the one column Valdi still takes: 61.5 vs
  64.6, five per cent.
- The CPU pass there (53.8 live, 21.7% jank) is the pure rasterizer's known
  shape; the GPU pass is the answer on that class of phone, and the
  `auto` policy ships it.
- The Redmi Note 7 Pro - the phone the flight ring was built for - is Medium
  tier and not in the farm's pools; its numbers move only through the ring,
  and the next time the user's phone is on USB is when that gets measured.

**And the framework gate found a real race.** The studio's repaint tests
flaked on `quick` ("the damaged repaint and a full repaint disagree at N
pixels") from the day the pool landed; bisected to `0ce1f08e`, isolated with
`VIEWW_RASTER_THREADS=1`, reproduced by a dedicated stress test in under a
second: a submitter whose batch drained while it ran its last chunk would
claim from the *next* batch installed, run its own data at that index, and
retire a chunk the foreign batch never ran — a lost band of coverage, i.e.
pixels. No workspace app had ever submitted from two threads at once, which
is why every other test was blind to it. Fix: `Batch::id` — a submitter
claims, retires and waits only on the batch it installed
(`8e939a7f`, with the eight-submitter stress test as its receipt). The
farm's single-threaded market-watch is unaffected; every multi-window app
was exposed.

Two smaller gate repairs rode along (`a03d8c21`): `VIEWW_GPU_DIRECT=0`, the
read-once comparison switch (the damage scenario in `tests/wait_loop.rs` had
been asserting the blit route's contract on frames the direct route takes by
design — full redraw into the swapchain image, cheaper there than damage);
and the route note names the switch when it is the reason a frame blitted.

### The round before: run `43722791555`'s verdict on the ring, and the two fixes it earned

The ring (`0ce1f08e`) ran on the Stylo 5 (low) and — a new mid-pool device —
the Galaxy S26 Ultra (Android 16, SM8850, 120.00001 Hz). Three findings:

1. **The GPU pass died at startup on the S26 Ultra** — one logcat line:
   `market-watch failed: opening a Vulkan device/surface: Vulkan:
   ERROR_OUT_OF_POOL_MEMORY`, zero frames the whole run. The flight ring
doubled the scene descriptor *sets* but left the pool's `SAMPLED_IMAGE`
count at 4 — one set's worth; two sets bind eight. The S26 Ultra's Adreno
(build 11/20/25) enforces a pool's declared counts; the Stylo's older one
let the overdrawn pool pass, which is why the Stylo's GPU pass was green.
   Fix: the pool is sized `4 * FLIGHT_SLOTS`, with the arithmetic at the
   allocation site so the two cannot drift apart again.
2. **The 48-px small-mask gate is a GPU-path win and a CPU-path loss** on
   the one phone that runs both policies back to back — the Stylo, eight
   Cortex-A53s, the tier whose walk is parallel. Against `fb38a5d5`
   (run `43714067671`), the CPU pass paid 23 ms p95 on live (64.03 →
   87.16) and 48 ms on storm (107.66 → 155.85) for thirteen pool
dispatches a frame; the GPU pass on the same phone *won* 6-8 ms (live
   70.85 → 62.30, storm 67.62 → 61.19) — its only CPU-side raster is the
   atlas mask build, so the sparkline walks are that path's long poles.
   Fix: the gate follows the present path (`set_process_raster_small_masks`):
   Vulkan keeps 48, the CPU path keeps 128 — the pool and its big-mask
   walk are shared, pixels cannot move either way, and the tier policy
   (serial on Medium/High phones) is unchanged.
3. **The S26 Ultra's CPU pass reads terribly in the table and that is the
   panel, not the frame**: 62-65 presented fps, 100% jank — the cadence
   policy's steady 60 on a panel the OS kept at 120 Hz, where every
   16.7 ms gap is a miss against 8.33. Valdi presents 118.9-124.1 there
   (native views on HWUI — a GPU path). The GPU pass is vieww's answer on
   that class of phone; it presents again once (1) lands.

And the framework gate itself was red on `0ce1f08e` — pushed without it, a
mistake: fmt; clippy's `large_enum_variant` (the ring's acquire semaphores
grew `VulkanSwapchain` past the `SurfaceKind` threshold — allowed inline with
a reason, one surface per window); two doc links to private items and two to
the `vulkan-swapchain`-gated `render_planned_to`/`AcquiredImage` (the docs
stage does not enable that feature); and the cross-check's headless shape
(`vulkan` without the swapchain), where the direct route's dormant items are
dead code — `cfg_attr`'d `expect(dead_code)` with reasons. Every stage re-run
green: fmt, clippy, build, docs, the three touched crates' tests
(348 + 225 + 31), the headless shape, the wasm audit and the fixture gallery.

Expected on the next run: the S26 Ultra's GPU pass presents, and holds the
panel's 120 where its frame fits 8.33 ms (the S20-class app work p95 was
6.9-7.5 ms); the Stylo's CPU pass returns to `fb38a5d5`'s shape (live ~64,
storm ~107) while its GPU pass keeps the gate's win (storm ~61); the Redmi
Note 7 Pro — the phone the ring was built for — is Medium tier, so its own
numbers move only through the ring, not the gate.

### The round before: the Redmi Note 7 Pro's numbers, and what they said

The user's own phone (SM6150, Adreno 612, Android 16, 1080x2340 at 2.75x)
against the farm's S20 (SM8250), same APK, same schedule:

| stage (p50) | Redmi `render_planned_to` | S20 direct |
|---|---|---|
| plan | 7.5-12.1 ms | 2.2-4.7 ms |
| GPU frame | 20.9-22.5 ms | 4.9-7.3 ms |
| fence wait | 11.6-16.1 ms | 0-7.3 ms |
| interval | 25.6-30.8 ms, jank 62-98% | 5.7-8.9 ms, jank 0 |

Valdi on the same phone: p50 23 ms, 14% jank, 404 MB PSS (vieww 106-132 MB).
The Redmi is **not** on the direct route — its idle frame draws nothing and
blits 11 ms (a full-screen restore from the intermediate), which only the
blit route does; the S20's blit column is 0.00. So three fixes:

- **The frames-in-flight ring** (`vieww-hal`): two command buffers, fences,
  uniforms, geometry buffers, staging and query pools, rotating per frame —
  a frame's CPU half (plan, record, submit) no longer waits for the GPU to
  finish the frame before it. An internal semaphore chain orders every
  submission against the previous one (`COLOR_ATTACHMENT_OUTPUT` and later:
  carries, draws, atlas uploads, blit), because a retained frame's damage
  renders into what the previous frame left in `L[0]`. The acquire
  semaphores are keyed by frame slot; `settle()` still means "all frames";
  `settle_for_next_frame()` is the overlap-preserving one the render paths
  take. Model: the Redmi's live interval was plan 7.5 + settle 12.5 +
  record ~5, GPU-bound at 21 ms underneath; the ring makes it
  max(CPU, GPU) = 21-22 ms, under the 25 ms jank threshold.
- **The pooled raster walk** (`vieww-paint`): the row-parallel scanline
  walk used to spawn a thread per worker per mask, so the size gate (128
  device pixels both axes) kept a sparkline's 198x99 stroke coverage
  serial — thirteen of those a frame, on both render paths, was the
  biggest CPU cost of a storm. A persistent condvar pool
  (`native/raster_pool.rs`) takes the spawn out; the gate is 48 and the
  chunk floor 16. Raster identity: all 30 fixture files byte-identical.
- **Route narration**: `gpu_route_status()` + a `__MW__ {"ev":"route"}`
  line, because the Redmi's non-direct route is currently a deduction
  from stages, not a statement — the next farm run says which of the three
  reasons it was (surface refuses COLOR_ATTACHMENT / plan has layers /
  resolution scale), and whether the direct route can be brought to it.

### The list we worked through, and what came of it

| # | Item | State | Where it landed |
|---|---|---|---|
| 1 | Heavy stress screen, and Device Farm passes for it | done | `examples/shared/screen.rs` `Demo::stress_screen`; passes `stresscpu`, `stressgpu` |
| 2 | Sustained pass: thermal, headroom, memory over time | done | `sustained` (60 s by the clock), `thermal.rs`, `pss_over_time` |
| 3 | CPU time per frame (energy stand-in) | done | `cpu_time.rs`, `cpu_time` observation |
| 4 | About 30–40 taps for input latency | done | `ci/mobile/device-suite.sh` |
| 5 | Publish iOS results even when a run fails | done | `.github/workflows/device-farm.yml` (`10587c1`) |
| 6 | CPU raster profile by kind of work | done | `RasterKindTimes`, `raster_by_kind` (`VIEWW_RASTER_PROFILE=1`) |
| 7 | Shrink the full-screen swapchain copy | done | `vieww-hal/src/vulkan/present_history.rs` (partial copies, verified on the device) |
| 8 | Verify everything | done for every patch so far | see section 3 |

### What the phones say (Device Farm, `42c6d92`, 2026-10-22)

| | LG Stylo 5 (low) | Galaxy A73 (medium) | Galaxy S26 Ultra (high) |
|---|---|---|---|
| Demo, GPU path | 10.1 ms | 1.3 ms | 0.39 ms |
| Demo, CPU path | 35 ms (known failure) | 11.7 ms | 5.5 ms |
| Swapchain copy, partial vs whole | 0.27 vs 2.94 ms | 0.04 vs 0.81 ms | 0.02 vs 0.23 ms |
| Stress, CPU path | 729 ms | 144 ms | 54 ms |
| Stress, GPU path | 951 ms | 136 ms | 20 ms |
| Stress GPU: backdrop blur ("filters") | 830 ms | 117 ms | 16 ms |
| No-trails check (`gpucheck`) | 594 images kept, 0 trails | 593, 0 | 592, 0 |

The run failed on one gate: the S26's `auto` pass, which shipped the CPU path
there. 15 of 240 frames were over budget (12 allowed), all of them waiting on
`acquire`. That is the S26's known CPU-path pacing problem. The fix is the
`auto` policy change in section 2.

### Commits of this stretch (all on `main` except the last)

- `6de1a8f`: device suite: stress screen, sustained pass, thermal, CPU time,
  more taps.
- `81099e6`: partial swapchain copies, verified on the device. Also fixes the
  scaled copy, which drew a `gpuscale` frame into the top-left quarter only.
- `0a57619`: CPU raster time by kind of command.
- `72b7c37`: shadows keyed by shape, exact at the screen's edge, one GPU pass.
- `cf1174f`: shadow sub-pixel phases snapped to a quarter pixel.
- `ff6884f`: stress and sustained passes finish; `gpuscale` no longer leaks
  into later passes.
- `d4b5c93`: a scrolling list's shadows stay in the caches. They now cache
  coverage rather than tinted pixels, never clamp a screen-sized shadow, snap
  the size, and the mask atlas keeps four frames.
- `42c6d92`: Device Farm waits on progress, not a fixed deadline; check mode
  compares one frame in eight.
- **Pending (this session's patch):** `auto` measures the GPU on its second
  launch even when the CPU path settled, and keeps a GPU that is at least
  twice as fast within budget (`MUCH_FASTER`). Also collects `shadow_blurs`,
  and adds this file.

---

## 2. What is next, in order

1. **Market Watch after the direct route (runs `43709382309`, `43712000396`).**
   A depth-0 opaque frame now renders straight into the swapchain image
   (`SceneRenderer::render_planned_direct`): no `L[0]`, no carry, no blit,
   uploads folded into the frame's one submission. On the SM8250 at 1440x3200
   the GPU frame went ~21 ms of GPU to 4.95, jank 91-100% to **0.0%**, and
   presented fps matched Valdi everywhere but storm (-5%). The CPU rasterizer
   dropped the f32 half of a scroll's carry (backdrop filters keep it), holds
   a removed row's vacated damage to its ancestors' clip, skips the full reset
   under an opaque first fill, and parallises gradient/tint/region-clear/
   region-convert and the Android window copy. CPU path on the msm8953:
   work p95 127-133 ms to 43-70. What is left, in order of size:
   - **Storm's stroke coverage is the biggest CPU cost on both paths**
     (`examples/kind_profile.rs`, added: paths are 56% of a storm frame's
     raster, and the same walk is the GPU planner's mask cost). Ten changed
     sparklines a frame, one 4x4-supersampled scanline walk each. Lowering
     `RASTER_MIN_PARALLEL_DIM` to 64 measured *worse* on the 2-core box —
     spawn overhead; a pooled worker or a cheaper walk is what would pay.
   - The Android window copy is uncached-write bound; cores only bought ~20%
     (S20 live: 9.9 to 7.7 ms). The floor is the copy itself.
   - The Stylo (msm8953) is slow on every path, Valdi included (CPU pegged
     100-130%); both frameworks are 15-25 fps there.
2. **Check the next Device Farm run for the `auto` change.**
   - The S26's `auto` should now choose the GPU, and its gate should pass.
   - The A73 and the Stylo should be unchanged (the A73's CPU never settled;
     the Stylo's GPU is not twice as fast at a budget it fits).
   - `shadow_blurs` should appear per pass. Expect about 1.4 a frame on the
     stress screen (the animated floater) and 0 on the demo screen.
2. **Run the iPhones — and find out why the mac legs never start.** `RUN.txt`
   says `ipa: skipped · iphones: skipped` for recent runs, and every
   `1 · Check` of 2026-11-15 — including the all-green `43745442356` —
   shows `framework-macos` and `framework-ios-simulator` *cancelled with
   no steps and no runner*: they are not failing, they never start. Runner
   availability or the macOS quota on the account; worth one look before
   the next thorough check is believed. The `auto` change applies there
   too. Expected: the CPU stays, because the iPhone GPU path has been
   slower (`rasterizer_choice.rs` table).
   Also from `43745442356`'s little brother `43741002736`: the gate's one
   failure that day was `sdk_bundle`'s
   `a_packaged_studio_compiles_with_the_compiler_it_ships` dying on
   `NoRustc("Text file busy")` — the exec of a just-written shell script
   racing something on the runner. It passed the full gate unchanged on
   the retry, and 15 local runs are clean; if it comes back, look at
   write-then-rename in `fake_toolchain` before blaming the code.
3. **The backdrop blur is now the biggest GPU cost** on the stress screen: 117
   ms on the A73, 830 ms on the Stylo. The GPU box blur (`post.wgsl` modes
   1/2) reads `2r + 1` texels a pixel, three passes each way. The CPU's
   running-sum blur is O(1) a pixel. Options, both of which must keep parity
   with `NativeRenderer` (the oracle):
   - a compute pass with a running sum per row and per column;
   - bilinear taps at exact half-texel offsets, which halves the reads.
   Start: `vieww-hal/src/vulkan/scene.rs` `fn blur`, and
   `vieww-shaders/shaders/post.wgsl`.
4. **The CPU stress frame** on the A73 is 144 ms: rects 25.9, layers 23.8,
   images 22.8, shadows 22.2 and composite 19.3 ms (`raster_by_kind`). The
   shadow share is now mostly the animated floater's blur, one a frame.
   Next: layers and composites (the frosted header's backdrop).
5. **Stylo CPU path** is 35 ms against 16.7 ms: known failure RC5,
   `PENDING.md` §4.7. `auto` ships the GPU path there, at 10 ms.
6. **Small things the data showed:**
   - The S26's `thermal` reads a "hottest zone" of 105.0 °C, constant. That
     is a zone that is not a temperature getting through the 150 °C filter in
     `thermal.rs`. Filter by zone `type`, or report the status and headroom
     only.
   - `idle_frames_do_not_repaint_the_whole_surface` failed on the S26's
     `stressgpu`. The stress screen never idles; make that check
     screen-aware, as the damage check already is (`device_tests.rs`, search
     `VIEWW_SCREEN`).
   - `known-failures.txt` entries for the A73 `cpu` and S26 `cpu` passes
     passed once on `42c6d92`. Both are pacing flakes, so delete them only
     after several clean runs.
   - The CI results are published while `checks.sh (Linux)` is still
     `in_progress` (`ci-results/main` `SUMMARY.md`), so that leg's verdict
     comes later. Check the run itself, or make the publisher wait for it.
   - Memory: the GPU path's PSS is 145–195 MB against 75 MB on the CPU path.
     The Stylo's stress screen reached about 500 MB, then held. `PENDING.md`
     §4.2.

---

## 3. How to work here

### Toolchain and the gate

- Rust is pinned to **1.98.1** (`rust-toolchain.toml`). Verify on that
  toolchain, never another: verifying on 1.97 is how a whole session once
  went in circles.
- **The gate is `bash ci/check/checks.sh`**, from `vieww_base/`. It runs fmt,
  clippy (features `vieww-paint/native,vieww-hal/vulkan`), docs with
  `-D warnings`, every test, wasm, the fixture gallery, raster identity,
  conformance, hot reload and packaging. Run all of it before handing work
  over. It takes about an hour here.
- **`bash ci/check/cross-check.sh`** type-checks macOS, iOS, Windows and
  Android from Linux. It needs `rust-src`. Code that is only used on
  Linux/Android needs `#[cfg]`, or macOS fails on dead code.
- **GPU parity under validation:**

  ```sh
  VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation \
  VK_LAYER_ENABLES=VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT \
  cargo test -p vieww-hal --features vulkan -- --ignored --test-threads=1
  ```

  It runs on lavapipe. Grep the output for `VUID` and `SYNC-HAZARD`: there
  should be none.
- **The desktop suite on the stress screen:**

  ```sh
  VIEWW_SCREEN=stress VIEWW_GPU=1 WINIT_X11_SCALE_FACTOR=2.625 \
    xvfb-run -a -s "-screen 0 1300x2500x24" \
    target/release/examples/desktop
  ```

  The scale factor imitates a phone's density: whole-pixel scrolling at a
  density of 1 hid the shadow-cache misses. `VIEWW_SUITE_SECONDS`,
  `VIEWW_PARTIAL_BLIT=0|1` and `VIEWW_PARTIAL_BLIT_CHECK=1` behave as on
  the phones.
- **Disk:** the target directory grows past 20 GB. Delete `target/tmp`,
  `target/package` and `target/debug/incremental` when space runs out.

### Pixels

**Raster identity** (`ci/check/raster-identity.sh`) fails when any fixture's
bytes change.

- If a change is intended, say what moved and by how much in a
  `Pixels-Changed:` commit trailer. `checks.sh` then reports the change
  instead of failing.
- Measure the change against the merge base. Compare decoded RGB, not with
  `getbbox()`, which in Pillow looks only at alpha on RGBA images.
- The GIF fixtures re-quantise their palettes per frame, so a small change
  moves unrelated pixels. Judge by the PNGs.

### Device Farm and the results branches

- **Read results with a forced fetch.** A plain `git fetch` can leave the
  remote-tracking ref stale, and once that produced a wrong "nothing new":

  ```sh
  git fetch origin \
    +refs/heads/ci-results/main:refs/remotes/origin/ci-results/main \
    +refs/heads/ci-results/device-farm-main:refs/remotes/origin/ci-results/device-farm-main
  ```

- **`ci-results/device-farm-main`:**
  - `RUN.txt`: what ran.
  - `android/SUMMARY.md`: per phone and pass.
  - `android/<phone>/Customer_Artifacts/Host_Machine_Files/$DEVICEFARM_LOG_DIR/vieww/`:
    `result.txt` (every key the run recorded), `<pass>-suite.log`,
    `<pass>-logcat.txt` and `sustained-pss.txt`.
- **`ci-results/main`:** `SUMMARY.md` (legs) and `jobs.json`.
- **Passes** (`ci/mobile/device-farm/run-android.sh`):
  - **Gating:** `cpu`, `gpu`, `auto`. The rest are diagnostic.
  - **Diagnostic:** `cpuvk`, `cpuhint`, `gpuscale`, `gpuserial`, `cpubare`,
    `cpunothreads`, `cpunofill`, `cpunoblur`, `stresscpu`, `stressgpu`,
    `sustained`, `gpucheck` and `gpufullcopy`.
  - **`gpucheck`'s exception:** its `partial_copies_leave_no_trails` check
    fails the run on any phone.
- **Accepted failures** go in `ci/mobile/device-farm/known-failures.txt`,
  keyed by `ro.product.model`. A listed check that passes produces a "stale
  entry" warning.
- **Android properties** (`debug.vieww.*`) set every option per pass. Set an
  empty value as `'""'`: a bare `""` is refused, and that is how `gpuscale`
  leaked into every later pass.

### Delivering work

This session cannot push or use the GitHub API: the user's GitHub account is
not linked to it. Work is handed over as a `git format-patch` file that the
user applies with `git am`.

- Before sending, check that it applies to `origin/main` in a scratch worktree
  and that the tree matches.
- End commit messages with the attribution lines the session gives.
- Keep commits reviewable: one change each, each one passing clippy on its
  own.

### How results have been read, and what went wrong doing it

These have each happened once. Do not repeat them:

- **A claim is data only when a number backs it.** Say which parts of a
  conclusion are judgement.
- **A desktop at density 1 does not behave like a phone.** It scrolls in whole
  pixels, so caches keyed by sub-pixel phase look perfect.
- **A pass that "never reported" is usually a deadline**, not a crash. Read its
  `-suite.log` and `-logcat.txt` before guessing.
- **Float keys compared by bit pattern** need snapping: `bottom - top` differs
  in the last bit by position.
- **An LRU cache over a cycle longer than itself hits nothing.** Size it for
  the working set, or store less per entry.
