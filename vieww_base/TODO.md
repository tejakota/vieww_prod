# Remaining work (updated 2026-11-20)

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
- **The compute blur's ports finished** (WebGPU in the web round, Metal and
  D3D12 on 2026-11-20): all four backends now dispatch `cs_blur` per
  scissor band. Metal's `compute_pipeline` and target `ShaderWrite`; D3D12's
  `cs_5_1`, blur root signature (root constants, one SRV table, one UAV
  table), `ALLOW_UNORDERED_ACCESS` targets and the heap's blur-pair region.
  Each suite pins that the pipeline is *built* unless the env opt-out is
  set, and that the compute and fragment forms agree within 2/255 — the
  macOS leg (`MTL_DEBUG_LAYER=1`) and the Windows leg (WARP) are the
  referees.
- **The thorough gate is honestly green** (run 32, `44665288956`): the
  Maestro step's fix — a ten-minute `MAESTRO_DRIVER_STARTUP_TIMEOUT`, an
  honest no-hierarchy warning, one retry when the JUnit report says the
  harness broke — is the last job's receipt; no job in the thorough
  dispatch is red.

## Done on 2026-11-20 – 23 (hardware runs on the laptop and the Mac)

- **Metal present path** (`a57a1ca`): 31/31 on a real Mac, including
  `layer_frames_leave_the_same_pixels_the_readback_draws`.
- **D3D12 on a hardware adapter**: 27/27 (20 scene + 7 smoke) on the
  laptop's Intel HD 620, adapter confirmed `device_type: "hardware"`.
- **Windows presents through D3D12** (`PresentPath::D3d12`, the default
  for a GPU window): Vulkan on the hybrid laptop woke the GeForce 920MX
  (19.3 s cold first frame) and crashed on the Intel GPU
  (`VIEWW_VK_DEVICE=intel`, unmodified code). D3D12 on the Intel:
  first frame 1.74 s, 60.0 fps held to vsync, 9.7% CPU animating, present
  2.5 ms (run 45148678530). Follow-ups landed with it: a refused swapchain
  says why and falls back to the CPU (never Vulkan); CPU frames reuse
  their upload buffer and list; compiled shaders are cached on disk
  (`d3d12::shader_cache`); the device is created beside the window; the
  Vulkan prewarm no longer runs on Windows or iOS.
- **Over-budget counts work, not pacing**: a frame's acquire (the swapchain
  deciding when it may start) no longer makes it "late".
- **Fixtures receipt, 30 runs, i5-7200U (Windows)**: glass 13.9 ms best
  (under budget, was 23–24); gallery 166 ms (was 195).
- **CI**: `6 · Run a command` runs any typed command on any machine.

## Open

1. **The iPhone numbers** (PENDING §1.2): the Metal path is proven on a
   Mac; the iPhone GPU time (under 8.3 ms at 120 Hz) is owed on a real
   iPhone — Firebase Test Lab's only 120 Hz model (iphone14pro) is being
   retired, so Device Farm or a phone on USB (`ci/mobile/ios-app.sh
   --device`).
2. **CPU fixtures still over budget on the i5-7200U**: `00-layers-nested`
   21.0 ms (bandwidth-bound: 16-byte layers — the fix moves bytes and needs
   a tolerance decision), `00-strokes` 25.5 ms and `00-curves` 18.8 ms
   (stroke expansion and flattening — not yet profiled).
3. **Windows GPU-path memory**: 79 MB peak against 40 MB; ~62 MB is in
   place at the first frame (D3D12 runtime + Intel driver). A VMMap /
   heaptrack split of driver vs vieww is the next step. The CPU path is
   31 MB (met).
4. **Windows idle CPU on D3D12**: 3.1% with zero frames (0% on the CPU
   path) — needs a profile (Windows Performance Recorder, idle window).
5. **D3D12 GPU-drawn window**: on the HD 620, damaged frames into the
   retained L[0] page-faulted on the 10th frame (DRED; fault matrix run
   45211604020); full redraws ran clean, so the window path now redraws in
   full (`VIEWW_D3D12_RETAINED=1` for the old path). Owed: the matrix
   re-run confirming it, and the GPU-drawn numbers from it.
6. **The compute blur's follow-ups**: the A73 stress screen's before/after
   on Device Farm (117 ms owed to halve).
7. **Device Farm round** (storm attribution, `gpu` vs `gpuserial`,
   small-damage), a first-frame timing for the cached 3D pipelines, and a
   vertex-bytes receipt for the card/list fixtures.
8. **Toolchain** — keep the legs on the pinned 1.98.1.
