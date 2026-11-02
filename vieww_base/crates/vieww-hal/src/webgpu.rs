//! The WebGPU backend — `scene_exec`'s op stream, driven through a
//! hand-written JavaScript shim to the browser's WebGPU API.
//!
//! FW-09. The web until now had exactly one renderer: the CPU rasterizer
//! presenting finished RGBA8 to a `<canvas>` 2D context. That path is
//! byte-certified on three engines and stays the default — it is the
//! deterministic fallback this backend falls back *to*. What was missing is
//! the GPU path every comparable framework has on the web (Flutter's Skwasm
//! renders through WebGPU/WebGL), which matters for exactly the scenes the
//! CPU rasterizer is weakest at: large animated ones.
//!
//! # The bridge, and why it is not `wasm-bindgen`
//!
//! This crate is a native crate first: it builds for Linux, Windows, macOS
//! and the phone targets, and adding `wasm-bindgen`/`web-sys` edges to it
//! would put JS-glue dependencies into the graph of every non-web consumer
//! of the HAL. The workspace already solved this problem once, for audio:
//! `vieww-audio`'s web backend talks to Web Audio through
//! `web/vieww-audio.js`, a hand-written shim, imported the raw way —
//! `#[link(wasm_import_module = "vieww_audio")]` and `extern "C"` functions,
//! no bindings crate in sight. This module is that pattern, for graphics:
//!
//! * the Rust side declares the [`GpuBridge`] trait below (create targets,
//!   upload atlases, encode passes, draw, submit, read back, present) and,
//!   on wasm, implements it over a raw extern surface whose import module
//!   is `"vieww_gpu_web"`;
//! * `web/vieww-gpu-web.js`, shipped beside this crate's Cargo.toml, holds
//!   the JS side — the `navigator.gpu` calls, `GPUDevice` bookkeeping,
//!   command encoding, the `mapAsync` readback and `getCurrentTexture()`
//!   presentation;
//! * a page maps the import module name `"vieww_gpu_web"` to that file (an
//!   import map or a bundler alias) and instantiates the wasm with it, the
//!   same two steps `vieww-audio.js`'s own header documents.
//!
//! The whole extern surface is `u32` handles and byte slices: no JS values
//! cross the boundary, so nothing here needs a bindings crate, and the same
//! surface would work under any wasm runtime that can supply the module.
//!
//! # Why WebGPU makes the port small
//!
//! The shaders need no translation. `vieww-shaders`' WGSL is authored in
//! WebGPU's own coordinate system (naga flips Y for Vulkan; see
//! `scene.wgsl`'s header), and `scene.wgsl` binds everything through plain
//! `@group/@binding` uniforms and textures — it compiles as-is. Exactly one
//! source needs an adapter: `post.wgsl` declares its immediates block as
//! `var<immediate>`, the WGSL address space for push constants, which
//! Vulkan and Metal (through naga) support and WebGPU does not. The
//! mechanical rewrite — the same struct, in a `var<uniform>` at
//! `@group(1) @binding(0)` — is [`post_wgsl_uniform`], pinned by a test
//! against the file so the two cannot drift silently.
//!
//! The traversal itself is not ported at all: [`frame_passes`] is consumed
//! here exactly as `vulkan::scene`, `metal::scene` and `d3d12::scene`
//! consume it. What a frame *is* has one definition; only the resource
//! management is WebGPU's.
//!
//! # The contract
//!
//! `vulkan::scene`'s, verbatim: targets are premultiplied RGBA16F (`L[n]`,
//! `S[n]` per nesting level, a 1x1 transparent `Dummy`), the four atlases
//! (R8 glyphs, RGBA8 images, R8 masks, RGBA32F ramps) sit at the bindings
//! the scene shader fetches them through, a frame is drawn over a clear
//! colour and read back as straight-alpha RGBA8 through the shared
//! conversion (`scene_exec::straight_rgba8_from_rgba16f`). An incomplete
//! plan is an error, never a frame with holes.
//!
//! # Asynchrony, honestly
//!
//! WebGPU has two asynchronous moments and no synchronous escape hatches:
//! adapter/device bring-up (`requestAdapter`/`requestDevice` are promises)
//! and readback (`mapAsync`). A wasm module is single-threaded and cannot
//! block on a promise. So this backend is a poll-shaped state machine, the
//! way the web platform crate is: [`WebGpuDevice::request`] starts bring-up
//! and [`WebGpuDevice::state`] reports it; [`WebGpuSceneRenderer::render_planned`]
//! submits a frame *and starts its readback*, and
//! [`WebGpuSceneRenderer::readback`] returns the pixels once they land
//! (frames complete in order, so one outstanding readback is enough). The
//! caller — a `requestAnimationFrame` loop, or a certification driver —
//! presents CPU-rasterized frames until the device is ready and the first
//! GPU frame's readback lands: the "auto-select WebGPU when the browser
//! reports it, CPU otherwise" policy in code, the same calibration
//! philosophy as `vieww-platform-winit`'s `rasterizer_choice` — the CPU
//! path is never disabled, it is the fallback.
//!
//! `Device::render_clear_to_pixels` is synchronous by trait, so its
//! implementation is a documented two-call protocol: the first call renders
//! and returns [`WebGpuError::Pending`], a later call returns the pixels.
//!
//! # What is deliberately simpler than Vulkan's
//!
//! * **No swapchain bookkeeping.** Presentation is `getCurrentTexture()`
//!   plus one copy-mode post pass into the canvas texture — there is no
//!   image queue, no acquire/present semaphore dance, no partial-blit
//!   history.
//! * **One frame in flight.** Every submit is followed by its readback;
//!   there is no frame overlap (Vulkan's `set_overlap` exists because its
//!   readback costs a stall — here the browser's own frame pacing is the
//!   scheduler).
//! * **Whole-atlas uploads**, as Metal's port does: a changed atlas version
//!   re-uploads the whole texture.
//! * **Host timings only** ([`FrameTimings`]): no timestamp queries in v1.
//!
//! # Verification
//!
//! The traversal's tests run everywhere (they are `scene_exec`'s).
//! [`post_wgsl_uniform`]'s rewrite is pinned by a unit test, and — because
//! the interpreter is written against [`GpuBridge`] rather than against
//! the extern surface — the *encoding* half is tested natively by
//! [`MockBridge`]: a recording implementation of the bridge that asserts,
//! with no browser and no GPU, that a nested-layer plan produces exactly
//! the pass/draw/post/readback sequence the contract above describes. The
//! real WebGPU path is certified by `examples/webgpu_scene.rs` +
//! `web-test/webgpu_parity.py`: a headless engine renders the same scenes
//! on the CPU rasterizer (in the same wasm) and through this backend, and
//! the frames are compared under the workspace's performance contract
//! (interiors ≤ 2/255, disagreements hugging the reference's own edges —
//! `vieww-test-harness`'s `ParityContract::Performance`). Engines without
//! WebGPU skip, loudly. No number from any of that is a performance claim
//! until it runs on real hardware, which a CI container has none of.

use std::time::{Duration, Instant};

use vieww_foundation::Color;
use vieww_gpu::{PixelRect, Planner, ScenePlan};

use crate::scene_exec::{frame_passes, PostPush, Redrawn, Retained, SceneOp, TargetId};

/// A start mark — `Instant` where the target has one, nothing where it
/// does not: `wasm32-unknown-unknown` has no monotonic clock, and its
/// `Instant::now` panics rather than degrading, so the backend's
/// [`FrameTimings`] are zero on the web target. (The first browser run of
/// the certification driver found this the loud way — a panic named
/// `Instant::now` the moment the second frame began.) Zero is the honest
/// value: "not measured", not "measured as fast".
struct Clock(Option<Instant>);

impl Clock {
    fn start() -> Self {
        Self(cfg!(not(target_arch = "wasm32")).then(Instant::now))
    }

    fn into_elapsed(self) -> Duration {
        self.0.map_or(Duration::ZERO, |started| started.elapsed())
    }
}

/// What this backend needs of whatever executes it — the extern surface's
/// shape, as a trait so the interpreter can be tested natively against
/// [`MockBridge`] and only *wired* to the raw imports on wasm.
///
/// Every method is synchronous: the shim's JS side keeps its own async
/// state (device bring-up, the readback's `mapAsync`) behind the two poll
/// methods, `state`-shaped as `0 pending, 1 ready, 2 failed`.
pub trait GpuBridge {
    /// (Re)create the atlas texture in `slot` (0 glyphs R8, 1 images RGBA8,
    /// 2 masks R8, 3 ramps RGBA32F) at `width x height`; returns its handle.
    fn atlas(&mut self, slot: u32, width: u32, height: u32) -> u32;
    /// Upload `bytes` of atlas texels, row-major, into `slot`'s texture.
    fn atlas_write(&mut self, slot: u32, bytes: &[u8]);
    /// Write the vertex (`which == 0`) or index (`which == 1`) buffer,
    /// recreating it if its size changed.
    fn buffer_write(&mut self, which: u32, bytes: &[u8]);
    /// Create the scene pipeline from WGSL; `Err` if the device refused.
    fn pipeline_scene(&mut self, wgsl: &str) -> Result<(), String>;
    /// Create a post pipeline from WGSL (the uniform-address-space
    /// variant); `over` selects the premultiplied-blend one.
    fn pipeline_post(&mut self, wgsl: &str, over: bool) -> Result<(), String>;
    /// Begin a frame: a command encoder, the viewport uniform for
    /// `width x height`.
    fn frame_begin(&mut self, width: u32, height: u32);
    /// Begin a pass on `target`; `clear` (premultiplied) clears, `None`
    /// loads what the target holds. Ends any pass already open.
    fn pass_begin(&mut self, target: u32, clear: Option<[f32; 4]>);
    /// End the open pass.
    fn pass_end(&mut self);
    /// Narrow the open pass's scissor to `x, y, w, h`.
    fn pass_scissor(&mut self, x: u32, y: u32, w: u32, h: u32);
    /// Bind the scene pipeline, its vertex/index buffers and atlas bind
    /// group.
    fn pass_scene(&mut self);
    /// One indexed draw of `count` indices from `first_index`.
    fn pass_draw(&mut self, first_index: u32, count: u32);
    /// Bind a post pipeline (`over` selects which) with the `src` and
    /// `aux` textures and the 128-byte immediates block.
    fn pass_post(&mut self, src: u32, aux: u32, over: bool, push: &PostPush);
    /// The full-screen post triangle.
    fn pass_post_draw(&mut self);
    /// Clear rectangles (`x, y, w, h` each, already clamped) of the open
    /// target to `color`, between passes — buffer-to-texture copies, the
    /// "no shader, no binding" decision Metal's port made after its
    /// draw-based clear came back all-zero on the macOS CI.
    fn clear_regions(&mut self, rects: &[[u32; 4]], color: &[f32; 4]);
    /// End the frame's open pass, submit, and begin `target`'s readback —
    /// `target` names the texture whose pixels the readback returns, so
    /// the shim never has to guess which handle is the frame.
    fn frame_submit(&mut self, target: u32);
    /// The readback's state: 0 pending, 1 ready, 2 failed.
    fn readback_state(&mut self) -> u32;
    /// Copy the readback (RGBA16F texels, little-endian) into `out`;
    /// returns bytes copied.
    fn readback_take(&mut self, out: &mut [u8]) -> usize;
    /// Present `target` into the `<canvas>` whose id is `canvas`.
    fn present(&mut self, canvas: &str, target: u32) -> Result<(), String>;
    /// A frame-sized premultiplied RGBA16F render target's handle.
    fn target_create(&mut self, width: u32, height: u32) -> u32;
    /// Drop a target or atlas texture by handle.
    fn target_free(&mut self, target: u32);
}

/// What can go wrong on the WebGPU path.
#[derive(Debug, Clone)]
pub enum WebGpuError {
    /// `navigator.gpu` is absent — the CPU canvas path is the answer.
    NoWebGpu,
    /// Bring-up or a readback has not finished; poll again next frame.
    Pending,
    /// The browser refused something (no adapter, device lost, a pipeline
    /// rejected). The message is the browser's own.
    Failed(String),
    /// This module was called in a shape it does not support (a plan too
    /// deep, a present with no frame) — a bug in the caller.
    Invalid(String),
}

impl std::fmt::Display for WebGpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWebGpu => write!(f, "navigator.gpu is absent: use the canvas-CPU backend"),
            Self::Pending => write!(f, "WebGPU is still working: poll again"),
            Self::Failed(message) => write!(f, "WebGPU failed: {message}"),
            Self::Invalid(message) => write!(f, "invalid WebGPU call: {message}"),
        }
    }
}

impl std::error::Error for WebGpuError {}

/// Where one frame's wall time went, on the CPU's clock — the field set
/// `metal::scene`'s `FrameTimings` keeps, for the same reason: this backend
/// has no GPU timestamp queries in v1, so the numbers say where the CPU
/// spent and waited, not where the GPU spent cycles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTimings {
    /// Atlas and geometry uploads, and any target (re)allocation.
    pub upload: Duration,
    /// Building the frame's passes through the bridge.
    pub record: Duration,
    /// Converting the finished frame to straight-alpha RGBA8.
    pub readback: Duration,
}

/// The four atlas slots, in the scene shader's binding order — the same
/// numbering Metal's port uses, so a bug in one backend's slot mapping
/// reads as a bug in both files.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Glyphs,
    Images,
    Masks,
    Ramps,
}

impl Slot {
    const ALL: [Self; 4] = [Self::Glyphs, Self::Images, Self::Masks, Self::Ramps];

    const fn index(self) -> usize {
        match self {
            Self::Glyphs => 0,
            Self::Images => 1,
            Self::Masks => 2,
            Self::Ramps => 3,
        }
    }

    /// The texel size of an upload — R8 for glyph and mask coverage, RGBA8
    /// for images, RGBA32F for ramps. The shim's own format table keys on
    /// the slot number, and this constant is why both sides agree.
    const fn bytes_per_texel(self) -> u32 {
        match self {
            Self::Glyphs | Self::Masks => 1,
            Self::Images => 4,
            Self::Ramps => 16,
        }
    }
}

/// One atlas slot's texture and the version last uploaded — Metal's
/// `AtlasTexture`, by handle.
struct AtlasTexture {
    texture: u32,
    version: Option<u64>,
    width: u32,
    height: u32,
}

/// `post.wgsl` with its immediates block in the address space WebGPU has.
///
/// The shader ships `var<immediate> pc: Post;` — WGSL's push-constant
/// space, which naga lowers for Vulkan and Metal but WebGPU does not
/// speak. The WebGPU variant is the same declaration in `var<uniform>` at
/// `@group(1) @binding(0)`, where the shim binds the 128-byte immediates
/// buffer. This is a *mechanical* rewrite of one declaration, asserted by
/// this module's tests against the file so a change to the source line
/// cannot slip the two apart silently — the drift failure mode the shader
/// library exists to prevent.
#[must_use]
pub fn post_wgsl_uniform() -> String {
    vieww_shaders::library::POST_WGSL.replacen(
        "var<immediate> pc: Post;",
        "@group(1) @binding(0) var<uniform> pc: Post;",
        1,
    )
}

/// A persistent WebGPU executor for [`ScenePlan`]s — `metal::scene`'s
/// shape, over any [`GpuBridge`]. See the module doc for the contract and
/// the poll protocol.
pub struct WebGpuSceneRenderer<B: GpuBridge> {
    bridge: B,
    atlases: [AtlasTexture; 4],
    levels: Vec<u32>,
    scratch: Vec<u32>,
    dummy: u32,
    size: (u32, u32),
    retained: Option<Retained>,
    timings: FrameTimings,
}

impl<B: GpuBridge> WebGpuSceneRenderer<B> {
    /// Create the pipelines, the dummy target and the placeholder atlases.
    ///
    /// # Errors
    ///
    /// [`WebGpuError::Failed`] when the bridge rejects a pipeline — on the
    /// browser that is the moment an engine without RGBA16F blending, or
    /// with WGSL the engine's validator refuses, is discovered; the
    /// message is the browser's own.
    pub fn new(mut bridge: B) -> Result<Self, WebGpuError> {
        bridge
            .pipeline_scene(vieww_shaders::library::SCENE_WGSL)
            .map_err(WebGpuError::Failed)?;
        let post = post_wgsl_uniform();
        bridge
            .pipeline_post(&post, false)
            .map_err(WebGpuError::Failed)?;
        bridge
            .pipeline_post(&post, true)
            .map_err(WebGpuError::Failed)?;

        // The 1x1 transparent stand-in, and one placeholder texel per
        // atlas slot in that slot's own format — the same bring-up Metal's
        // port does, for the same reason: a post pass that reads nothing
        // binds the dummy, and a scene draw with no atlas yet fetches a
        // zeroed texel rather than an unbound texture.
        let dummy = bridge.target_create(1, 1);
        bridge.frame_begin(1, 1);
        bridge.pass_begin(dummy, Some([0.0; 4]));
        bridge.pass_end();
        bridge.frame_submit(dummy);
        let atlases = Slot::ALL.map(|slot| {
            let texture = bridge.atlas(slot.index() as u32, 1, 1);
            AtlasTexture {
                texture,
                version: None,
                width: 1,
                height: 1,
            }
        });
        Ok(Self {
            bridge,
            atlases,
            levels: Vec::new(),
            scratch: Vec::new(),
            dummy,
            size: (0, 0),
            retained: None,
            timings: FrameTimings::default(),
        })
    }

    /// Execute `plan` at `width x height` over `clear`, submitting the
    /// frame and starting its readback; the pixels come back from
    /// [`Self::readback`].
    ///
    /// # Errors
    ///
    /// An incomplete plan (render such a frame on the CPU rasterizer), or
    /// the bridge refusing a call — see [`WebGpuError`].
    pub fn render_planned(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Result<(), WebGpuError> {
        self.render_planned_damaged(planner, plan, width, height, clear, None)
            .map(|_| ())
    }

    /// [`Self::render_planned`], redrawing only `damage` of the previous
    /// frame this renderer drew — the shared traversal's retained-frame
    /// rules, which are `vulkan::scene`'s.
    ///
    /// # Errors
    ///
    /// As [`Self::render_planned`].
    pub fn render_planned_damaged(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
        damage: Option<&[PixelRect]>,
    ) -> Result<Redrawn, WebGpuError> {
        if !plan.is_complete() {
            return Err(WebGpuError::Invalid(format!(
                "incomplete plan: this backend cannot draw {}. Render this frame on the CPU \
                 rasterizer",
                plan.unsupported
                    .iter()
                    .map(|(kind, count)| format!("{count}x{}", kind.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        self.timings = FrameTimings::default();
        let started = Clock::start();
        self.upload_planner(planner)?;
        self.timings.upload = started.into_elapsed();
        self.render_with(plan, width, height, clear, damage)
    }

    /// The submitted frame's pixels, once its readback lands: straight-alpha
    /// RGBA8, `width x height` of the last render. `None` until then —
    /// poll per frame.
    #[must_use]
    pub fn readback(&mut self) -> Option<Vec<u8>> {
        if self.bridge.readback_state() != 1 {
            return None;
        }
        let (width, height) = self.size;
        let texels = width as usize * height as usize * 4;
        let mut half = vec![0u8; texels * 2];
        let started = Clock::start();
        let copied = self.bridge.readback_take(&mut half);
        let halves: Vec<u16> = half[..copied.min(half.len())]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let pixels = crate::scene_exec::straight_rgba8_from_rgba16f(&halves);
        self.timings.readback = started.into_elapsed();
        Some(pixels)
    }

    /// Present the last rendered frame into the `<canvas>` with this id —
    /// the bridge configures the `webgpu` context and copies the frame
    /// target into `getCurrentTexture()`.
    ///
    /// # Errors
    ///
    /// [`WebGpuError::Invalid`] with no frame rendered yet, or
    /// [`WebGpuError::Failed`] when the canvas does not exist, will not
    /// give a `webgpu` context, or the browser refuses the copy.
    pub fn present(&mut self, canvas_id: &str) -> Result<(), WebGpuError> {
        let frame = self
            .levels
            .first()
            .copied()
            .ok_or_else(|| WebGpuError::Invalid("present() before a first frame".into()))?;
        self.bridge
            .present(canvas_id, frame)
            .map_err(WebGpuError::Failed)
    }

    /// Where the last frame's wall time went — see [`FrameTimings`].
    #[must_use]
    pub const fn last_timings(&self) -> FrameTimings {
        self.timings
    }

    // ── the frame ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_lines)]
    fn render_with(
        &mut self,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
        damage: Option<&[PixelRect]>,
    ) -> Result<Redrawn, WebGpuError> {
        let width = width.max(1);
        let height = height.max(1);
        let exec = frame_passes(plan, width, height, clear, damage, self.retained);
        // Until this frame finishes, `L[0]` is half-drawn.
        self.retained = None;

        let started = Clock::start();
        self.ensure_targets(width, height, plan.target_depth + 1)?;
        self.timings.upload += started.into_elapsed();

        self.bridge.buffer_write(0, as_bytes(&plan.vertices));
        self.bridge.buffer_write(1, as_bytes(&plan.indices));

        // ── the interpreter ─────────────────────────────────────────────
        //
        // The same open-target bookkeeping Metal's port keeps: a target
        // switch ends the pass and the next segment reopens one. The
        // pipeline deduplication lives in the bridge (a `setPipeline` call
        // the browser's encoder would elide anyway); what matters here is
        // the *sequence*, which is the contract the mock pins.
        let started = Clock::start();
        self.bridge.frame_begin(width, height);
        let mut open: Option<TargetId> = None;

        for op in &exec.ops {
            match op {
                SceneOp::Begin { target, clear, .. } => {
                    self.bridge.pass_begin(self.texture(*target), *clear);
                    open = Some(*target);
                }
                SceneOp::ClearRegions { rects, color } => {
                    // Never a first op: the frame's own begin precedes it.
                    open.expect("a region clear follows a begin");
                    let clamped: Vec<[u32; 4]> = rects
                        .iter()
                        .filter_map(|r| scissor(*r, width, height))
                        .collect();
                    if clamped.is_empty() {
                        continue;
                    }
                    self.bridge.clear_regions(&clamped, color);
                }
                SceneOp::DrawRun {
                    target,
                    first_index,
                    index_count,
                    scissor: rect,
                } => {
                    if open != Some(*target) {
                        self.bridge.pass_begin(self.texture(*target), None);
                        open = Some(*target);
                    }
                    self.bridge.pass_scene();
                    if let Some([x, y, w, h]) = scissor(*rect, width, height) {
                        self.bridge.pass_scissor(x, y, w, h);
                    }
                    self.bridge.pass_draw(*first_index, *index_count);
                }
                SceneOp::Post { pass, scissors, .. } => {
                    if open != Some(pass.into) {
                        self.bridge.pass_begin(self.texture(pass.into), None);
                        open = Some(pass.into);
                    }
                    // The post bind is per-op by construction (`src` and
                    // `aux` differ), so it is set every time.
                    self.bridge.pass_post(
                        self.texture(pass.src),
                        self.texture(pass.aux),
                        pass.over,
                        &pass.push,
                    );
                    for rect in scissors {
                        if let Some([x, y, w, h]) = scissor(*rect, width, height) {
                            self.bridge.pass_scissor(x, y, w, h);
                        }
                        self.bridge.pass_post_draw();
                    }
                }
            }
        }
        // The frame is `L[0]`: `ensure_targets` above guaranteed at least
        // one level, and the traversal's first op is always its begin.
        self.bridge.frame_submit(self.levels[0]);
        self.timings.record = started.into_elapsed();

        self.retained = Some(exec.retained);
        self.size = (width, height);
        Ok(exec.redrawn)
    }

    // ── resources ─────────────────────────────────────────────────────────

    /// The texture handle of `target` — the level, its scratch twin, or
    /// the dummy.
    fn texture(&self, target: TargetId) -> u32 {
        match target {
            TargetId::Level(n) => self.levels[n],
            TargetId::Scratch(n) => self.scratch[n],
            TargetId::Dummy => self.dummy,
        }
    }

    /// (Re)allocate the levels and scratch to `depth` targets of the frame
    /// size, clearing every fresh one to transparent — a scratch read
    /// before its first write must see transparent, not undefined bytes.
    fn ensure_targets(&mut self, width: u32, height: u32, depth: usize) -> Result<(), WebGpuError> {
        if self.size != (width, height) {
            self.clear_targets();
            self.size = (width, height);
        }
        while self.levels.len() < depth {
            let level = self.bridge.target_create(width, height);
            self.clear_target(level, width, height);
            self.levels.push(level);
            let scratch = self.bridge.target_create(width, height);
            self.clear_target(scratch, width, height);
            self.scratch.push(scratch);
        }
        Ok(())
    }

    /// Drop every level and scratch texture.
    fn clear_targets(&mut self) {
        for t in self.levels.drain(..) {
            self.bridge.target_free(t);
        }
        for t in self.scratch.drain(..) {
            self.bridge.target_free(t);
        }
    }

    /// One submitted pass that clears `target` to fully transparent.
    fn clear_target(&mut self, target: u32, width: u32, height: u32) {
        self.bridge.frame_begin(width, height);
        self.bridge.pass_begin(target, Some([0.0; 4]));
        self.bridge.pass_end();
        self.bridge.frame_submit(target);
    }

    /// Upload every atlas `planner` holds that changed since the last
    /// upload — whole-texture uploads, as Metal's port does.
    fn upload_planner(&mut self, planner: &Planner) -> Result<(), WebGpuError> {
        let atlas = planner.atlas();
        self.upload_atlas(
            Slot::Glyphs,
            atlas.texels(),
            atlas.side(),
            atlas.side(),
            Some(atlas.version()),
        )?;
        let images = planner.image_atlas();
        self.upload_atlas(
            Slot::Images,
            images.texels(),
            images.side(),
            images.side(),
            Some(images.version()),
        )?;
        let masks = planner.mask_atlas();
        self.upload_atlas(
            Slot::Masks,
            masks.texels(),
            masks.side(),
            masks.side(),
            Some(masks.version()),
        )?;
        let ramps = planner.ramp_atlas();
        self.upload_atlas(
            Slot::Ramps,
            as_bytes(ramps.texels()),
            vieww_gpu::RampAtlas::WIDTH,
            ramps.rows(),
            Some(ramps.version()),
        )?;
        Ok(())
    }

    /// Bring one atlas up to `bytes` at `version`: skipped when unchanged,
    /// recreated when the size changed, written through the bridge
    /// otherwise.
    fn upload_atlas(
        &mut self,
        slot: Slot,
        bytes: &[u8],
        width: u32,
        height: u32,
        version: Option<u64>,
    ) -> Result<(), WebGpuError> {
        let atlas = &mut self.atlases[slot.index()];
        if version.is_some()
            && atlas.version == version
            && atlas.width == width
            && atlas.height == height
        {
            return Ok(());
        }
        // The bridge reads a whole width x height region, so the source
        // must be at least that long: an atlas shorter than its declared
        // extent (or empty, whose pointer is dangling) is padded with
        // transparent texels rather than read out of bounds — Metal's
        // port's rule.
        let needed =
            u64::from(width.max(1)) * u64::from(height.max(1)) * u64::from(slot.bytes_per_texel());
        let padded;
        let source: &[u8] = if bytes.len() as u64 >= needed {
            bytes
        } else {
            let mut v = vec![
                0u8;
                usize::try_from(needed)
                    .map_err(|_| WebGpuError::Invalid("atlas size".into()))?
            ];
            v[..bytes.len()].copy_from_slice(bytes);
            padded = v;
            &padded
        };
        if atlas.width != width || atlas.height != height {
            atlas.texture = self
                .bridge
                .atlas(slot.index() as u32, width.max(1), height.max(1));
            atlas.width = width;
            atlas.height = height;
        }
        self.bridge.atlas_write(slot.index() as u32, source);
        atlas.version = version;
        Ok(())
    }
}

impl<B: GpuBridge> Drop for WebGpuSceneRenderer<B> {
    fn drop(&mut self) {
        self.clear_targets();
        for slot in Slot::ALL {
            self.bridge.target_free(self.atlases[slot.index()].texture);
        }
        self.bridge.target_free(self.dummy);
    }
}

impl<B: GpuBridge> std::fmt::Debug for WebGpuSceneRenderer<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebGpuSceneRenderer")
            .field("size", &self.size)
            .field("targets", &self.levels.len())
            .finish_non_exhaustive()
    }
}

/// A rectangle clamped to the frame, as `[x, y, w, h]`, or `None` when it
/// is empty once clamped — every rect the traversal emits is already
/// inside the frame, so this is a cast with the frame as a backstop.
fn scissor(rect: PixelRect, width: u32, height: u32) -> Option<[u32; 4]> {
    let x = rect.x0.clamp(0, width as i32) as u32;
    let y = rect.y0.clamp(0, height as i32) as u32;
    let x1 = rect.x1.clamp(0, width as i32) as u32;
    let y1 = rect.y1.clamp(0, height as i32) as u32;
    let w = x1.saturating_sub(x);
    let h = y1.saturating_sub(y);
    (w > 0 && h > 0).then_some([x, y, w, h])
}

/// `&[T]` as its bytes, for the whole-frame buffer writes — the same
/// "plain data, no padding" rule the other backends' `as_bytes` state, for
/// the same types (`Vertex`, `u32`, `f32`).
fn as_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: `T` is `Copy` plain data with no padding (see the doc); the
    // slice is the same memory read as bytes.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

// ── the wasm half: the raw extern surface, the device, `Device` ───────────
//
// Everything above compiles on every target (it is bridge-shaped); the
// extern surface below can only exist where an import module can — wasm.
// On any other target `vieww-hal --features webgpu` builds the mock-tested
// interpreter and nothing that links a browser.

#[cfg(target_arch = "wasm32")]
mod device {
    use super::{GpuBridge, PostPush, WebGpuError};
    use crate::{AdapterInfo, Device};
    use vieww_foundation::Color;

    // The wasm import module the page must map to `web/vieww-gpu-web.js` —
    // exactly the contract `vieww-audio`'s shim documents for
    // `"vieww_audio"`.
    #[link(wasm_import_module = "vieww_gpu_web")]
    unsafe extern "C" {
        // 1 when `navigator.gpu` exists — the availability probe.
        fn vieww_gpu_available() -> u32;
        // Begin adapter/device bring-up; returns the device handle the
        // rest of this surface is addressed by.
        fn vieww_gpu_request() -> u32;
        // Bring-up state: 0 pending, 1 ready, 2 failed.
        fn vieww_gpu_state(handle: u32) -> u32;
        // Copy the failure message into `buf` (at most `cap` bytes,
        // NUL-terminated); returns the message's full length.
        fn vieww_gpu_failure(handle: u32, buf: *mut u8, cap: u32) -> u32;
        // The adapter's own description, into `buf` as above.
        fn vieww_gpu_adapter_name(handle: u32, buf: *mut u8, cap: u32) -> u32;
        // Destroy the device and drop the handle's registry entry.
        fn vieww_gpu_release(handle: u32);

        // ── resources ────────────────────────────────────────────────────
        // A frame-sized premultiplied RGBA16F render target
        // (RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC); its handle.
        fn vieww_gpu_target_create(handle: u32, width: u32, height: u32) -> u32;
        // Drop a target or atlas texture.
        fn vieww_gpu_target_free(handle: u32, target: u32);
        // (Re)create the atlas texture in `slot` (0 glyphs R8, 1 images
        // RGBA8, 2 masks R8, 3 ramps RGBA32F) at `width x height`.
        fn vieww_gpu_atlas(handle: u32, slot: u32, width: u32, height: u32) -> u32;
        // Upload `len` bytes of atlas texels, row-major, into `slot`'s
        // texture.
        fn vieww_gpu_atlas_write(handle: u32, slot: u32, data: *const u8, len: u32);
        // Write `len` bytes into the vertex (which: 0) or index (1)
        // buffer, recreating either if its size changed.
        fn vieww_gpu_buffer_write(handle: u32, which: u32, data: *const u8, len: u32);

        // ── pipelines ────────────────────────────────────────────────────
        // Create the scene pipeline from `wgsl` (`len` UTF-8 bytes, no
        // NUL); 1 on success, 0 if the device rejected it.
        fn vieww_gpu_pipeline_scene(handle: u32, wgsl: *const u8, len: u32) -> u32;
        // Create a post pipeline from the uniform-address-space variant of
        // `post.wgsl`; `over` selects the premultiplied-blend one.
        fn vieww_gpu_pipeline_post(handle: u32, wgsl: *const u8, len: u32, over: u32) -> u32;

        // ── one frame ────────────────────────────────────────────────────
        // Begin a frame: a command encoder, and the viewport uniform for
        // `width x height`.
        fn vieww_gpu_frame_begin(handle: u32, width: u32, height: u32);
        // Begin a pass on `target`; `clear` (four premultiplied f32s)
        // clears, null loads what the target holds. Ends any open pass.
        fn vieww_gpu_pass_begin(handle: u32, target: u32, clear: *const f32);
        // End the open pass.
        fn vieww_gpu_pass_end(handle: u32);
        // Narrow the open pass's scissor.
        fn vieww_gpu_pass_scissor(handle: u32, x: u32, y: u32, w: u32, h: u32);
        // Bind the scene pipeline, its vertex/index buffers and atlas bind
        // group.
        fn vieww_gpu_pass_scene(handle: u32);
        // One indexed draw of `count` indices from `first_index`.
        fn vieww_gpu_pass_draw(handle: u32, first_index: u32, count: u32);
        // Bind a post pipeline (`over` selects which) with `src` and `aux`
        // textures and the 128-byte immediates block at `push`.
        fn vieww_gpu_pass_post(handle: u32, src: u32, aux: u32, over: u32, push: *const f32);
        // The full-screen post triangle.
        fn vieww_gpu_pass_post_draw(handle: u32);
        // Clear `count` rectangles (`x, y, w, h` each, already clamped) of
        // the open target to `color` — between passes.
        fn vieww_gpu_clear_regions(handle: u32, rects: *const u32, count: u32, color: *const f32);
        // End the frame's open pass, submit, and begin `target`'s readback.
        fn vieww_gpu_frame_submit(handle: u32, target: u32);
        // The readback's state: 0 pending, 1 ready, 2 failed.
        fn vieww_gpu_readback_state(handle: u32) -> u32;
        // Copy the readback (RGBA16F texels) into `out`; returns bytes
        // copied.
        fn vieww_gpu_readback_take(handle: u32, out: *mut u8, len: u32) -> u32;
        // Present `target` into the `<canvas>` whose id is `canvas` (`len`
        // UTF-8 bytes): configure the context, copy-mode post pass into
        // `getCurrentTexture()`, submit. 1 on success.
        fn vieww_gpu_present(handle: u32, canvas: *const u8, len: u32, target: u32) -> u32;
    }

    /// Read one of the shim's string-returning imports (fill a caller-owned
    /// buffer, return the full length) into a `String`.
    fn shim_string(handle: u32, import: unsafe extern "C" fn(u32, *mut u8, u32) -> u32) -> String {
        // SAFETY: the first call asks the length; `buf` may be null for a
        // length-only query, by the shim's contract.
        let len = unsafe { import(handle, std::ptr::null_mut(), 0) };
        let mut buf = vec![0u8; len.saturating_add(1) as usize];
        // SAFETY: `buf` is `len + 1` bytes — the message, and room for the
        // NUL; the returned count is the message's own length.
        let filled = unsafe { import(handle, buf.as_mut_ptr(), buf.len() as u32) };
        buf.truncate(filled as usize);
        buf.truncate(buf.iter().position(|&b| b == 0).unwrap_or(buf.len()));
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// A `&str` handed to a shim import as `(ptr, len)`.
    fn call_with_str(text: &str, call: impl FnOnce(*const u8, u32) -> u32) -> u32 {
        call(text.as_ptr(), u32::try_from(text.len()).unwrap_or(u32::MAX))
    }

    /// The extern surface, as a [`GpuBridge`] — one device handle's calls,
    /// forwarded verbatim.
    ///
    /// `pub` only so [`WebGpuDevice::scene_renderer`] can name the type it
    /// hands back without leaking a private one through a public method;
    /// this whole module is private to the backend, so nothing outside it
    /// can construct or name `ExternBridge` anyway.
    #[derive(Debug)]
    pub struct ExternBridge {
        handle: u32,
    }

    impl GpuBridge for ExternBridge {
        fn atlas(&mut self, slot: u32, width: u32, height: u32) -> u32 {
            // SAFETY: the handle's own slot, at the size named.
            unsafe { vieww_gpu_atlas(self.handle, slot, width, height) }
        }

        fn atlas_write(&mut self, slot: u32, bytes: &[u8]) {
            // SAFETY: `bytes` is the slice its length names.
            unsafe {
                vieww_gpu_atlas_write(
                    self.handle,
                    slot,
                    bytes.as_ptr(),
                    u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                )
            };
        }

        fn buffer_write(&mut self, which: u32, bytes: &[u8]) {
            // SAFETY: `bytes` is the slice its length names.
            unsafe {
                vieww_gpu_buffer_write(
                    self.handle,
                    which,
                    bytes.as_ptr(),
                    u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                )
            };
        }

        fn pipeline_scene(&mut self, wgsl: &str) -> Result<(), String> {
            // SAFETY: the WGSL slice, by pointer and length.
            let ok = call_with_str(wgsl, |ptr, len| unsafe {
                vieww_gpu_pipeline_scene(self.handle, ptr, len)
            });
            u32::from(ok == 1)
                .checked_sub(1)
                .map(|_| ())
                .ok_or_else(|| shim_string(self.handle, vieww_gpu_failure))
        }

        fn pipeline_post(&mut self, wgsl: &str, over: bool) -> Result<(), String> {
            // SAFETY: the WGSL slice, by pointer and length; `over` as the
            // one flag the shim's two post pipelines differ by.
            let ok = call_with_str(wgsl, |ptr, len| unsafe {
                vieww_gpu_pipeline_post(self.handle, ptr, len, u32::from(over))
            });
            u32::from(ok == 1)
                .checked_sub(1)
                .map(|_| ())
                .ok_or_else(|| shim_string(self.handle, vieww_gpu_failure))
        }

        fn frame_begin(&mut self, width: u32, height: u32) {
            // SAFETY: the frame's own extents.
            unsafe { vieww_gpu_frame_begin(self.handle, width, height) };
        }

        fn pass_begin(&mut self, target: u32, clear: Option<[f32; 4]>) {
            // SAFETY: `clear` is borrowed for the whole call (an `if let`
            // on a local, not a `map_or` whose closure returns a pointer
            // into its own dropped temporary — the first browser run
            // caught exactly that mistake, as a non-finite clear colour
            // read from a dead stack slot); `clear`'s four f32s, or null
            // to load.
            if let Some(color) = &clear {
                unsafe { vieww_gpu_pass_begin(self.handle, target, color.as_ptr()) };
            } else {
                unsafe { vieww_gpu_pass_begin(self.handle, target, std::ptr::null()) };
            }
        }

        fn pass_end(&mut self) {
            // SAFETY: ends the open pass.
            unsafe { vieww_gpu_pass_end(self.handle) };
        }

        fn pass_scissor(&mut self, x: u32, y: u32, w: u32, h: u32) {
            // SAFETY: the clamped rectangle.
            unsafe { vieww_gpu_pass_scissor(self.handle, x, y, w, h) };
        }

        fn pass_scene(&mut self) {
            // SAFETY: binds the scene pipeline and its group.
            unsafe { vieww_gpu_pass_scene(self.handle) };
        }

        fn pass_draw(&mut self, first_index: u32, count: u32) {
            // SAFETY: the indexed draw's own range.
            unsafe { vieww_gpu_pass_draw(self.handle, first_index, count) };
        }

        fn pass_post(&mut self, src: u32, aux: u32, over: bool, push: &PostPush) {
            // SAFETY: `push` is `#[repr(C)]` 128 bytes of f32s; the slice
            // is its own memory.
            unsafe {
                vieww_gpu_pass_post(
                    self.handle,
                    src,
                    aux,
                    u32::from(over),
                    std::ptr::from_ref(push).cast(),
                )
            };
        }

        fn pass_post_draw(&mut self) {
            // SAFETY: the full-screen post triangle.
            unsafe { vieww_gpu_pass_post_draw(self.handle) };
        }

        fn clear_regions(&mut self, rects: &[[u32; 4]], color: &[f32; 4]) {
            // SAFETY: the rectangles and the clear colour, by length.
            unsafe {
                vieww_gpu_clear_regions(
                    self.handle,
                    rects.as_ptr().cast(),
                    rects.len() as u32,
                    color.as_ptr(),
                )
            };
        }

        fn frame_submit(&mut self, target: u32) {
            // SAFETY: submits the frame and starts the named target's
            // readback.
            unsafe { vieww_gpu_frame_submit(self.handle, target) };
        }

        fn readback_state(&mut self) -> u32 {
            // SAFETY: the readback's state.
            unsafe { vieww_gpu_readback_state(self.handle) }
        }

        fn readback_take(&mut self, out: &mut [u8]) -> usize {
            // SAFETY: `out` is the slice its length names; the shim copies
            // at most that much.
            let copied =
                unsafe { vieww_gpu_readback_take(self.handle, out.as_mut_ptr(), out.len() as u32) };
            copied as usize
        }

        fn present(&mut self, canvas: &str, target: u32) -> Result<(), String> {
            // SAFETY: the canvas id, by pointer and length.
            let ok = call_with_str(canvas, |ptr, len| unsafe {
                vieww_gpu_present(self.handle, ptr, len, target)
            });
            if ok == 1 {
                Ok(())
            } else {
                Err(shim_string(self.handle, vieww_gpu_failure))
            }
        }

        fn target_create(&mut self, width: u32, height: u32) -> u32 {
            // SAFETY: a fresh frame-sized target.
            unsafe { vieww_gpu_target_create(self.handle, width, height) }
        }

        fn target_free(&mut self, target: u32) {
            // SAFETY: frees a texture this bridge created.
            unsafe { vieww_gpu_target_free(self.handle, target) };
        }
    }

    /// Bring-up state — what [`WebGpuDevice::state`] reports.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BringUp {
        /// The adapter/device request is still in flight.
        Pending,
        /// The device is ready.
        Ready,
        /// The browser refused; the message is its own.
        Failed(String),
    }

    /// A pending or ready WebGPU device, by handle into the shim's
    /// registry — the bridge it hands [`WebGpuSceneRenderer`](super::WebGpuSceneRenderer).
    ///
    /// Cheap to hold (one `u32`) and `Send + Sync` because wasm is
    /// single-threaded and the handle is plain data — but deliberately
    /// **not `Clone`**: the handle is released in `Drop`, and a second owner
    /// would release the browser's device underneath the first.
    #[derive(Debug)]
    pub struct WebGpuDevice {
        handle: u32,
    }

    impl WebGpuDevice {
        /// 1 when the browser reports WebGPU at all — the auto-selection
        /// probe. A page without it should not call [`Self::request`]; the
        /// CPU canvas backend is the whole answer there.
        #[must_use]
        pub fn available() -> bool {
            // SAFETY: an availability probe with no arguments — the one
            // import that is safe to call before any device exists, and
            // only to *ask* whether one can.
            unsafe { vieww_gpu_available() != 0 }
        }

        /// Begin adapter/device bring-up. The result starts
        /// [`BringUp::Pending`]; poll [`Self::state`] until it is not.
        #[must_use]
        pub fn request() -> Self {
            // SAFETY: bring-up begins here; the handle it returns
            // addresses every later call, and stays valid until `Drop`.
            Self {
                handle: unsafe { vieww_gpu_request() },
            }
        }

        /// Where bring-up stands.
        #[must_use]
        pub fn state(&self) -> BringUp {
            // SAFETY: a state read on a handle this type owns.
            match unsafe { vieww_gpu_state(self.handle) } {
                1 => BringUp::Ready,
                2 => BringUp::Failed(shim_string(self.handle, vieww_gpu_failure)),
                _ => BringUp::Pending,
            }
        }

        /// The adapter's own description, for receipts and devtools.
        #[must_use]
        pub fn adapter_name(&self) -> String {
            shim_string(self.handle, vieww_gpu_adapter_name)
        }

        /// The scene renderer over this device — the pipelines are created
        /// here, which is where a browser that cannot take them says so.
        ///
        /// # Errors
        ///
        /// [`WebGpuError::Pending`] until the device is ready — call again
        /// next frame; [`WebGpuError::Failed`] if a pipeline was refused.
        pub fn scene_renderer(
            &self,
        ) -> Result<super::WebGpuSceneRenderer<ExternBridge>, WebGpuError> {
            match self.state() {
                BringUp::Ready => {}
                BringUp::Pending => return Err(WebGpuError::Pending),
                BringUp::Failed(message) => return Err(WebGpuError::Failed(message)),
            }
            super::WebGpuSceneRenderer::new(ExternBridge {
                handle: self.handle,
            })
        }
    }

    impl Drop for WebGpuDevice {
        fn drop(&mut self) {
            // SAFETY: the handle's own release; the shim destroys the
            // device and drops the registry entry, so no later call can
            // address it.
            unsafe { vieww_gpu_release(self.handle) };
        }
    }

    impl crate::sealed::Sealed for WebGpuDevice {}

    impl Device for WebGpuDevice {
        type Error = WebGpuError;

        /// The M0/M1 clear, read back — asynchronously, because WebGPU's
        /// only readback is `mapAsync` and wasm cannot block.
        ///
        /// The trait's contract is synchronous, so this is the documented
        /// two-call protocol from the module doc: a call renders
        /// `clear_color` into a fresh target, submits, starts the
        /// readback, and returns [`WebGpuError::Pending`]; a later call
        /// returns those pixels as straight-alpha RGBA8. Callers that want
        /// the poll shape spelled out use [`WebGpuDevice::scene_renderer`].
        fn render_clear_to_pixels(
            &self,
            width: u32,
            height: u32,
            clear_color: Color,
        ) -> Result<Vec<u8>, WebGpuError> {
            if !matches!(self.state(), BringUp::Ready) {
                return Err(WebGpuError::Pending);
            }
            let mut bridge = ExternBridge {
                handle: self.handle,
            };
            let a = f32::from(clear_color.a) / 255.0;
            let premultiplied = [
                f32::from(clear_color.r) / 255.0 * a,
                f32::from(clear_color.g) / 255.0 * a,
                f32::from(clear_color.b) / 255.0 * a,
                a,
            ];
            let (width, height) = (width.max(1), height.max(1));
            let target = bridge.target_create(width, height);
            bridge.frame_begin(width, height);
            bridge.pass_begin(target, Some(premultiplied));
            bridge.pass_end();
            bridge.frame_submit(target);
            bridge.target_free(target);
            if bridge.readback_state() != 1 {
                return Err(WebGpuError::Pending);
            }
            let texels = width as usize * height as usize * 4;
            let mut half = vec![0u8; texels * 2];
            let copied = bridge.readback_take(&mut half);
            let halves: Vec<u16> = half[..copied.min(half.len())]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            Ok(crate::scene_exec::straight_rgba8_from_rgba16f(&halves))
        }

        fn info(&self) -> AdapterInfo {
            AdapterInfo {
                name: self.adapter_name(),
                backend: "webgpu",
                device_type: "browser".to_owned(),
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use device::{BringUp, ExternBridge, WebGpuDevice};

// ── the tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use vieww_foundation::{BlendMode, Color};
    use vieww_gpu::{DrawRun, FilterOp, Step, Unsupported};

    use super::*;

    /// The post rewrite is mechanical and pinned: if `post.wgsl`'s
    /// declaration line changes, this test fails before the WebGPU backend
    /// can drift from the shader it shares with Vulkan, Metal and D3D12.
    #[test]
    fn the_post_rewrite_touches_exactly_one_declaration() {
        let rewritten = post_wgsl_uniform();
        assert!(
            rewritten.contains("@group(1) @binding(0) var<uniform> pc: Post;"),
            "the rewrite must put the immediates block in a uniform at group 1 binding 0"
        );
        assert!(
            !rewritten.contains("var<immediate>"),
            "no immediate address space may survive the rewrite"
        );
        let source = vieww_shaders::library::POST_WGSL;
        assert_eq!(
            rewritten.len(),
            source.len() + "@group(1) @binding(0) var<uniform> pc: Post;".len()
                - "var<immediate> pc: Post;".len()
        );
        assert!(
            source.contains("var<immediate> pc: Post;"),
            "post.wgsl still declares its immediates the push-constant way — if this \
             fails, the WebGPU rewrite needs updating in the same commit"
        );
    }

    /// `scene.wgsl` needs no rewrite at all — it is authored against
    /// WebGPU's binding model (a uniform at group 0 binding 0, four texture
    /// bindings), which is what the shim's pipeline layout encodes. Pin the
    /// declarations the shim depends on, so a shader edit cannot silently
    /// break the browser path.
    #[test]
    fn the_scene_shader_is_already_webgpu_shaped() {
        let source = vieww_shaders::library::SCENE_WGSL;
        assert!(source.contains("@group(0) @binding(0) var<uniform> u: Uniforms;"));
        for binding in (1..=4).map(|b| format!("@group(0) @binding({b}) var t_")) {
            assert!(
                source.contains(&binding),
                "scene.wgsl must keep {binding}… — the shim's scene bind group layout is \
                 built from it"
            );
        }
        assert!(
            !source.contains("var<immediate>"),
            "the scene shader must stay free of the immediate address space, which WebGPU \
             does not speak"
        );
    }

    #[test]
    fn a_scissor_clamps_to_the_frame() {
        assert_eq!(
            scissor(
                PixelRect {
                    x0: -4,
                    y0: 0,
                    x1: 10,
                    y1: 6
                },
                8,
                8
            ),
            Some([0, 0, 8, 6])
        );
        assert_eq!(
            scissor(
                PixelRect {
                    x0: 8,
                    y0: 8,
                    x1: 9,
                    y1: 9
                },
                8,
                8
            ),
            None,
            "a scissor wholly outside the frame draws nothing"
        );
    }

    /// A recording bridge: every call appends one line, so a plan's whole
    /// encoding is asserted as a sequence — the interpreter's contract,
    /// checked with no browser and no GPU, on every machine that runs
    /// `cargo test -p vieww-hal --features webgpu`.
    #[derive(Default)]
    struct MockBridge {
        log: Vec<String>,
        readback_ready: bool,
    }

    /// The calls' names, as the log writes them — shared by the tests, so
    /// an assertion failure reads as "the encoder did this, wanted that".
    /// A call with no detail logs as exactly its name, so the tests can
    /// compare whole lines (`"submit"`, `"scene"`) without trailing-space
    /// accidents.
    fn log_line(kind: &str, detail: String) -> String {
        if detail.is_empty() {
            kind.to_owned()
        } else {
            format!("{kind} {detail}")
        }
    }

    impl GpuBridge for MockBridge {
        fn atlas(&mut self, slot: u32, width: u32, height: u32) -> u32 {
            self.log
                .push(log_line("atlas", format!("{slot} {width}x{height}")));
            100 + slot
        }
        fn atlas_write(&mut self, slot: u32, bytes: &[u8]) {
            self.log
                .push(log_line("atlas_write", format!("{slot} {}", bytes.len())));
        }
        fn buffer_write(&mut self, which: u32, bytes: &[u8]) {
            self.log.push(log_line(
                if which == 0 { "vertices" } else { "indices" },
                bytes.len().to_string(),
            ));
        }
        fn pipeline_scene(&mut self, _wgsl: &str) -> Result<(), String> {
            self.log.push(log_line("pipeline", "scene".into()));
            Ok(())
        }
        fn pipeline_post(&mut self, _wgsl: &str, over: bool) -> Result<(), String> {
            self.log.push(log_line(
                "pipeline",
                if over { "post_over" } else { "post_replace" }.into(),
            ));
            Ok(())
        }
        fn frame_begin(&mut self, width: u32, height: u32) {
            self.log
                .push(log_line("frame", format!("{width}x{height}")));
        }
        fn pass_begin(&mut self, target: u32, clear: Option<[f32; 4]>) {
            self.log.push(log_line(
                "pass",
                format!(
                    "t{target} {}",
                    clear.map_or_else(|| "load".to_owned(), |c| format!("clear {:?}", c))
                ),
            ));
        }
        fn pass_end(&mut self) {
            self.log.push(log_line("pass_end", String::new()));
        }
        fn pass_scissor(&mut self, x: u32, y: u32, w: u32, h: u32) {
            self.log
                .push(log_line("scissor", format!("{x},{y} {w}x{h}")));
        }
        fn pass_scene(&mut self) {
            self.log.push(log_line("scene", String::new()));
        }
        fn pass_draw(&mut self, first_index: u32, count: u32) {
            self.log
                .push(log_line("draw", format!("{first_index}+{count}")));
        }
        fn pass_post(&mut self, src: u32, aux: u32, over: bool, push: &PostPush) {
            self.log.push(log_line(
                "post",
                format!(
                    "src{src} aux{aux} {} mode={}",
                    if over { "over" } else { "replace" },
                    push.args[0]
                ),
            ));
        }
        fn pass_post_draw(&mut self) {
            self.log.push(log_line("post_draw", String::new()));
        }
        fn clear_regions(&mut self, rects: &[[u32; 4]], color: &[f32; 4]) {
            self.log.push(log_line(
                "clear_regions",
                format!("{} rects {:?}", rects.len(), color),
            ));
        }
        fn frame_submit(&mut self, _target: u32) {
            self.log.push(log_line("submit", String::new()));
            self.readback_ready = true;
        }
        fn readback_state(&mut self) -> u32 {
            u32::from(self.readback_ready)
        }
        fn readback_take(&mut self, out: &mut [u8]) -> usize {
            // Transparent RGBA16F, so the conversion has something to chew.
            out.fill(0);
            out.len()
        }
        fn present(&mut self, canvas: &str, target: u32) -> Result<(), String> {
            self.log
                .push(log_line("present", format!("{canvas} t{target}")));
            Ok(())
        }
        fn target_create(&mut self, width: u32, height: u32) -> u32 {
            self.log
                .push(log_line("target", format!("{width}x{height}")));
            // Handles past the atlas range, so the log names them apart.
            10 + self.log.len() as u32
        }
        fn target_free(&mut self, target: u32) {
            self.log.push(log_line("free", format!("t{target}")));
        }
    }

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    const CLEAR: Color = Color::rgba(10, 20, 30, 255);

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> PixelRect {
        PixelRect { x0, y0, x1, y1 }
    }

    /// A plan of `steps` with one run of `indices`, so a `Draw` step has
    /// something to draw — `scene_exec`'s own helper, so these tests pin
    /// the same shapes its traversal tests do.
    fn plan(steps: Vec<Step>, target_depth: usize) -> ScenePlan {
        ScenePlan {
            vertices: vec![vieww_gpu::Vertex {
                position: [0.0, 0.0],
                color: [1.0, 0.0, 0.0, 1.0],
                uv: [0.0; 2],
                texture_kind: 0.0,
                params: [0.0; 4],
                local: [0.0; 2],
                extra: [0.0; 4],
                shape: [0.0; 4],
                mask: [0.0; 4],
            }],
            indices: vec![0, 0, 0],
            runs: vec![DrawRun {
                first_index: 0,
                index_count: 3,
                scissor: None,
            }],
            steps,
            target_depth,
            unsupported: BTreeMap::new(),
        }
    }

    /// An empty scene: exactly the module contract's minimal frame — begin
    /// cleared on `L[0]`, submit, readback — and nothing else.
    #[test]
    fn an_empty_scene_is_one_cleared_pass() {
        let bridge = MockBridge::default();
        let mut renderer = WebGpuSceneRenderer::new(bridge).expect("pipelines build");
        let mut planner = Planner::new();
        let empty = planner.plan(&vieww_paint::Scene::default(), WIDTH as f32, HEIGHT as f32);
        renderer
            .render_planned(&planner, &empty, WIDTH, HEIGHT, CLEAR)
            .expect("render");
        let log = &renderer.bridge.log;
        let frame_at = log
            .iter()
            .rposition(|line| line.starts_with("frame 64x48"))
            .expect("a frame begins");
        let frame = &log[frame_at..];
        assert_eq!(
            frame.iter().filter(|l| l.starts_with("pass t")).count(),
            1,
            "exactly one pass: {frame:?}"
        );
        assert_eq!(
            frame.iter().filter(|l| l.as_str() == "submit").count(),
            1,
            "exactly one submit"
        );
        assert_eq!(
            frame.iter().filter(|l| l.starts_with("draw")).count(),
            0,
            "nothing to draw"
        );
        assert_eq!(
            frame.iter().filter(|l| l.starts_with("post")).count(),
            0,
            "nothing to composite"
        );
        // The readback lands as the whole frame.
        let pixels = renderer.readback().expect("readback lands");
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    }

    /// One rectangle: a scene draw under the frame's own pass, with the
    /// atlas bind between pass begin and draw — the encoding order the
    /// browser path depends on.
    #[test]
    fn one_rectangle_draws_under_the_frame_pass() {
        let bridge = MockBridge::default();
        let mut renderer = WebGpuSceneRenderer::new(bridge).expect("pipelines build");
        let plan = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        let planner = Planner::new();
        renderer
            .render_planned(&planner, &plan, WIDTH, HEIGHT, CLEAR)
            .expect("render");
        let log = &renderer.bridge.log;
        let frame_at = log
            .iter()
            .rposition(|line| line.starts_with("frame 64x48"))
            .expect("a frame begins");
        let frame = &log[frame_at..];
        let pass_at = frame
            .iter()
            .position(|l| l.starts_with("pass t"))
            .expect("the frame pass");
        let scene_at = frame
            .iter()
            .position(|l| l.as_str() == "scene")
            .expect("the scene bind");
        let draw_at = frame
            .iter()
            .position(|l| l.starts_with("draw "))
            .expect("the draw");
        let submit_at = frame
            .iter()
            .position(|l| l.as_str() == "submit")
            .expect("submit");
        assert!(
            pass_at < scene_at && scene_at < draw_at && draw_at < submit_at,
            "bind before draw before submit: {frame:?}"
        );
        // One draw for the one run; the vertices and indices were written.
        assert!(renderer
            .bridge
            .log
            .iter()
            .any(|l| l.starts_with("vertices")));
        assert!(renderer.bridge.log.iter().any(|l| l.starts_with("indices")));
    }

    /// A nested layer with group opacity and a blur: the encoding is the
    /// composite contract — layer pass, three blur ping-pongs, the OVER
    /// composite back into `L[0]`, one submit.
    #[test]
    fn a_blurred_layer_ping_pongs_and_composites() {
        let bridge = MockBridge::default();
        let mut renderer = WebGpuSceneRenderer::new(bridge).expect("pipelines build");
        let layer = rect(0, 0, 60, 40);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: layer,
                    backdrop: None,
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: layer,
                    composite: Some(layer),
                    filter: Some(FilterOp {
                        box_radius: Some(2),
                        color_matrix: None,
                    }),
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 0.5,
                },
            ],
            1,
        );
        let planner = Planner::new();
        renderer
            .render_planned(&planner, &plan, WIDTH, HEIGHT, CLEAR)
            .expect("render");
        let log = &renderer.bridge.log;
        let frame_at = log
            .iter()
            .rposition(|line| line.starts_with("frame 64x48"))
            .expect("a frame begins");
        let frame = &log[frame_at..];
        let posts: Vec<&String> = frame.iter().filter(|l| l.starts_with("post ")).collect();
        // Three horizontal and three vertical blur passes, then the OVER.
        assert_eq!(
            posts
                .iter()
                .filter(|l| l.contains("mode=1") || l.contains("mode=2"))
                .count(),
            6,
            "three blur ping-pongs: {posts:?}"
        );
        assert!(
            posts
                .iter()
                .any(|l| l.contains("mode=6") && l.contains("over")),
            "the composite is a premultiplied OVER"
        );
        // Two passes at least: the layer's own and the frame's.
        assert!(
            frame.iter().filter(|l| l.starts_with("pass t")).count() >= 2,
            "the layer's pass and the frame's: {frame:?}"
        );
        assert_eq!(
            frame.iter().filter(|l| l.as_str() == "submit").count(),
            1,
            "one submit for the whole frame"
        );
        // A damaged re-render of the same plan emits region clears, not a
        // full clear — the retained path through the bridge.
        renderer
            .render_planned_damaged(
                &planner,
                &plan,
                WIDTH,
                HEIGHT,
                CLEAR,
                Some(&[PixelRect {
                    x0: 0,
                    y0: 0,
                    x1: 8,
                    y1: 8,
                }]),
            )
            .expect("damaged render");
        let log = &renderer.bridge.log;
        let second_frame = log[frame_at..]
            .iter()
            .filter(|l| l.starts_with("clear_regions"))
            .count();
        assert!(second_frame >= 1, "the damaged frame clears its regions");
    }

    /// The incomplete-plan refusal — a frame with holes is an error, never
    /// a frame.
    #[test]
    fn an_incomplete_plan_is_refused() {
        let bridge = MockBridge::default();
        let mut renderer = WebGpuSceneRenderer::new(bridge).expect("pipelines build");
        let mut incomplete = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        incomplete.unsupported.insert(Unsupported::Glyphs, 3);
        let planner = Planner::new();
        let refused = renderer.render_planned(&planner, &incomplete, WIDTH, HEIGHT, CLEAR);
        assert!(
            refused.is_err(),
            "an incomplete plan is an error, not a frame"
        );
        let message = refused.err().unwrap().to_string();
        assert!(message.contains("incomplete plan"), "{message}");
        // And nothing was encoded: no frame began after bring-up.
        let log = &renderer.bridge.log;
        assert!(
            log.iter().filter(|l| l.starts_with("frame 64x48")).count() == 0,
            "a refused plan encodes nothing"
        );
    }
}
