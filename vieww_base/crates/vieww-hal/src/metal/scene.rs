//! The Metal scene renderer — `vulkan::scene::SceneRenderer`'s frame, on
//! [`MetalDevice`].
//!
//! # What this is
//!
//! PENDING 1.2's port, built the way the audit recommends: not a copy of the
//! Vulkan renderer, but an interpreter of the *shared* plan traversal
//! ([`crate::scene_exec::frame_passes`]) over Metal primitives that already
//! existed — [`MetalDevice::pipeline`] (WGSL → MSL through `vieww-shaders`,
//! the vertex descriptor, the blend states), `render_target`, `buffer_with`
//! and the readback shape. What a frame *is* — the pass order, the scissored
//! regions, the damage a retained frame may keep, the immediates every post
//! pass pushes — is one definition in `scene_exec`, consumed by this backend
//! and Vulkan's alike; only the resource management is Metal's.
//!
//! # The contract, and what it means here
//!
//! Exactly `vulkan::scene`'s: targets are premultiplied RGBA16F (`L[n]` and
//! `S[n]` per nesting level, a 1x1 transparent `Dummy`), the four atlases
//! (R8 glyphs, RGBA8 images, R8 masks, RGBA32F ramps) sit at the WGSL
//! bindings the scene shader fetches them through, a frame is drawn over a
//! clear colour and read back as straight-alpha RGBA8 through the shared
//! conversion ([`crate::scene_exec::straight_rgba8_from_rgba16f`]). An
//! incomplete plan is an error, never a frame with holes.
//!
//! # What is deliberately simpler than Vulkan's
//!
//! * **One window route.** [`MetalSceneRenderer::render_planned_to_layer`]
//!   renders into `L[0]` and copies it onto a `CAMetalLayer` drawable in the
//!   same command buffer ([`super::present`]), with the frame left in
//!   flight — no readback. There is no direct (no-`L[0]`) route, no present
//!   history and no partial-copy probing: `L[0]` is the retained frame and
//!   every present copies all of it, which is one cheap pass on a tiled GPU.
//!   `render_planned` / `render_planned_damaged` keep the readback for the
//!   parity suites. `vieww-platform-winit` selects this route with
//!   `PresentPath::Metal` (the iOS default; `VIEWW_PRESENT=metal` on macOS).
//! * **Partial atlas uploads, and a carried grow.** Glyphs, images and
//!   ramps upload only the rectangles that changed when the atlas can say
//!   which ones those are (`vulkan::scene`'s `record_glyphs`/`record_images`/
//!   `record_ramps` on this backend's blits), and a grown atlas carries its
//!   old texture across with one GPU copy instead of re-sending it —
//!   [`MetalSceneRenderer::upload_regions`] and
//!   [`MetalSceneRenderer::resize_with_carry`]. Masks were always partial
//!   ([`MetalSceneRenderer::upload_masks`]).
//! * **Host timing.** [`FrameTimings`] is `Instant`-based. Metal's real
//!   timestamps are counter samples (`MTLCounterSampleBuffer`, which the
//!   `metal` crate binds) — unported here, so there is no `GpuPassTimes`
//!   on this backend; `scene_exec`'s ops already carry the `PassKind`
//!   marks a port would sample at, which is the seam. The numbers say
//!   where the CPU spent and waited, not where the GPU spent cycles — the
//!   module docs' own rule for what such numbers may claim.
//! * **No persistent staging to trim.** Vulkan keeps a staging buffer per
//!   atlas slot and frees an eight-times-oversized one after a run of
//!   smaller frames (`STAGING_TRIM_FRAMES`); this backend builds its
//!   staging per upload and drops it when the blit is encoded, so there is
//!   nothing idle to free. The *target* trim (`TARGET_TRIM_FRAMES`) is
//!   ported: a run of frames shallower than the deepest one frees the
//!   surplus layer targets.
//! * **One encoder per target segment.** Consecutive ops on one target
//!   share a render encoder (and its pipeline binds, deduplicated); every
//!   target switch ends it and the next segment rebinds. Vulkan's render
//!   passes achieve the same sharing; Metal has no `cmd_clear_attachments`
//!   inside an encoder, so a damaged frame's region clears are blits of
//!   the clear colour under the damage's rectangles — the draw-based clear
//!   this doc used to describe came back all-zero inside the damage on the
//!   macOS CI, and the blit replaced it.
//!
//! # Verification
//!
//! The traversal's tests run everywhere (they are `scene_exec`'s, on every
//! platform with no GPU at all). This backend type-checks from Linux
//! (`ci/check/cross-check.sh macos ios`) and executes for the first time on
//! the `framework-checks` macOS runner, in `tests/metal_scene.rs` — the
//! Vulkan parity suites' scenes against `vieww_paint`'s CPU rasterizer. No
//! number from a paravirtual GPU is a performance claim.

use std::time::{Duration, Instant};

use metal::{
    CommandQueue, ComputePipelineState, Device, MTLBlitOption, MTLClearColor, MTLIndexType,
    MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType, MTLResourceOptions, MTLScissorRect,
    MTLSize, MTLStorageMode, MTLStoreAction, MTLTextureUsage, MTLViewport, RenderPassDescriptor,
    RenderPipelineState, Texture, TextureDescriptor,
};
use vieww_foundation::Color;
use vieww_gpu::{PixelRect, Planner, ScenePlan, Vertex};

use crate::scene_exec::{
    compute_blur_wanted, frame_passes, PostPush, Redrawn, Retained, SceneOp, TargetId,
};

use super::{check_size, Attribute, AttributeFormat, Blend, MetalDevice, MetalError};

/// Where one frame's wall time went, on the CPU's clock — this backend's
/// counterpart of `vulkan::scene`'s
/// [`GpuFrameTimings`](crate::vulkan::GpuFrameTimings), with the fields a
/// backend that records no counter samples can honestly fill (the module
/// doc names what a port would sample). `submit` includes waiting for the
/// GPU, because this backend never leaves a frame in flight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTimings {
    /// Atlas and geometry uploads, and any target (re)allocation.
    pub upload: Duration,
    /// Building the command buffer's encoders.
    pub record: Duration,
    /// Committing it and waiting for the GPU to finish it.
    pub submit: Duration,
    /// Converting the finished frame to straight-alpha RGBA8.
    pub readback: Duration,
}

/// A persistent Metal executor for [`ScenePlan`]s — see the module doc.
pub struct MetalSceneRenderer {
    device: Device,
    queue: CommandQueue,
    scene_pipeline: RenderPipelineState,
    post_replace: RenderPipelineState,
    post_over: RenderPipelineState,
    /// `cs_blur` as a compute pipeline — the one-add-one-sub running sum
    /// the fragment form can only approximate. `None` when the run asked
    /// the compute blur off (`VIEWW_GPU_COMPUTE_BLUR=0`, the A/B), in which
    /// case the blur modes keep the fragment hybrid.
    blur: Option<ComputePipelineState>,
    levels: Vec<Texture>,
    scratch: Vec<Texture>,
    dummy: Texture,
    atlases: [AtlasTexture; 4],
    size: (u32, u32),
    retained: Option<Retained>,
    timings: FrameTimings,
    /// Bytes the most recent atlas uploads sent, per slot: the whole
    /// texture after a full upload, the changed rectangles otherwise, zero
    /// when nothing changed. For tests and measurement — the churn numbers
    /// the plan's parity suites compare between backends.
    glyph_upload_bytes: u64,
    image_upload_bytes: u64,
    ramp_upload_bytes: u64,
    /// Bytes the most recent [`Self::upload_masks`] sent.
    mask_upload_bytes: u64,
    /// Frames since a frame used the full depth of `levels` — the idle
    /// signal that frees the surplus targets; see [`TARGET_TRIM_FRAMES`].
    idle_target_frames: u32,
    /// How many idle frames before the target surplus is freed —
    /// [`TARGET_TRIM_FRAMES`] unless a test narrowed it.
    target_trim_frames: u32,
}

/// One atlas texture, and the version last uploaded.
struct AtlasTexture {
    texture: Texture,
    version: Option<u64>,
    width: u32,
    height: u32,
}

/// The four atlas slots, in the scene shader's binding order 1..=4.
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

    /// The WGSL `@binding` the scene shader fetches this atlas through.
    const fn binding(self) -> u64 {
        match self {
            Self::Glyphs => 1,
            Self::Images => 2,
            Self::Masks => 3,
            Self::Ramps => 4,
        }
    }
}

/// `vieww_gpu::Vertex`, field by field — the WGSL locations' offsets in
/// bytes. Location 0 is the position, location 1 the paint-space
/// coordinate, location 2 the `u32` index into the frame's constants
/// table; the flat payload those used to share a vertex with rides in the
/// table, fetched by the vertex stage through `buffer(5)` and sized by
/// `buffer(vieww_shaders::msl_slots::SIZES_BUFFER)`.
const SCENE_ATTRIBUTES: [Attribute; 3] = [
    Attribute {
        location: 0,
        offset: 0,
        format: AttributeFormat::Float(2),
    }, // position
    Attribute {
        location: 1,
        offset: 8,
        format: AttributeFormat::Float(2),
    }, // local
    Attribute {
        location: 2,
        offset: 16,
        format: AttributeFormat::Uint,
    }, // draw — the constants table index
];

/// Every render target and the dummy are premultiplied RGBA16F.
const TARGET_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA16Float;

/// How many frames that use fewer layer targets than the deepest frame
/// before the surplus is freed: ~10 s at 60 Hz. Depth grows when a frame
/// first nests layers and only ever grew before the trim existed — one
/// deep-layered frame pinned its targets for the life of the renderer, which
/// on a phone is the whole process. `L[0]` is always kept, so the retained
/// damage frame survives a trim.
const TARGET_TRIM_FRAMES: u32 = 600;

/// One atlas slot's own format — the scene shader's four bindings, as
/// `vulkan::scene`'s placeholder uploads size them.
const fn atlas_format(slot: Slot) -> MTLPixelFormat {
    match slot {
        Slot::Glyphs => MTLPixelFormat::R8Unorm,
        Slot::Images => MTLPixelFormat::RGBA8Unorm,
        Slot::Masks => MTLPixelFormat::R8Unorm,
        Slot::Ramps => MTLPixelFormat::RGBA32Float,
    }
}

const fn bytes_per_texel(format: MTLPixelFormat) -> u64 {
    match format {
        MTLPixelFormat::R8Unorm => 1,
        MTLPixelFormat::RGBA8Unorm => 4,
        MTLPixelFormat::RGBA32Float => 16,
        _ => 8,
    }
}

/// A GPU-only texture this backend can render into and sample from —
/// `MetalDevice::render_target`'s creation, on the raw device the renderer
/// holds (it outlives the `MetalDevice` that built it, so it is kept by
/// handle, not by reference to the wrapper).
fn new_target(device: &Device, width: u32, height: u32, format: MTLPixelFormat) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_width(u64::from(width.max(1)));
    desc.set_height(u64::from(height.max(1)));
    desc.set_pixel_format(format);
    desc.set_storage_mode(MTLStorageMode::Private);
    // `ShaderWrite` is the compute blur's `textureStore`: `cs_blur` writes
    // its running sums straight into the destination target, which a
    // `RenderTarget | ShaderRead` texture refuses. Vulkan's twin decision
    // (the `STORAGE` usage flag, set with the pipeline or not at all) is
    // made here for the same reason.
    desc.set_usage(
        MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite,
    );
    device.new_texture(&desc)
}

/// A shared buffer holding `bytes`. An empty slice gets a zeroed 16-byte
/// buffer instead: its pointer is dangling (`0x4` for a `u32` slice), and
/// `newBufferWithBytes` copies `length` bytes from it — the macOS CI's
/// SIGSEGV on the empty scene, `EXC_BAD_ACCESS (address=0x4)` in memmove.
fn shared_buffer(device: &Device, bytes: &[u8]) -> metal::Buffer {
    if bytes.is_empty() {
        device.new_buffer(16, MTLResourceOptions::StorageModeShared)
    } else {
        device.new_buffer_with_data(
            bytes.as_ptr().cast(),
            len64(bytes),
            MTLResourceOptions::StorageModeShared,
        )
    }
}

/// The one place `T`'s bytes come from: a plain-data slice, as
/// `vulkan::scene`'s `as_bytes` — every type it is used with here (`Vertex`,
/// `u32`) is a struct of `f32`/`u32` fields with no padding.
fn as_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: `T` is `Copy` with no padding — see the doc above; the slice
    // is the same memory read as bytes.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

fn len64(bytes: &[u8]) -> u64 {
    bytes.len() as u64
}

/// A scissored rectangle, clamped to the frame — every rect the shared
/// traversal emits is already inside it, so this is a cast with the frame
/// as a backstop.
fn scissor(rect: PixelRect, width: u32, height: u32) -> MTLScissorRect {
    let x = rect.x0.clamp(0, width as i32) as u64;
    let y = rect.y0.clamp(0, height as i32) as u64;
    let x1 = rect.x1.clamp(0, width as i32) as u64;
    let y1 = rect.y1.clamp(0, height as i32) as u64;
    MTLScissorRect {
        x,
        y,
        width: x1.saturating_sub(x),
        height: y1.saturating_sub(y),
    }
}

impl MetalSceneRenderer {
    /// Build the pipelines and the placeholder atlases.
    ///
    /// # Errors
    ///
    /// Shader translation (a `vieww-shaders` error) or Metal refusing a
    /// pipeline — see [`MetalDevice::pipeline`].
    pub fn new(device: &MetalDevice) -> Result<Self, MetalError> {
        let scene_pipeline = device.pipeline(
            vieww_shaders::library::SCENE_WGSL,
            &SCENE_ATTRIBUTES,
            u64::try_from(std::mem::size_of::<Vertex>())
                .map_err(|_| MetalError::Invalid("vertex stride".into()))?,
            TARGET_FORMAT,
            Blend::PremultipliedOver,
        )?;
        let post_replace = device.pipeline(
            vieww_shaders::library::POST_WGSL,
            &[],
            0,
            TARGET_FORMAT,
            Blend::Replace,
        )?;
        let post_over = device.pipeline(
            vieww_shaders::library::POST_WGSL,
            &[],
            0,
            TARGET_FORMAT,
            Blend::PremultipliedOver,
        )?;
        // The compute blur — `cs_blur`, the third port of it after Vulkan
        // and WebGPU. No storage-feature probe the way Vulkan needs one:
        // every Metal device this renderer targets writes RGBA16F, and a
        // pipeline that cannot be built is an error this backend reports
        // rather than a route it silently declines.
        let blur = if compute_blur_wanted() {
            Some(device.compute_pipeline(vieww_shaders::library::POST_WGSL, "cs_blur")?)
        } else {
            None
        };
        // Region clears are blits of the clear colour (see `render_with`).

        let mut renderer = Self {
            device: device.raw().clone(),
            queue: device.queue().clone(),
            scene_pipeline,
            post_replace,
            post_over,
            blur,
            levels: Vec::new(),
            scratch: Vec::new(),
            dummy: new_target(device.raw(), 1, 1, TARGET_FORMAT),
            // Each placeholder in its own slot's format: the 1x1 upload below
            // blits one texel of *that* format, and `upload_atlas` only
            // recreates a texture whose size changed — a placeholder in the
            // target format would take an R8 or RGBA8 row into an RGBA16F
            // texture (a bytes-per-row below the texel size Metal requires).
            atlases: Slot::ALL.map(|slot| AtlasTexture {
                texture: new_target(device.raw(), 1, 1, atlas_format(slot)),
                version: None,
                width: 1,
                height: 1,
            }),
            size: (0, 0),
            retained: None,
            timings: FrameTimings::default(),
            glyph_upload_bytes: 0,
            image_upload_bytes: 0,
            ramp_upload_bytes: 0,
            mask_upload_bytes: 0,
            idle_target_frames: 0,
            target_trim_frames: TARGET_TRIM_FRAMES,
        };
        // New textures hold nothing: clear the dummy to transparent, and
        // give each atlas slot one zeroed texel of its own format.
        renderer.clear_to_transparent(renderer.dummy.as_ref())?;
        for slot in Slot::ALL {
            let format = atlas_format(slot);
            let zeros = vec![0u8; bytes_per_texel(format) as usize];
            renderer.upload_atlas(slot, &zeros, 1, 1, None)?;
        }
        Ok(renderer)
    }

    /// Execute `plan` at `width x height` over `clear`, with `planner`'s
    /// atlases uploaded first, and read the frame back as straight-alpha
    /// RGBA8 — `vulkan::scene::render_planned`'s contract.
    ///
    /// # Errors
    ///
    /// An incomplete plan (this backend cannot draw it either — render such
    /// a frame on the CPU rasterizer), or Metal failing.
    pub fn render_planned(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Result<Vec<u8>, MetalError> {
        self.render_planned_damaged(planner, plan, width, height, clear, None)
            .map(|(pixels, _)| pixels)
    }

    /// [`Self::render_planned`], redrawing only `damage` of the previous
    /// frame this renderer drew — the shared traversal's retained-frame
    /// rules, which are `vulkan::scene`'s. `None` redraws everything.
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
    ) -> Result<(Vec<u8>, Redrawn), MetalError> {
        if !plan.is_complete() {
            return Err(MetalError::Invalid(format!(
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
        let started = Instant::now();
        self.upload_planner(planner)?;
        self.timings.upload = started.elapsed();
        self.render_with(plan, width, height, clear, damage, None)
    }

    /// Execute a complete `plan` at the layer's extent and put it on
    /// `presenter`'s next drawable — no readback, the frame left in flight.
    /// `damage` redraws only those rectangles of the retained `L[0]`, as in
    /// [`Self::render_planned_damaged`]; the whole of `L[0]` is presented.
    /// Any clear alpha: the layer composites premultiplied pixels.
    ///
    /// # Errors
    ///
    /// As [`Self::render_planned`].
    pub fn render_planned_to_layer(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        clear: Color,
        presenter: &mut super::MetalLayerPresenter,
        damage: Option<&[PixelRect]>,
    ) -> Result<Redrawn, MetalError> {
        if !plan.is_complete() {
            return Err(MetalError::Invalid("incomplete plan".into()));
        }
        let (width, height) = presenter.extent();
        self.timings = FrameTimings::default();
        let started = Instant::now();
        self.upload_planner(planner)?;
        self.timings.upload = started.elapsed();
        metal::objc::rc::autoreleasepool(|| {
            self.render_with(plan, width, height, clear, damage, Some(presenter))
                .map(|(_, redrawn)| redrawn)
        })
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
        layer: Option<&mut super::MetalLayerPresenter>,
    ) -> Result<(Vec<u8>, Redrawn), MetalError> {
        let width = width.max(1);
        let height = height.max(1);
        check_size(width, height)?;

        let exec = frame_passes(plan, width, height, clear, damage, self.retained);
        // Until this frame finishes, `L[0]` is half-drawn.
        self.retained = None;

        let started = Instant::now();
        self.ensure_targets(width, height, plan.target_depth + 1)?;
        let vertices = shared_buffer(&self.device, as_bytes(&plan.vertices));
        let indices = shared_buffer(&self.device, as_bytes(&plan.indices));
        // The per-draw constants table and the size naga's bounds-check
        // clamp reads for it — one `uint` per runtime array, so one here.
        // Both are shared (not private) buffers: the encoder reads them as
        // bound, and the command buffer retains them past its own life.
        let constants = shared_buffer(&self.device, as_bytes(&plan.constants));
        let table_bytes = u32::try_from(as_bytes(&plan.constants).len()).unwrap_or(u32::MAX);
        let table_size = shared_buffer(&self.device, as_bytes(&[table_bytes]));
        #[expect(clippy::cast_precision_loss, reason = "frame extents")]
        let viewport: [f32; 4] = [width as f32, height as f32, 0.0, 0.0];
        self.timings.upload += started.elapsed();

        // ── the interpreter ─────────────────────────────────────────────
        //
        // One encoder per contiguous run of ops on one target; a target
        // switch ends it and the next segment rebinds. `bound` is the
        // encoder's own pipeline state, reset by every switch — the same
        // dedup Vulkan's interpreter does per render pass.
        let started = Instant::now();
        let commands = self.queue.new_command_buffer();

        #[derive(Clone, Copy, PartialEq)]
        enum Bound {
            None,
            Scene,
            /// The post pipeline, and whether it is the blending one.
            Post(bool),
        }
        let mut bound = Bound::None;
        let mut open: Option<TargetId> = None;
        let mut encoder: Option<&metal::RenderCommandEncoderRef> = None;

        fn begin_on<'a>(
            renderer: &'a MetalSceneRenderer,
            commands: &'a metal::CommandBufferRef,
            target: TargetId,
            clear: Option<[f32; 4]>,
            width: u32,
            height: u32,
        ) -> &'a metal::RenderCommandEncoderRef {
            let pass = RenderPassDescriptor::new();
            let attachment = pass
                .color_attachments()
                .object_at(0)
                .expect("attachment 0 exists");
            attachment.set_texture(Some(renderer.target(target)));
            match clear {
                Some(color) => {
                    attachment.set_load_action(MTLLoadAction::Clear);
                    attachment.set_clear_color(MTLClearColor::new(
                        f64::from(color[0]),
                        f64::from(color[1]),
                        f64::from(color[2]),
                        f64::from(color[3]),
                    ));
                }
                None => attachment.set_load_action(MTLLoadAction::Load),
            }
            attachment.set_store_action(MTLStoreAction::Store);
            let encoder = commands.new_render_command_encoder(pass);
            encoder.set_viewport(MTLViewport {
                originX: 0.0,
                originY: 0.0,
                width: f64::from(width),
                height: f64::from(height),
                znear: 0.0,
                zfar: 1.0,
            });
            encoder
        }

        fn end_current(encoder: &mut Option<&metal::RenderCommandEncoderRef>) {
            if let Some(open) = encoder.take() {
                open.end_encoding();
            }
        }

        for op in &exec.ops {
            match op {
                SceneOp::Begin { target, clear, .. } => {
                    end_current(&mut encoder);
                    bound = Bound::None;
                    encoder = Some(begin_on(self, commands, *target, *clear, width, height));
                    open = Some(*target);
                }
                SceneOp::ClearRegions { rects, color } => {
                    // Never a first op: the frame's own begin precedes it.
                    // A blit of the clear colour, not a draw: no pipeline,
                    // no shader, no binding — the draw-based clear (`clear.wgsl`
                    // under the scissor, blending `Replace`) came back
                    // all-zero inside the damage on the macOS CI, and this
                    // replaced it.
                    let target = open.expect("a region clear follows a begin");
                    end_current(&mut encoder);
                    bound = Bound::None;
                    let texel: [u16; 4] = color.map(crate::scene_exec::f32_to_half);
                    let blit = commands.new_blit_command_encoder();
                    let mut staging_buffers = Vec::new();
                    for rect in rects {
                        let r = scissor(*rect, width, height);
                        if r.width == 0 || r.height == 0 {
                            continue;
                        }
                        let count = usize::try_from(r.width * r.height).unwrap_or(0);
                        let fill: Vec<[u16; 4]> = vec![texel; count];
                        let staging = shared_buffer(&self.device, as_bytes(&fill));
                        blit.copy_from_buffer_to_texture(
                            &staging,
                            0,
                            r.width * 8,
                            r.width * r.height * 8,
                            MTLSize {
                                width: r.width,
                                height: r.height,
                                depth: 1,
                            },
                            self.target(target),
                            0,
                            0,
                            MTLOrigin {
                                x: r.x,
                                y: r.y,
                                z: 0,
                            },
                            MTLBlitOption::empty(),
                        );
                        staging_buffers.push(staging);
                    }
                    blit.end_encoding();
                    drop(staging_buffers); // retained by the command buffer
                    encoder = Some(begin_on(self, commands, target, None, width, height));
                }
                SceneOp::DrawRun {
                    target,
                    first_index,
                    index_count,
                    scissor: rect,
                } => {
                    if open != Some(*target) {
                        end_current(&mut encoder);
                        bound = Bound::None;
                        open = Some(*target);
                        encoder = Some(begin_on(self, commands, *target, None, width, height));
                    }
                    let enc = encoder.expect("a draw has a target to encode onto");
                    if bound != Bound::Scene {
                        enc.set_render_pipeline_state(&self.scene_pipeline);
                        enc.set_vertex_bytes(0, 16, viewport.as_ptr().cast());
                        enc.set_vertex_buffer(
                            u64::from(vieww_shaders::msl_slots::VERTEX_BUFFER),
                            Some(&vertices),
                            0,
                        );
                        // The constants table the scene's vertices index
                        // into, at the WGSL binding naga assigned it — and
                        // the byte size its `Restrict` clamp reads, at the
                        // slot `msl_slots` reserves for sizes.
                        enc.set_vertex_buffer(5, Some(&constants), 0);
                        enc.set_vertex_buffer(
                            u64::from(vieww_shaders::msl_slots::SIZES_BUFFER),
                            Some(&table_size),
                            0,
                        );
                        let [glyphs, images, masks, ramps] = &self.atlases;
                        enc.set_fragment_textures(
                            Slot::Glyphs.binding(),
                            &[
                                Some(&glyphs.texture),
                                Some(&images.texture),
                                Some(&masks.texture),
                                Some(&ramps.texture),
                            ],
                        );
                        bound = Bound::Scene;
                    }
                    enc.set_scissor_rect(scissor(*rect, width, height));
                    enc.draw_indexed_primitives(
                        MTLPrimitiveType::Triangle,
                        u64::from(*index_count),
                        MTLIndexType::UInt32,
                        &indices,
                        u64::from(*first_index) * 4,
                    );
                }
                SceneOp::Post { pass, scissors, .. } => {
                    // The blur modes' compute route: one dispatch per scissor
                    // band, the running sum carried along each scanline —
                    // `vulkan::scene`'s choice and `webgpu`'s, with the same
                    // `p0`-as-write-band convention. No direct-frame
                    // exclusion here: this backend has no presentation path
                    // to exclude (every frame renders to targets and reads
                    // back, the module doc's own simplification), and no
                    // storage-feature probe either — the pipeline is built
                    // for every device this renderer runs on. Encoders on
                    // one command buffer are ordered and their writes
                    // visible to the next, the same guarantee the blit-based
                    // region clears and the post passes' Load already rest
                    // on, so a dispatch between two render encoders needs
                    // no barrier of its own.
                    if let Some(blur) = self.blur.as_ref().filter(|_| {
                        matches!(
                            pass.push.args[0],
                            crate::scene_exec::mode::BOX_H | crate::scene_exec::mode::BOX_V
                        )
                    }) {
                        let horizontal = pass.push.args[0] < 1.5;
                        for rect in scissors {
                            let r = scissor(*rect, width, height);
                            if r.width == 0 || r.height == 0 {
                                continue;
                            }
                            let scanlines = if horizontal { r.height } else { r.width };
                            if scanlines == 0 {
                                continue;
                            }
                            let mut push = pass.push;
                            #[expect(clippy::cast_precision_loss, reason = "scissor extents")]
                            let band: [f32; 4] = [
                                r.x as f32,
                                r.y as f32,
                                (r.x + r.width) as f32,
                                (r.y + r.height) as f32,
                            ];
                            push.p[0] = band;
                            end_current(&mut encoder);
                            bound = Bound::None;
                            open = None;
                            let cs = commands.new_compute_command_encoder();
                            cs.set_compute_pipeline_state(blur);
                            // `t_src` at binding 0 and `t_dst` at binding 3 —
                            // the fragment set's bindings 1 and 2 are not in
                            // the compute blob, and the immediates ride the
                            // same `PostPush` the fragment pass receives,
                            // with `p0` borrowed for this dispatch's band.
                            cs.set_texture(0, Some(self.target(pass.src)));
                            cs.set_texture(3, Some(self.target(pass.into)));
                            cs.set_bytes(
                                u64::from(vieww_shaders::msl_slots::IMMEDIATES_BUFFER),
                                u64::try_from(std::mem::size_of::<PostPush>())
                                    .expect("the immediates block is 128 bytes"),
                                std::ptr::from_ref(&push).cast(),
                            );
                            // One invocation per scanline of the band, 64 to
                            // a threadgroup — the workgroup size `cs_blur`
                            // declares and the MSL kernel carries as its
                            // `max_total_threads_per_threadgroup`.
                            cs.dispatch_thread_groups(
                                MTLSize {
                                    width: scanlines.div_ceil(64),
                                    height: 1,
                                    depth: 1,
                                },
                                MTLSize {
                                    width: 64,
                                    height: 1,
                                    depth: 1,
                                },
                            );
                            cs.end_encoding();
                        }
                    } else {
                        if open != Some(pass.into) {
                            end_current(&mut encoder);
                            bound = Bound::None;
                            open = Some(pass.into);
                            encoder =
                                Some(begin_on(self, commands, pass.into, None, width, height));
                        }
                        let enc = encoder.expect("a post pass has a target to encode onto");
                        if bound != Bound::Post(pass.over) {
                            enc.set_render_pipeline_state(if pass.over {
                                &self.post_over
                            } else {
                                &self.post_replace
                            });
                            bound = Bound::Post(pass.over);
                        }
                        enc.set_fragment_textures(
                            0,
                            &[
                                Some(self.target(pass.src)),
                                Some(self.target(pass.aux)),
                                Some(&self.atlases[Slot::Masks.index()].texture),
                            ],
                        );
                        enc.set_fragment_bytes(
                            u64::from(vieww_shaders::msl_slots::IMMEDIATES_BUFFER),
                            128,
                            std::ptr::from_ref(&pass.push).cast(),
                        );
                        for rect in scissors {
                            enc.set_scissor_rect(scissor(*rect, width, height));
                            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
                        }
                    }
                }
            }
        }
        end_current(&mut encoder);
        self.timings.record = started.elapsed();

        // ── the frame out: onto a drawable, left in flight ──────────────
        if let Some(presenter) = layer {
            let submitting = Instant::now();
            presenter.encode_and_present(commands, self.levels[0].as_ref(), true);
            // The drawable wait is the presenter's `acquire`, not work.
            self.timings.submit = submitting
                .elapsed()
                .saturating_sub(presenter.last_timings().acquire);
            self.retained = Some(exec.retained);
            return Ok((Vec::new(), exec.redrawn));
        }

        // ── the frame out: readback ─────────────────────────────────────
        let started = Instant::now();
        let row = u64::from(width) * 8;
        let len = row * u64::from(height);
        let staging = self
            .device
            .new_buffer(len.max(1), MTLResourceOptions::StorageModeShared);
        {
            let blit = commands.new_blit_command_encoder();
            blit.copy_from_texture_to_buffer(
                self.levels[0].as_ref(),
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
                MTLSize {
                    width: u64::from(width),
                    height: u64::from(height),
                    depth: 1,
                },
                &staging,
                0,
                row,
                len,
                MTLBlitOption::empty(),
            );
            blit.end_encoding();
        }
        let submitting = Instant::now();
        commands.commit();
        commands.wait_until_completed();
        super::check(commands)?;
        self.timings.submit = submitting.elapsed();

        // SAFETY: a `Shared` buffer of exactly `len` bytes, written by the
        // completed blit above and not touched by the GPU again.
        let half: Vec<u16> = unsafe {
            std::slice::from_raw_parts(staging.contents().cast::<u16>(), (len / 2) as usize)
        }
        .to_vec();
        // The shared readback conversion — see `scene_exec`'s module doc.
        let pixels = crate::scene_exec::straight_rgba8_from_rgba16f(&half);
        self.timings.readback = started.elapsed() - self.timings.submit;

        self.retained = Some(exec.retained);
        Ok((pixels, exec.redrawn))
    }

    // ── resources ─────────────────────────────────────────────────────────

    /// The texture of `target` — the level, its scratch twin, or the dummy.
    fn target(&self, target: TargetId) -> &Texture {
        match target {
            TargetId::Level(n) => &self.levels[n],
            TargetId::Scratch(n) => &self.scratch[n],
            TargetId::Dummy => &self.dummy,
        }
    }

    /// (Re)allocate the levels and scratch to `depth` targets of the frame
    /// size, clearing every fresh one to transparent — a scratch read
    /// before its first write must see transparent, not undefined bytes.
    fn ensure_targets(&mut self, width: u32, height: u32, depth: usize) -> Result<(), MetalError> {
        if self.size != (width, height) {
            self.levels.clear();
            self.scratch.clear();
            self.size = (width, height);
        }
        let mut grew = false;
        while self.levels.len() < depth {
            let level_texture = new_target(&self.device, width, height, TARGET_FORMAT);
            self.clear_to_transparent(level_texture.as_ref())?;
            self.levels.push(level_texture);
            let scratch_texture = new_target(&self.device, width, height, TARGET_FORMAT);
            self.clear_to_transparent(scratch_texture.as_ref())?;
            self.scratch.push(scratch_texture);
            grew = true;
        }
        // **The idle shrink** — `vulkan::scene`'s, on this backend's
        // lifetimes: this renderer never leaves a frame in flight (every
        // command buffer is committed and waited on before the frame
        // returns), so the surplus targets are referenced by nothing when
        // this runs and dropping them needs no settle. `L[0]` is always
        // kept, so the retained damage frame survives. Growth — and a frame
        // that uses every level there is — is the opposite of idle.
        if grew || depth >= self.levels.len() {
            self.idle_target_frames = 0;
        } else {
            self.idle_target_frames = self.idle_target_frames.saturating_add(1);
            if self.idle_target_frames >= self.target_trim_frames && self.levels.len() > 1 {
                let keep = depth.max(1);
                self.levels.truncate(keep);
                self.scratch.truncate(keep);
                self.idle_target_frames = 0;
            }
        }
        Ok(())
    }

    /// One command buffer that clears `texture` to fully transparent.
    fn clear_to_transparent(&self, texture: &metal::TextureRef) -> Result<(), MetalError> {
        let commands = self.queue.new_command_buffer();
        let pass = RenderPassDescriptor::new();
        let attachment = pass
            .color_attachments()
            .object_at(0)
            .expect("attachment 0 exists");
        attachment.set_texture(Some(texture));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        attachment.set_store_action(MTLStoreAction::Store);
        let encoder = commands.new_render_command_encoder(pass);
        encoder.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        super::check(commands)
    }

    /// Upload every atlas `planner` holds that changed since the last
    /// upload — only the changed rectangles for every atlas now, with a
    /// grown atlas carried rather than re-sent; see the module doc.
    fn upload_planner(&mut self, planner: &Planner) -> Result<(), MetalError> {
        self.upload_glyphs(planner.atlas())?;
        self.upload_images(planner.image_atlas())?;
        self.upload_masks(planner.mask_atlas())?;
        self.upload_ramps(planner.ramp_atlas())?;
        Ok(())
    }

    /// Bring the glyph texture up to `atlas` the way [`Self::upload_masks`]
    /// brings the mask texture up: only the rectangles that changed, when
    /// the atlas can say which ones those are, and a grow carried by one
    /// GPU copy — `vulkan::scene`'s `record_glyphs`, on this backend's
    /// primitives. The numbers are that doc's own: one new glyph is a few
    /// hundred coverage bytes where the whole texture is up to 16 MB, and
    /// the frames that pay are the almost-steady ones.
    ///
    /// # Errors
    ///
    /// Metal refusing the copy or the command buffer.
    fn upload_glyphs(&mut self, atlas: &vieww_gpu::Atlas) -> Result<(), MetalError> {
        let held = self.atlases[Slot::Glyphs.index()].version;
        let side = atlas.side();
        let same_size = {
            let a = &self.atlases[Slot::Glyphs.index()];
            a.width == side && a.height == side
        };
        self.glyph_upload_bytes = 0;
        let rects = match atlas.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A grow, carried** — see `upload_glyphs`'s doc. `Regions`
                // with a resized texture means the atlas grew (a compaction
                // would answer `Full`): every texel kept its position, so
                // one GPU copy carries the old content across and only the
                // rectangles come from the host.
                self.resize_with_carry(Slot::Glyphs, side, side);
                rects
            }
            _ => {
                self.upload_atlas(
                    Slot::Glyphs,
                    atlas.texels(),
                    side,
                    side,
                    Some(atlas.version()),
                )?;
                self.glyph_upload_bytes = u64::from(side) * u64::from(side);
                return Ok(());
            }
        };
        let version = Some(atlas.version());
        self.glyph_upload_bytes =
            self.upload_regions(Slot::Glyphs, atlas.texels(), 1, side, version, rects)?;
        Ok(())
    }

    /// Bring the image texture up to `images` the same way. An insert's
    /// rectangle is the image plus its one-texel padding ring (linear
    /// sampling reads the ring, so it changes with the image); a frame that
    /// decodes one image sends that image's texels instead of the whole
    /// (up to 64 MB) texture.
    ///
    /// # Errors
    ///
    /// Metal refusing the copy or the command buffer.
    fn upload_images(&mut self, images: &vieww_gpu::ImageAtlas) -> Result<(), MetalError> {
        let held = self.atlases[Slot::Images.index()].version;
        let side = images.side();
        let same_size = {
            let a = &self.atlases[Slot::Images.index()];
            a.width == side && a.height == side
        };
        self.image_upload_bytes = 0;
        let rects = match images.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A grow, carried** — the 64 MB one: a photo app's first
                // overflow image used to re-send the entire atlas.
                self.resize_with_carry(Slot::Images, side, side);
                rects
            }
            _ => {
                self.upload_atlas(
                    Slot::Images,
                    images.texels(),
                    side,
                    side,
                    Some(images.version()),
                )?;
                self.image_upload_bytes = u64::from(side) * u64::from(side) * 4;
                return Ok(());
            }
        };
        let version = Some(images.version());
        self.image_upload_bytes =
            self.upload_regions(Slot::Images, images.texels(), 4, side, version, rects)?;
        Ok(())
    }

    /// Bring the ramp texture up to `ramps` the same way. A new ramp is
    /// exactly one row — 4 KB, the smallest write of the four atlases — and
    /// the only unit of ramp churn: a gradient whose stops animate is a new
    /// row every frame.
    ///
    /// # Errors
    ///
    /// Metal refusing the copy or the command buffer.
    fn upload_ramps(&mut self, ramps: &vieww_gpu::RampAtlas) -> Result<(), MetalError> {
        let held = self.atlases[Slot::Ramps.index()].version;
        let rows = ramps.rows();
        let same_size = {
            let a = &self.atlases[Slot::Ramps.index()];
            a.width == vieww_gpu::RampAtlas::WIDTH && a.height == rows
        };
        self.ramp_upload_bytes = 0;
        let rects = match ramps.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A row doubling, carried** — the copy is the old rows; the
                // rectangles are the new ones.
                self.resize_with_carry(Slot::Ramps, vieww_gpu::RampAtlas::WIDTH, rows);
                rects
            }
            _ => {
                self.upload_atlas(
                    Slot::Ramps,
                    as_bytes(ramps.texels()),
                    vieww_gpu::RampAtlas::WIDTH,
                    rows,
                    Some(ramps.version()),
                )?;
                self.ramp_upload_bytes =
                    u64::from(vieww_gpu::RampAtlas::WIDTH) * u64::from(rows) * 16;
                return Ok(());
            }
        };
        let version = Some(ramps.version());
        self.ramp_upload_bytes = self.upload_regions(
            Slot::Ramps,
            as_bytes(ramps.texels()),
            16,
            vieww_gpu::RampAtlas::WIDTH,
            version,
            rects,
        )?;
        Ok(())
    }

    /// Bring one atlas up to `bytes` at `version`: skipped when unchanged,
    /// recreated when the size changed, blitted from a shared buffer
    /// otherwise. Version `None` is the zeroed placeholder at bring-up.
    fn upload_atlas(
        &mut self,
        slot: Slot,
        bytes: &[u8],
        width: u32,
        height: u32,
        version: Option<u64>,
    ) -> Result<(), MetalError> {
        let atlas = &mut self.atlases[slot.index()];
        if version.is_some()
            && atlas.version == version
            && atlas.width == width
            && atlas.height == height
        {
            return Ok(());
        }
        let format = atlas_format(slot);
        if atlas.texture.width() != u64::from(width) || atlas.texture.height() != u64::from(height)
        {
            atlas.texture = new_target(&self.device, width.max(1), height.max(1), format);
            atlas.width = width;
            atlas.height = height;
        }
        // Metal copies `length` bytes from the pointer and the blit reads a
        // whole width x height region, so the source must be at least that
        // long: an atlas shorter than its declared extent (or empty, whose
        // pointer is dangling) is padded with transparent texels rather than
        // read out of bounds.
        let needed = u64::from(width.max(1)) * u64::from(height.max(1)) * bytes_per_texel(format);
        let padded;
        let source: &[u8] = if len64(bytes) >= needed {
            bytes
        } else {
            let mut v = vec![
                0u8;
                usize::try_from(needed)
                    .map_err(|_| MetalError::Invalid("atlas size".into()))?
            ];
            v[..bytes.len()].copy_from_slice(bytes);
            padded = v;
            &padded
        };
        let staging = self.device.new_buffer_with_data(
            source.as_ptr().cast(),
            len64(source),
            MTLResourceOptions::StorageModeShared,
        );
        let commands = self.queue.new_command_buffer();
        let blit = commands.new_blit_command_encoder();
        blit.copy_from_buffer_to_texture(
            &staging,
            0,
            u64::from(width.max(1)) * bytes_per_texel(format),
            0,
            MTLSize {
                width: u64::from(width.max(1)),
                height: u64::from(height.max(1)),
                depth: 1,
            },
            &atlas.texture,
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
            MTLBlitOption::empty(),
        );
        blit.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        super::check(commands)?;
        atlas.version = version;
        Ok(())
    }

    /// Bring the mask texture up to `masks`, sending only the rectangles
    /// that changed when the atlas can say which ones those are —
    /// `vulkan::scene`'s `record_masks` on this backend's primitives. The
    /// one shape difference is the submit: Vulkan records its copies into
    /// the frame's open command buffer, while every upload here is its own
    /// commit-and-wait, so the folded form `upload_planner` uses and this
    /// public one are the same function.
    ///
    /// # Why not just the whole-texture upload
    ///
    /// The reasons are `record_masks`'s own, and its numbers stand: a shape
    /// that changes every frame adds a new mask every frame, and a
    /// whole-texture upload on every version change made each of those
    /// frames send the entire atlas (measured 4.2 MB a frame for one
    /// rotating panel, 16.8 MB for three, against masks of a few hundred
    /// kilobytes). [`vieww_gpu::MaskAtlas::changes_since`] answers which
    /// texels differ from the version this texture holds; the rest of the
    /// texture is kept — Metal needs no barrier for that, the blit's own
    /// ordering is the whole story, where Vulkan transitions the image
    /// layout both ways. Anything the atlas cannot vouch for is a full
    /// upload, as before.
    ///
    /// # Errors
    ///
    /// Metal refusing the copy or the command buffer — the same failures
    /// the whole-texture upload path reports.
    pub fn upload_masks(&mut self, masks: &vieww_gpu::MaskAtlas) -> Result<(), MetalError> {
        let held = self.atlases[Slot::Masks.index()].version;
        let side = masks.side();
        let same_size = {
            let atlas = &self.atlases[Slot::Masks.index()];
            atlas.width == side && atlas.height == side
        };
        self.mask_upload_bytes = 0;
        let rects = match masks.changes_since(held) {
            vieww_gpu::MaskChanges::None => return Ok(()),
            vieww_gpu::MaskChanges::Regions(rects) if same_size => rects,
            _ => {
                self.upload_atlas(
                    Slot::Masks,
                    masks.texels(),
                    side,
                    side,
                    Some(masks.version()),
                )?;
                self.mask_upload_bytes = u64::from(side) * u64::from(side);
                return Ok(());
            }
        };
        let version = Some(masks.version());
        if rects.is_empty() {
            self.atlases[Slot::Masks.index()].version = version;
            return Ok(());
        }
        self.mask_upload_bytes =
            self.upload_regions(Slot::Masks, masks.texels(), 1, side, version, rects)?;
        Ok(())
    }

    /// The rectangle half every partial upload shares: `rects` — rectangles
    /// over a texture `pitch` texels wide of `stride`-byte texels — packed
    /// into one staging buffer, one blit per rectangle, leaving the texels
    /// outside them alone. Returns the bytes sent; an empty `rects` only
    /// stamps `version`, because a blit of nothing is not a no-op a queue
    /// is required to accept.
    ///
    /// The packed rows are `w * stride` bytes wide, which Metal's rule for
    /// `bytesPerRow` (a multiple of the texture's pixel size) is satisfied
    /// by construction for every atlas format here: R8 (1), RGBA8 (4) and
    /// RGBA32F (16) all divide their row bytes.
    ///
    /// # Errors
    ///
    /// Metal refusing the copy or the command buffer.
    fn upload_regions(
        &mut self,
        slot: Slot,
        texels: &[u8],
        stride: usize,
        pitch: u32,
        version: Option<u64>,
        rects: &[[u32; 4]],
    ) -> Result<u64, MetalError> {
        if rects.is_empty() {
            self.atlases[slot.index()].version = version;
            return Ok(0);
        }
        // Pack each rectangle's rows contiguously in the staging buffer.
        // Vulkan hands its copy regions to one `cmd_copy_buffer_to_image`;
        // Metal's blit takes one destination origin a call, so the packed
        // buffer is walked rectangle by rectangle.
        let mut packed = Vec::new();
        let mut regions = Vec::with_capacity(rects.len());
        for &[x, y, w, h] in rects {
            regions.push((len64(&packed), x, y, w, h));
            for row in y..y + h {
                let from = (row * pitch + x) as usize * stride;
                let to = from + w as usize * stride;
                packed.extend_from_slice(&texels[from..to]);
            }
        }
        let staging = shared_buffer(&self.device, &packed);
        let commands = self.queue.new_command_buffer();
        let blit = commands.new_blit_command_encoder();
        for &(offset, x, y, w, h) in &regions {
            blit.copy_from_buffer_to_texture(
                &staging,
                offset,
                u64::from(w) * stride as u64,
                0,
                MTLSize {
                    width: u64::from(w),
                    height: u64::from(h),
                    depth: 1,
                },
                &self.atlases[slot.index()].texture,
                0,
                0,
                MTLOrigin {
                    x: u64::from(x),
                    y: u64::from(y),
                    z: 0,
                },
                MTLBlitOption::empty(),
            );
        }
        blit.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        super::check(commands)?;
        self.atlases[slot.index()].version = version;
        Ok(len64(&packed))
    }

    /// Replace `slot`'s texture with a fresh `width` x `height` one and copy
    /// the old texture's content into it — `vulkan::scene`'s
    /// `resize_with_carry`, so a grown atlas spends one GPU copy instead of
    /// re-sending every texel it already holds. The old texture's lifetime
    /// ends with this command buffer's completion, which the commit-and-wait
    /// here has already happened by the time the caller uploads its regions.
    fn resize_with_carry(&mut self, slot: Slot, width: u32, height: u32) {
        let atlas = &mut self.atlases[slot.index()];
        let format = atlas_format(slot);
        let old = std::mem::replace(
            &mut atlas.texture,
            new_target(&self.device, width.max(1), height.max(1), format),
        );
        let copy_w = atlas.width.min(width);
        let copy_h = atlas.height.min(height);
        let commands = self.queue.new_command_buffer();
        let blit = commands.new_blit_command_encoder();
        blit.copy_from_texture(
            old.as_ref(),
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
            MTLSize {
                width: u64::from(copy_w),
                height: u64::from(copy_h),
                depth: 1,
            },
            atlas.texture.as_ref(),
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        atlas.width = width;
        atlas.height = height;
    }

    /// How many bytes the most recent [`Self::upload_masks`] sent — the
    /// whole texture after a full upload, the changed rectangles otherwise,
    /// zero when nothing changed. For tests and measurement.
    #[must_use]
    pub const fn last_mask_upload_bytes(&self) -> u64 {
        self.mask_upload_bytes
    }

    /// How many bytes the most recent glyph, image and ramp uploads sent —
    /// [`Self::last_mask_upload_bytes`]'s contract, per slot. For tests and
    /// measurement: the churn numbers the plan's parity suites compare
    /// between backends (glyphs about 153 KB against 2.56 MB whole, on the
    /// churn fixtures).
    #[must_use]
    pub const fn last_atlas_upload_bytes(&self) -> (u64, u64, u64) {
        (
            self.glyph_upload_bytes,
            self.image_upload_bytes,
            self.ramp_upload_bytes,
        )
    }

    /// Narrow the idle threshold that frees the renderer's surplus layer
    /// targets: how many frames of unused depth before they are freed. The
    /// default ([`TARGET_TRIM_FRAMES`]) is for a production process; a test
    /// that wants to *see* a trim renders far fewer frames than ten seconds
    /// of them. There is no staging threshold to narrow — this backend keeps
    /// no staging between uploads (see the module doc).
    pub fn set_memory_trim_frames(&mut self, frames: u32) {
        self.target_trim_frames = frames.max(1);
    }

    /// How many render targets the renderer holds — `L[n]` and `S[n]`
    /// together, the dummy excluded. For tests and measurement.
    #[must_use]
    pub fn target_count(&self) -> usize {
        self.levels.len() + self.scratch.len()
    }

    /// Whether the compute blur's pipeline was built — the route the blur
    /// modes dispatch when it is there. For tests and the A/B: the run that
    /// sets `VIEWW_GPU_COMPUTE_BLUR=0` answers `false` and the fragment
    /// hybrid takes the blur modes back.
    #[must_use]
    pub const fn compute_blur_built(&self) -> bool {
        self.blur.is_some()
    }
}

impl std::fmt::Debug for MetalSceneRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalSceneRenderer")
            .field("size", &self.size)
            .field("targets", &self.levels.len())
            .finish_non_exhaustive()
    }
}
