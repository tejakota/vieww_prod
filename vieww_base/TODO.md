# Remaining work (updated 2026-11-19)

Verified state of each item is in `PENDING.md`; this is the short list.

## Done on 2026-11-19 (main, through the vertex-diet round)

Evidence in `TRACKER.md` and the WORKLOG; the status document this list
follows is *vieww_prod performance plan: status check and to-do plan*
(2026-11-19).

- **The gate is green again** — the rustfmt diff, the WebGPU test vertex,
  the signed APK and the docs links fixed, `macos-15` runners starting the
  Apple legs again (`c7e8056`, `fceec64`): `framework-macos` and
  `framework-ios-simulator` execute, and goldens / web / gate / emulator
  pass.
- **Metal has run** (PENDING §1.2, the first execution): 28/28 on the
  macOS runner's paravirtual GPU — the compositor matrix, partial
  glyph/image/mask/ramp uploads, the idle target trim.
- **D3D12 has run** (PENDING §1.3): the full parity matrix on WARP, 10
  tests, 0 interior violations, worst delta 1/255 (`cb8c641`).
- **The atlas diet and the target trim reach all four backends** (§4.2):
  Metal and D3D12 in `cb8c641`, WebGPU in the same round — partial uploads
  with a carried grow, `TARGET_TRIM_FRAMES`, mock-pinned encodings and a
  browser parity run whose churn pairs must take the region path.
- **A wake that carries nothing draws no frame** (§4.2's idle CPU): the
  wake half of the no-op-present item (`d05bf95`).
- **Release builds carry a trained profile** — the gate trains its own on
  the fixture gallery and exports `-Cprofile-use`; the phone has a path to
  train an aarch64 one (`cf1f61e`).
- **Clipped shadows are quads** — the clip folds into the shadow's
  coverage (one mask where the pass fetched two), so a card list under a
  viewport clip is one geometry run.
- **C-3's leftovers landed fused** (`72fb565`) — the clip walks read rows
  and the gradient skips interiors; the clip fixtures 24% and 23% faster,
  the gallery byte-identical.
- **The vertex diet's second half** (`af1ddaf`): the flat payload every
  vertex of a draw carried — colour, uv, material, params, extra, shape,
  mask — moved behind a per-draw index into a constants table (a storage
  buffer the vertex stage fetches), and the vertex is 20 bytes against the
  plan's 108-byte figure. All four backends' attribute tables rewritten;
  Metal's bounds-check clamp gained the sizes buffer it needed
  (`msl_slots::SIZES_BUFFER`, without which naga silently drops the entry
  point). The lavapipe / macOS / WARP / browser parity legs are the
  pixel-level referee.
- **The compute blur** (`4ab8899`, Vulkan first): `cs_blur` in
  `post.wgsl` — one invocation per scanline, the running sum carried from
  one pixel to the next, one add and one sub per pixel and reads O(1) in
  the radius, where the fragment hybrid still paid `2r + 1` past a
  scanline's start. Built when the device reports RGBA16F storage (else
  the fragment hybrid stays), excluded on direct frames (the swapchain
  image is nobody's storage target), `VIEWW_GPU_COMPUTE_BLUR=0` for the
  A/B. Metal/D3D12/WebGPU keep the fragment blur — their compute
  plumbing is the follow-up this landed the shape for.

## Open

1. **Metal on the iPhone** (PENDING §1.2): a `CAMetalLayer`
   swapchain/drawable path so iOS windows stop presenting through
   MoltenVK; `MTLCounterSampleBuffer` timestamps; then the Device Farm
   numbers (iPhone 14 / 18 Pro Max, GPU time under 8.3 ms at 120 Hz).
2. **D3D12 on a hardware adapter** (PENDING §1.3): run the same suite plus
   `fixtures`/`baseline` on the Kaby Lake laptop's GPU and commit the
   receipt — WARP is correctness, never speed.
3. **The two over-budget fixtures** (PENDING §4.1): AVX2 shipped
   (`3736a4a`, runtime-detected); CI's Linux leg now shows glass at 4.4 ms
   and nested at 4.6 ms — the i5-7200U receipt is what closes it.
4. **The compute blur's own follow-ups**: the ports' compute plumbing —
   WebGPU is done (the browser parity leg is the referee; the env gate is
   `scene_exec::compute_blur_wanted`), **Metal and D3D12 still run the
   fragment hybrid** — and the A73
   stress screen's before/after on Device Farm — 117 ms is the number the
   one-add-one-sub form is owed to halve.
5. **Windows start-up measurement** (§3.5): the 840 → ~250 ms code landed
   (`9b82b2b`); the before/after `baseline` trace on the laptop is owed.
6. **The verification owed on done items** — one Device Farm round (storm
   attribution, `gpu` vs `gpuserial`, small-damage), one laptop round
   (`fixtures --runs 10 --receipt`, `baseline` with `VIEWW_TRACE_FRAMES=1`),
   a first-frame timing for the cached 3D pipelines, and a vertex-bytes
   receipt for the card/list fixtures against the 108-byte figure the
   plan's 50% target names.
7. **Toolchain** — keep the legs on the pinned 1.98.1; the soma upgrade's
   web dispatch has no web build (`apps/soma/ci.toml` is android+ios only).
