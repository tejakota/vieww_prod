//! Executing a [`vieww_gpu::ScenePlan`] on a real Vulkan device.
//!
//! # Shape
//!
//! [`SceneRenderer`] is built once per surface and keeps everything that does
//! not change between frames: two render passes, three graphics pipelines,
//! four texture resources and the descriptor sets that bind them. Per frame
//! it uploads whichever atlases changed, writes the vertex and index buffers,
//! records **one** command buffer for the plan's whole [`Step`](vieww_gpu::Step) program, and
//! submits it once.
//!
//! ```text
//!   targets  (RGBA16F, premultiplied, frame-sized)
//!     L[0]  the frame            S[0]  its scratch
//!     L[1]  first layer / work   S[1]  …
//!     …     up to plan.target_depth
//!
//!   pipelines
//!     scene         batched geometry, source-over     (Step::Draw)
//!     post_replace  full-screen, no blending          copy · blur · matrix · mask · blend
//!     post_over     full-screen, source-over          normal composite · shadow tint
//! ```
//!
//! # Why every target is RGBA16F and premultiplied
//!
//! `NativeRenderer` — the oracle this backend is tested against — composites
//! premultiplied `f32` pixels. A layer that is later scaled by group opacity,
//! blurred or pushed through `ColorBurn` must hold the same numbers the CPU
//! layer buffer would, and an 8-bit premultiplied buffer loses exactly the
//! low-alpha precision those operations amplify. RGBA16F is the smallest
//! format Vulkan *requires* to be renderable and blendable that does not.
//! The frame is quantised to 8 bits once, on readback, with the same rounding
//! `NativeRenderer` uses.
//!
//! # Damage: redrawing only what changed
//!
//! `L[0]` is kept between frames, so a frame that names its damage
//! ([`SceneRenderer::render_planned_damaged`], and `render_planned_to` —
//! the swapchain-image form, behind the `vulkan-swapchain` feature and so
//! spelled rather than linked, this doc builds without it)
//! redraws only those rectangles of it and leaves the rest as the previous
//! frame finished it. What is clipped is narrow on purpose: **only writes
//! into `L[0]` (and its scratch `S[0]`)**. Layers, blurs and shadow work run
//! over their full extent as before — a blur reads pixels around the ones
//! it writes, so clipping its input would bleed last frame into this one —
//! and are skipped outright when what they composite misses the damage.
//!
//! Two things make the kept pixels unusable, and the renderer checks both
//! itself rather than trusting a caller (`Retained`): a frame of a
//! different size or clear colour, and a backdrop filter at the top level
//! whose rectangle reaches outside the damage — it samples `L[0]` as drawn
//! *so far this frame*, which outside the damage is last frame's finished
//! picture. Either one redraws the frame in full. What it cannot check is
//! whether something else reached the screen since its last frame (a CPU
//! fallback frame, say); a caller that presented anything else passes no
//! damage.
//!
//! # Refusal
//!
//! [`SceneRenderer::render`] refuses a plan that is not
//! [`ScenePlan::is_complete`](vieww_gpu::ScenePlan::is_complete). A plan's remaining gaps are resource limits
//! (see `vieww_gpu::scene`); a frame with a missing photograph is a wrong
//! frame, not a degraded one. [`SceneRenderer::render_incomplete`] exists for
//! tools that want the partial result on purpose.

use std::collections::HashMap;
use std::ffi::CString;
use std::time::{Duration, Instant};

use ash::vk;
use vieww_foundation::Color;
use vieww_gpu::{PixelRect, Planner, ScenePlan, Vertex};

use crate::scene_exec::{frame_passes, PassKind, PostPush, Retained, SceneOp, TargetId};

use super::present_history::{self, PresentHistory};
#[cfg(feature = "vulkan-swapchain")]
use super::AcquiredImage;
use super::{translate_wgsl_to_spirv, Stage, VulkanDevice, VulkanError};

// The traversal itself, and the types it shares with this backend's
// interpreter, live in `crate::scene_exec` — one definition of what a frame
// does, consumed by this backend and the Metal/D3D12 ports alike.
pub(super) use crate::scene_exec::disjoint;
pub use crate::scene_exec::Redrawn;

/// The swapchain image a depth-0 frame renders into directly — see
/// [`SceneRenderer::render_planned_direct`]. No `L[0]`, no carry, no blit:
/// the render pass's own final layout hands the image to the present.
#[derive(Clone, Copy)]
struct DirectFrame {
    /// A framebuffer over the swapchain image, from
    /// [`SceneRenderer::direct_framebuffer`].
    framebuffer: vk::Framebuffer,
    /// Signalled by the presentation engine when the image may be written;
    /// the render pass waits on it at `COLOR_ATTACHMENT_OUTPUT`.
    ready: vk::Semaphore,
    /// Signalled when the frame's commands finish; the present waits on it.
    done: vk::Semaphore,
}

/// Pipelines, render passes and per-image framebuffers for rendering
/// depth-0 opaque frames straight into swapchain images — built once per
/// swapchain format, on the first frame that wants the route.
///
/// The scene and post pipelines are the same shaders and layouts as the
/// `RGBA16F` set, compiled against the swapchain's 8-bit format: a frame
/// rendered this way blends in 8 bits per channel, which every `u8` surface
/// UI stack does, and skips the `L[0]` load/store, the carry copies and the
/// f16→UNORM blit that dominated the frame's bandwidth on low-end GPUs.
#[cfg_attr(
    not(feature = "vulkan-swapchain"),
    expect(dead_code, reason = "the direct route renders into swapchain images")
)]
struct DirectRoute {
    format: vk::Format,
    /// Load-carrying pass (a pass re-opened without a clear).
    rp_load: vk::RenderPass,
    /// Clearing pass; `initialLayout` `UNDEFINED`, `finalLayout`
    /// `PRESENT_SRC_KHR`, so the pass itself performs the layout the present
    /// needs.
    rp_clear: vk::RenderPass,
    scene_pipeline: vk::Pipeline,
    post_replace: vk::Pipeline,
    post_over: vk::Pipeline,
    /// `(index, generation, view, framebuffer)` per swapchain image of the
    /// current generation; stale generations are pruned as new ones appear.
    frames: Vec<(u32, u64, vk::ImageView, vk::Framebuffer)>,
}

/// Where `execute` sends the finished frame instead of the readback buffer:
/// a presentable image, and the semaphore the blit into it must wait on.
#[derive(Clone, Copy)]
struct BlitTarget {
    image: vk::Image,
    ready: vk::Semaphore,
    /// Signalled when the frame's commands finish, for the present to wait
    /// on — see [`SceneRenderer::settle`].
    done: vk::Semaphore,
    /// Where the scene frame ends in its own coordinates: the render
    /// extent, which is the target's unless a resolution scale says
    /// otherwise (then the blit enlarges). `[0, 0]` corners, so one corner
    /// says it all.
    src_extent: [u32; 2],
    /// The filter the blit runs with: `NEAREST` at 1:1, `LINEAR` when the
    /// source is a different size — nearest-neighbour upscaling doubles
    /// pixels instead of blending them.
    filter: vk::Filter,
    /// The swapchain image's own extent: where the blit ends in it. Not
    /// `src_extent` — with a resolution scale on, those differ, and a blit
    /// to the render extent filled only the image's top-left corner.
    dst_extent: [u32; 2],
    /// For [`PresentHistory`]: which image, of which generation of the
    /// swapchain, made for partial blits or not, in which byte order.
    index: u32,
    generation: u64,
    partial: bool,
    bgra: bool,
}

/// A frame's image reads, looked at once the GPU has finished the frame —
/// see `present_history`.
#[derive(Debug, Clone, Copy)]
struct PendingProbe {
    index: u32,
    before: bool,
    after: bool,
    compare: bool,
    extent: (u32, u32),
    bgra: bool,
    /// The flight slot the probing frame was submitted on; its fence is
    /// what makes the probe's reads safe to collect.
    slot: usize,
}

/// Colour format of every render target.
const TARGET_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

/// Where a rendered frame ends up: copied back to host memory, blitted from
/// `L[0]` into a swapchain image, or — for a depth-0 opaque frame — rendered
/// straight into the swapchain image with no intermediate target at all.
///
/// `Blit` and `Direct` are only ever constructed by the
/// `vulkan-swapchain`-gated submits; `execute`'s match arms stay compiled so
/// the enum's one shape serves both builds.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(feature = "vulkan-swapchain"),
    expect(
        dead_code,
        reason = "only the swapchain submits construct Blit and Direct"
    )
)]
enum Out {
    Readback,
    Blit(BlitTarget),
    Direct(DirectFrame),
}

/// A persistent Vulkan executor for [`ScenePlan`]s. See the module doc.
pub struct SceneRenderer {
    /// A clone of the device handle (function table + `VkDevice`). The
    /// [`VulkanDevice`] this was built from owns the device and must outlive
    /// the renderer.
    device: ash::Device,
    queue: vk::Queue,
    queue_family: u32,
    host_visible_memory: u32,
    mem_props: vk::PhysicalDeviceMemoryProperties,

    rp_load: vk::RenderPass,
    rp_clear: vk::RenderPass,

    scene_pipeline: vk::Pipeline,
    scene_layout: vk::PipelineLayout,
    scene_set_layout: vk::DescriptorSetLayout,
    scene_pool: vk::DescriptorPool,
    /// The scene pipeline's descriptor set, one per flight slot (each bound
    /// to its own uniform).
    scene_sets: [vk::DescriptorSet; FLIGHT_SLOTS],

    post_replace: vk::Pipeline,
    post_over: vk::Pipeline,
    post_layout: vk::PipelineLayout,
    post_set_layout: vk::DescriptorSetLayout,
    post_pool: vk::DescriptorPool,
    post_sets: HashMap<(TargetId, TargetId), vk::DescriptorSet>,

    shaders: Vec<vk::ShaderModule>,
    command_pool: vk::CommandPool,
    /// The frames-in-flight ring. A frame rewrites its command buffer, its
    /// copy of the viewport uniform, its vertex and index buffers and its
    /// timestamp pool between `settle` and submit — all of it per slot, so
    /// the frame after it can do the same while the GPU still draws this
    /// one. See [`FlightSlot`] and [`FLIGHT_SLOTS`].
    slots: [FlightSlot; FLIGHT_SLOTS],
    /// Which slot the frame being recorded uses; `begin_commands` sets it.
    recording: usize,
    /// How many frames have been submitted. A frame records on slot
    /// `frames % FLIGHT_SLOTS`, so the slot it waits on held the frame
    /// `FLIGHT_SLOTS` before — long finished by then, or waited for here.
    frames: u64,
    /// The internal ordering chain between submissions: submission *n*
    /// signals `chain[n % 2]` on completion and submission *n + 1* waits it
    /// before any colour-attachment work. Every submission joins — frames
    /// and one-off uploads alike — so a frame's draws, carries, atlas
    /// uploads and blit never race the submission before them, while the
    /// CPU-side work (planning, recording, the submit call itself) still
    /// overlaps the GPU's.
    chain: [vk::Semaphore; 2],
    /// The chain semaphore the last submission signalled, for the next one
    /// to wait on. `None` before the first submission.
    chain_open: Option<vk::Semaphore>,
    /// Total submissions, frames and one-off uploads alike — the chain's
    /// parity, so every submission in queue order both waits and signals.
    submissions: u64,
    /// Whether the queue can timestamp, as (nanoseconds per tick,
    /// valid-bit mask). The pools themselves live per flight slot.
    timestamps: Option<(f32, u64)>,
    /// The device's pipeline cache — shared by the direct route's lazily
    /// built pipelines, so a format's first frame pays the compile once.
    #[cfg_attr(
        not(feature = "vulkan-swapchain"),
        expect(dead_code, reason = "only the direct route's pipelines read it")
    )]
    pipeline_cache: vk::PipelineCache,

    glyphs: Texture,
    images: Texture,
    masks: Texture,
    ramps: Texture,
    /// 1x1 transparent RGBA16F, bound wherever a post pass reads nothing.
    dummy: Target,

    size: (u32, u32),
    levels: Vec<Target>,
    scratch: Vec<Target>,
    readback: Buffer,
    /// Bytes the last [`SceneRenderer::upload_masks`] sent.
    mask_upload_bytes: u64,
    /// Bytes the last glyph-atlas upload sent — see
    /// [`Self::last_glyph_upload_bytes`].
    glyph_upload_bytes: u64,
    /// Bytes the last image-atlas upload sent — see
    /// [`Self::last_image_upload_bytes`].
    image_upload_bytes: u64,
    /// Bytes the last ramp-atlas upload sent — see
    /// [`Self::last_ramp_upload_bytes`].
    ramp_upload_bytes: u64,
    /// Where the last frame's time went — see [`GpuFrameTimings`].
    timings: GpuFrameTimings,
    /// What `L[0]` holds between frames — see [`Retained`].
    retained: Option<Retained>,
    /// The rectangles of `L[0]` the frame being recorded redraws; `None`
    /// redraws all of it. Set by `execute` for the length of one frame.
    frame_clip: Option<Vec<PixelRect>>,
    /// Pixels the next damaged frame carries inside `L[0]` before it
    /// redraws anything: `(src, dx, dy)` in render pixels — a scroll's
    /// moved rows. See [`set_pending_copies`](Self::set_pending_copies).
    pending_copies: Vec<(PixelRect, i32, i32)>,
    /// Atlas textures replaced by [`resize_with_carry`](Self::resize_with_carry):
    /// their content was copied into the replacement **on the command buffer
    /// being recorded**, so they cannot be destroyed until that buffer has
    /// been submitted and finished. Drained by [`settle`](Self::settle).
    retired_textures: Vec<Texture>,
    /// Per flight slot, the texture bindings (bit = [`Slot::binding`]) its
    /// scene set still points at a replaced texture for. A resize of the
    /// glyph, image or ramp texture rewrites the recording slot's set at
    /// once and leaves the others here — they may belong to frames still in
    /// flight, whose sets cannot be written — for [`begin_commands`](Self::begin_commands)
    /// to rewrite when that slot is next recorded. That is what lets the
    /// resize skip the device-wide wait it used to take: the first scroll
    /// of a text-heavy screen grew the glyph atlas mid-phase, and the wait
    /// was a visible hitch on every phone in the 2026-11-18 Test Lab run.
    stale_scene_bindings: [u8; FLIGHT_SLOTS],
    /// Consecutive frames whose plans used fewer targets than are allocated
    /// — the idle signal that frees the surplus. See [`TARGET_TRIM_FRAMES`].
    idle_target_frames: u32,
    /// How many idle frames before the target surplus is freed —
    /// [`TARGET_TRIM_FRAMES`] unless a test narrowed it. See
    /// [`set_memory_trim_frames`](Self::set_memory_trim_frames).
    target_trim_frames: u32,
    /// How many idle frames before an oversized staging buffer is released —
    /// [`STAGING_TRIM_FRAMES`] unless a test narrowed it. Same setter.
    staging_trim_frames: u32,
    /// What the last frame whose timestamps were read spent on the GPU.
    pass_times: Option<GpuPassTimes>,
    /// Whether a frame into a swapchain image is left in flight
    /// ([`set_overlap`](Self::set_overlap)).
    overlap: bool,
    /// Time spent in [`settle`](Self::settle) since the last frame was
    /// recorded: charged to the next frame's [`GpuFrameTimings::wait`].
    settled_wait: Duration,
    /// What each swapchain image holds, so a frame copies only what changed
    /// — see `present_history`.
    presents: PresentHistory,
    /// The direct route's resources, built on the first frame that wants it
    /// — see [`DirectRoute`].
    direct: Option<DirectRoute>,
    /// Host-visible buffers a swapchain image is read into, before and
    /// after a frame's copy, while partial copies are being verified.
    probe_before: Buffer,
    probe_after: Buffer,
    /// The last frame's reads, until its fence has signalled.
    pending_probe: Option<PendingProbe>,
}

/// Where one frame's time went inside [`SceneRenderer`], on the CPU's clock.
///
/// # Why
///
/// The audit's Android finding was one number: a 21 ms median GPU frame on an
/// Adreno 612. A median of a whole frame cannot say whether the time went to
/// moving atlases and geometry into GPU-visible memory, to building the command
/// buffer, to the driver's submit, to the GPU itself, or to the readback the
/// non-direct path does — and each of those has a different fix (smaller
/// uploads; cached command buffers; fewer submits; less GPU work; rendering
/// straight into the swapchain image). So each is measured separately, every
/// frame, at no cost beyond five `Instant::now()` calls.
///
/// # What the fields can and cannot say
///
/// These are host-side wall-clock durations, not GPU timestamps. `wait` is
/// the time the CPU blocked on a fence. On a frame read back, or with
/// [`SceneRenderer::set_overlap`] off, that is this frame's own fence, and a
/// fair upper bound on its GPU time. With frames overlapping (the default for
/// a frame into a swapchain image) it is the time this frame waited for the
/// **previous** one before it could touch the renderer's resources — the part
/// of the GPU's work the application's own work did not cover. Either way the
/// GPU's own time is [`GpuPassTimes`], from timestamp queries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuFrameTimings {
    /// Planner atlases (glyphs, images, masks, ramps), uniforms, vertex and
    /// index data, and any target (re)allocation — everything that has to be
    /// in GPU-visible memory before the frame can be recorded. Staging
    /// uploads submit and wait on their own, and that wait is counted here.
    pub upload: Duration,
    /// Building the frame's command buffer on the CPU.
    pub record: Duration,
    /// The `vkQueueSubmit` call itself (plus ending the command buffer).
    pub submit: Duration,
    /// Blocked on a fence: this frame's, or the previous frame's when they
    /// overlap — see the type docs.
    pub wait: Duration,
    /// Mapping the readback buffer and converting RGBA16F to RGBA8. Zero on
    /// the direct-to-swapchain path, which has no readback.
    pub readback: Duration,
}

impl GpuFrameTimings {
    /// The sum of every stage.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.upload + self.record + self.submit + self.wait + self.readback
    }
}

/// Where one frame's time went **on the GPU**, by kind of work, from
/// timestamp queries — what [`GpuFrameTimings::wait`] cannot say, because
/// that is one number for the whole frame on the CPU's clock.
///
/// The question behind it: on the LG Stylo 5 (Device Farm run 39538891917)
/// the GPU spent about 12 ms on a frame whose damage was 8.6% of the
/// screen. Plain geometry redrawn under the damage, or layers, filters and
/// shadows recomputed over their full extent (the module docs say why they
/// are), are different problems with different fixes; these fields say which.
///
/// # How it is measured
///
/// A timestamp is written each time the frame's commands change kind, and
/// the interval up to the next one is charged to the kind that began it.
/// Timestamps are written at `BOTTOM_OF_PIPE`, so an interval is the time
/// between one group of work finishing and the next finishing: on a GPU
/// that overlaps adjacent work this is an attribution, not an isolated
/// cost, and the fields sum to the frame's GPU time ([`total`](Self::total))
/// rather than to the sum of what each would cost alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuPassTimes {
    /// Geometry drawn into the frame or a layer (`Step::Draw`), including
    /// clearing the damaged rectangles.
    pub draw: Duration,
    /// Layer copies, masks and composites (`PushLayer` / `PopLayer`).
    pub layers: Duration,
    /// Filters: blurs, colour matrices — a layer's or a backdrop's.
    pub filters: Duration,
    /// Box shadows: seed, blur, inset and tint.
    pub shadows: Duration,
    /// The frame's last step: the blit into the swapchain image, or the
    /// copy into the readback buffer.
    pub blit: Duration,
}

impl GpuPassTimes {
    /// The frame's GPU time: every kind, summed.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.draw + self.layers + self.filters + self.shadows + self.blit
    }
}

/// Timestamp slots per frame. A kind changes a few times per layer, filter
/// or shadow; a frame that changes more often than this charges the rest to
/// the kind it was in when the slots ran out (`Queries::overflowed`).
const QUERY_SLOTS: u32 = 128;

/// One slot of the frames-in-flight ring: everything a frame rewrites on
/// the CPU between `settle` and submit. Two frames may be recorded and
/// submitted back to back; a new frame takes the slot of the frame
/// [`FLIGHT_SLOTS`] before it and waits for that frame's fence first, so
/// nothing the GPU may still be reading is rewritten.
struct FlightSlot {
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
    /// This slot's copy of the viewport uniform, and the scene descriptor
    /// set bound to it — a submission in flight reads its own slot's set.
    uniform: Buffer,
    vertices: Buffer,
    indices: Buffer,
    /// This slot's timestamp pool, when the queue can timestamp at all.
    queries: Option<SlotQueries>,
    /// A frame submitted on this slot that the CPU has not waited for yet.
    in_flight: bool,
}

/// The per-slot half of the timestamp queries: one pool reset by the frame
/// recording on this slot, and that frame's kind list. The period and
/// valid-bit mask are properties of the queue, held once on the renderer.
struct SlotQueries {
    pool: vk::QueryPool,
    /// The kind each written timestamp begins; the last one ends the frame.
    kinds: Vec<PassKind>,
    /// Whether this frame ran out of slots.
    overflowed: bool,
    /// A submitted frame whose timestamps have not been read yet.
    pending: bool,
}

/// How many frames may be recorded and submitted before the oldest has to
/// be waited for. Two, so a frame's CPU half — planning, atlas uploads,
/// command recording, the submit call — runs while the GPU draws the frame
/// before it. The GPU halves still run one after the other (the chain on
/// the renderer orders every submission against the previous one, because
/// a retained frame's damage renders into what the previous frame left in
/// `L[0]` and its atlas uploads overwrite slots the previous frame may
/// still sample); what overlaps is the CPU's work with the GPU's, which on
/// the phones that need it — the Redmi's `render_planned_to` route measured
/// 7.5 ms of planning plus 5 ms of recording after a 12 ms wait for the
/// previous frame's 21 ms — was most of the frame.
const FLIGHT_SLOTS: usize = 2;

/// How many consecutive frames a renderer may hold more layer targets than
/// any of them used before the surplus is freed: ~10 s at 60 Hz. Depth grows
/// on the first frames of a scene or the first layer of a new one, and one
/// deep frame — a keyboard popup, a six-deep stack of glass — pins
/// `2 * depth` full-frame RGBA16F targets (39 MB *each* at 1080x2340) for
/// the life of the renderer, because nothing ever shrank the list. The
/// hysteresis is what keeps a scene that honestly oscillates between two
/// depths from thrashing: ten seconds of the shallower depth is a decision,
/// not a jitter.
const TARGET_TRIM_FRAMES: u32 = 600;

struct Buffer {
    handle: vk::Buffer,
    memory: vk::DeviceMemory,
    capacity: u64,
}

impl Buffer {
    const EMPTY: Self = Self {
        handle: vk::Buffer::null(),
        memory: vk::DeviceMemory::null(),
        capacity: 0,
    };
}

/// A render target: image, memory, view and a framebuffer for it.
struct Target {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
}

impl Target {
    const NULL: Self = Self {
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        view: vk::ImageView::null(),
        framebuffer: vk::Framebuffer::null(),
    };
}

/// A sampled texture fed from a CPU atlas.
struct Texture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    format: vk::Format,
    width: u32,
    height: u32,
    /// The atlas version last uploaded; `None` for the initial placeholder.
    version: Option<u64>,
    /// One staging buffer per flight slot: the frame recording an upload
    /// writes its own while the frame before it may still be copying out of
    /// its.
    staging: [Buffer; FLIGHT_SLOTS],
    /// Consecutive frames each flight slot's staging has held at least
    /// [`STAGING_TRIM_FACTOR`] times the bytes the partial uploads actually
    /// sent — the idle signal that releases it back to `EMPTY`. A full
    /// upload (a grow, a compaction) uses the whole buffer and resets its
    /// slot's counter; see `record_regions`.
    staging_idle: [u16; FLIGHT_SLOTS],
}

/// How much larger than the partial uploads' bytes a staging buffer must be,
/// and for how many frames, before it is released: 8x for ~4 s at 60 Hz. The
/// factor keeps a buffer that is *sometimes* the right size — an atlas whose
/// compactions alternate with quiet frames — while still retiring the 64 MB
/// image-atlas staging a single full upload once pinned forever.
const STAGING_TRIM_FACTOR: u64 = 8;
const STAGING_TRIM_FRAMES: u32 = 240;

impl Texture {
    const NULL: Self = Self {
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        view: vk::ImageView::null(),
        format: vk::Format::UNDEFINED,
        width: 0,
        height: 0,
        version: None,
        staging: [Buffer::EMPTY; FLIGHT_SLOTS],
        staging_idle: [0; FLIGHT_SLOTS],
    };
}

/// Which texture slot, for uploads and descriptor rebinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Glyphs,
    Images,
    Masks,
    Ramps,
}

/// The public name of a `Slot`, for [`SceneRenderer::staging_capacity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingSlot {
    Glyphs,
    Images,
    Masks,
    Ramps,
}

impl StagingSlot {
    const fn as_slot(self) -> Slot {
        match self {
            Self::Glyphs => Slot::Glyphs,
            Self::Images => Slot::Images,
            Self::Masks => Slot::Masks,
            Self::Ramps => Slot::Ramps,
        }
    }
}

impl Slot {
    const ALL: [Self; 4] = [Self::Glyphs, Self::Images, Self::Masks, Self::Ramps];

    const fn binding(self) -> u32 {
        match self {
            Self::Glyphs => 1,
            Self::Images => 2,
            Self::Masks => 3,
            Self::Ramps => 4,
        }
    }
}

/// Which pipeline the command buffer last bound — the interpreter's bind
/// dedup. `Post(over)` remembers which of the two post pipelines, because a
/// run of post passes alternates between them.
#[derive(Clone, Copy, PartialEq)]
enum Bound {
    None,
    Scene,
    /// The post pipeline, and whether it is the blending (`over`) one.
    Post(bool),
}

impl SceneRenderer {
    /// Build the render passes, pipelines and placeholder textures.
    ///
    /// # Errors
    ///
    /// Any Vulkan object failing to be created, or a shader failing to
    /// translate (a bug in this workspace, surfaced here).
    pub fn new(device: &VulkanDevice) -> Result<Self, VulkanError> {
        let handle = device.raw_device().clone();
        let host_visible = device.find_memory_type(
            u32::MAX,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let mem_props = device.memory_properties();

        let rp_load = create_render_pass(
            &handle,
            vk::AttachmentLoadOp::LOAD,
            TARGET_FORMAT,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )?;
        let rp_clear = create_render_pass(
            &handle,
            vk::AttachmentLoadOp::CLEAR,
            TARGET_FORMAT,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )?;

        // ── descriptor layouts ──────────────────────────────────────────────
        let scene_bindings: Vec<_> = (0..5u32)
            .map(|binding| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(if binding == 0 {
                        vk::DescriptorType::UNIFORM_BUFFER
                    } else {
                        vk::DescriptorType::SAMPLED_IMAGE
                    })
                    .descriptor_count(1)
                    .stage_flags(if binding == 0 {
                        vk::ShaderStageFlags::VERTEX
                    } else {
                        vk::ShaderStageFlags::FRAGMENT
                    })
            })
            .collect();
        let scene_set_layout = unsafe {
            handle.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&scene_bindings),
                None,
            )
        }?;
        let post_bindings: Vec<_> = (0..3u32)
            .map(|binding| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            })
            .collect();
        let post_set_layout = unsafe {
            handle.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&post_bindings),
                None,
            )
        }?;

        let scene_pool = unsafe {
            handle.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&[
                        vk::DescriptorPoolSize {
                            ty: vk::DescriptorType::UNIFORM_BUFFER,
                            // One uniform per set, one set per flight slot.
                            descriptor_count: FLIGHT_SLOTS as u32,
                        },
                        vk::DescriptorPoolSize {
                            ty: vk::DescriptorType::SAMPLED_IMAGE,
                            // The scene set binds four sampled images
                            // (bindings 1..4), so `FLIGHT_SLOTS` sets draw
                            // `4 * FLIGHT_SLOTS` of them from the pool. This
                            // said `4` when the ring held one frame, and the
                            // second slot's allocation made
                            // `vkAllocateDescriptorSets` answer
                            // `VK_ERROR_OUT_OF_POOL_MEMORY` on the drivers
                            // that enforce a pool's declared counts — the
                            // S26 Ultra's Adreno (Device Farm run 43722791555:
                            // the GPU pass presented nothing, its logcat one
                            // line: "opening a Vulkan device/surface: Vulkan:
                            // ERROR_OUT_OF_POOL_MEMORY"), while the Stylo's
                            // older driver let the overdrawn pool pass.
                            descriptor_count: 4 * FLIGHT_SLOTS as u32,
                        },
                    ])
                    .max_sets(FLIGHT_SLOTS as u32),
                None,
            )
        }?;
        let scene_sets: [vk::DescriptorSet; FLIGHT_SLOTS] = unsafe {
            handle
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(scene_pool)
                        .set_layouts(&[scene_set_layout; FLIGHT_SLOTS]),
                )
                .map(|sets| sets.try_into().expect("one set per flight slot"))
        }?;
        let post_pool = create_post_pool(&handle)?;

        // ── shaders and pipelines ───────────────────────────────────────────
        let sources = [
            (vieww_shaders::library::SCENE_WGSL, Stage::Vertex, "vs_main"),
            (
                vieww_shaders::library::SCENE_WGSL,
                Stage::Fragment,
                "fs_main",
            ),
            (vieww_shaders::library::POST_WGSL, Stage::Vertex, "vs_main"),
            (
                vieww_shaders::library::POST_WGSL,
                Stage::Fragment,
                "fs_main",
            ),
        ];
        let mut shaders = Vec::new();
        for (source, stage, entry) in sources {
            let code = translate_wgsl_to_spirv(source, entry, stage)?;
            shaders.push(unsafe {
                handle
                    .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
            }?);
        }

        let scene_layout = unsafe {
            handle.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[scene_set_layout]),
                None,
            )
        }?;
        let push_ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: std::mem::size_of::<PostPush>() as u32,
        }];
        let post_layout = unsafe {
            handle.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[post_set_layout])
                    .push_constant_ranges(&push_ranges),
                None,
            )
        }?;

        let f = std::mem::size_of::<f32>() as u32;
        let (scene_binding, scene_attributes) = scene_vertex_layout();
        // 23 floats plus the packed 4-byte colour. A shipped swapped-offset
        // bug once escaped review (GPU-RENDERER-STATUS: only a pixel
        // comparison caught it), so the stride is pinned at creation: a
        // `Vertex` that drifts from the attribute table fails here, on the
        // device, before any frame draws wrong.
        assert_eq!(
            std::mem::size_of::<Vertex>() as u32,
            23 * f + 4,
            "vertex layout drifted"
        );

        let scene_pipeline = create_pipeline(
            (&handle, device.pipeline_cache()),
            rp_load,
            scene_layout,
            (shaders[0], shaders[1]),
            &[scene_binding],
            &scene_attributes,
            true,
        )?;
        let post_replace = create_pipeline(
            (&handle, device.pipeline_cache()),
            rp_load,
            post_layout,
            (shaders[2], shaders[3]),
            &[],
            &[],
            false,
        )?;
        let post_over = create_pipeline(
            (&handle, device.pipeline_cache()),
            rp_load,
            post_layout,
            (shaders[2], shaders[3]),
            &[],
            &[],
            true,
        )?;

        let command_pool = unsafe {
            handle.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(device.queue_family_index())
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }?;
        let command_buffers: [vk::CommandBuffer; FLIGHT_SLOTS] = unsafe {
            handle
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(command_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(FLIGHT_SLOTS as u32),
                )
                .map(|buffers| {
                    buffers
                        .try_into()
                        .expect("one command buffer per flight slot")
                })
        }?;
        let fences: [vk::Fence; FLIGHT_SLOTS] = {
            let mut fences = [vk::Fence::null(); FLIGHT_SLOTS];
            for fence in &mut fences {
                *fence = unsafe { handle.create_fence(&vk::FenceCreateInfo::default(), None) }?;
            }
            fences
        };
        let chain: [vk::Semaphore; 2] = {
            let mut chain = [vk::Semaphore::null(); 2];
            for semaphore in &mut chain {
                *semaphore =
                    unsafe { handle.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? };
            }
            chain
        };
        let mut uniforms = [Buffer::EMPTY; FLIGHT_SLOTS];
        for uniform in &mut uniforms {
            *uniform = allocate_buffer(
                &handle,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                host_visible,
                std::mem::size_of::<[f32; 4]>() as u64,
            )?;
        }
        let glyphs = create_texture(&handle, &mem_props, vk::Format::R8_UNORM, 1, 1)?;
        let images = create_texture(&handle, &mem_props, vk::Format::R8G8B8A8_UNORM, 1, 1)?;
        let masks = create_texture(&handle, &mem_props, vk::Format::R8_UNORM, 1, 1)?;
        let ramps = create_texture(&handle, &mem_props, vk::Format::R32G32B32A32_SFLOAT, 1, 1)?;
        let dummy = create_target(&handle, &mem_props, rp_load, 1, 1)?;
        let queries = match device.timestamp_support() {
            Some((period, bits)) => {
                let mask = if bits >= 64 {
                    u64::MAX
                } else {
                    (1u64 << bits) - 1
                };
                let mut pools = [const { None }; FLIGHT_SLOTS];
                for queries in &mut pools {
                    let pool = unsafe {
                        handle.create_query_pool(
                            &vk::QueryPoolCreateInfo::default()
                                .query_type(vk::QueryType::TIMESTAMP)
                                .query_count(QUERY_SLOTS),
                            None,
                        )
                    }?;
                    *queries = Some(SlotQueries {
                        pool,
                        kinds: Vec::with_capacity(QUERY_SLOTS as usize),
                        overflowed: false,
                        pending: false,
                    });
                }
                Some((period, mask, pools))
            }
            None => None,
        };
        let (timestamps, slot_queries) = match queries {
            Some((period, mask, pools)) => (Some((period, mask)), pools),
            None => (None, [const { None }; FLIGHT_SLOTS]),
        };
        let mut slot_queries = slot_queries;
        let slots: [FlightSlot; FLIGHT_SLOTS] = {
            let mut slots = [const { None }; FLIGHT_SLOTS];
            for (index, slot) in slots.iter_mut().enumerate() {
                *slot = Some(FlightSlot {
                    command_buffer: command_buffers[index],
                    fence: fences[index],
                    uniform: std::mem::replace(&mut uniforms[index], Buffer::EMPTY),
                    vertices: Buffer::EMPTY,
                    indices: Buffer::EMPTY,
                    queries: slot_queries[index].take(),
                    in_flight: false,
                });
            }
            slots.map(|slot| slot.expect("every slot was filled above"))
        };

        let mut renderer = Self {
            device: handle,
            queue: device.raw_queue(),
            queue_family: device.queue_family_index(),
            host_visible_memory: host_visible,
            mem_props,
            rp_load,
            rp_clear,
            scene_pipeline,
            scene_layout,
            scene_set_layout,
            scene_pool,
            scene_sets,
            post_replace,
            post_over,
            post_layout,
            post_set_layout,
            post_pool,
            post_sets: HashMap::new(),
            shaders,
            command_pool,
            slots,
            recording: 0,
            frames: 0,
            chain,
            chain_open: None,
            submissions: 0,
            timestamps,
            pipeline_cache: device.pipeline_cache(),
            glyphs,
            images,
            masks,
            ramps,
            dummy,
            size: (0, 0),
            levels: Vec::new(),
            scratch: Vec::new(),
            readback: Buffer::EMPTY,
            mask_upload_bytes: 0,
            glyph_upload_bytes: 0,
            image_upload_bytes: 0,
            ramp_upload_bytes: 0,
            timings: GpuFrameTimings::default(),
            retained: None,
            idle_target_frames: 0,
            target_trim_frames: TARGET_TRIM_FRAMES,
            staging_trim_frames: STAGING_TRIM_FRAMES,
            retired_textures: Vec::new(),
            stale_scene_bindings: [0; FLIGHT_SLOTS],
            frame_clip: None,
            pending_copies: Vec::new(),
            pass_times: None,
            overlap: true,
            settled_wait: Duration::ZERO,
            presents: PresentHistory::new(),
            direct: None,
            probe_before: Buffer::EMPTY,
            probe_after: Buffer::EMPTY,
            pending_probe: None,
        };

        // Each slot's scene set reads its own uniform.
        for index in 0..FLIGHT_SLOTS {
            let uniform_info = [vk::DescriptorBufferInfo {
                buffer: renderer.slots[index].uniform.handle,
                offset: 0,
                range: renderer.slots[index].uniform.capacity,
            }];
            unsafe {
                renderer.device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(renderer.scene_sets[index])
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(&uniform_info)],
                    &[],
                );
            }
        }
        // A new image's contents are undefined; every placeholder is written
        // (zeros) and bound before anything can read it.
        for slot in Slot::ALL {
            let bytes = vec![0u8; bytes_per_texel(renderer.texture(slot).format)];
            renderer.upload(slot, &bytes, 1, 1, None, true)?;
        }
        renderer.initialise_targets(&[TargetId::Dummy])?;
        Ok(renderer)
    }

    /// The queue family this renderer records for.
    #[must_use]
    pub const fn queue_family(&self) -> u32 {
        self.queue_family
    }

    /// Whether the probe awaiting collection came from `index`'s slot.
    fn pending_probe_is(&self, index: usize) -> bool {
        self.pending_probe
            .as_ref()
            .is_some_and(|probe| probe.slot == index)
    }

    /// The command buffer of the slot being recorded.
    fn cmd(&self) -> vk::CommandBuffer {
        self.slots[self.recording].command_buffer
    }

    /// The slot the next frame records on — and therefore the one whose
    /// frame, `FLIGHT_SLOTS` back, has to be finished before it does.
    fn next_slot(&self) -> usize {
        #[expect(clippy::cast_possible_truncation, reason = "two slots")]
        let index = (self.frames % FLIGHT_SLOTS as u64) as usize;
        index
    }

    /// Wait for the frame on `index`'s slot, if there is one, and read its
    /// GPU timestamps. After this, that slot's command buffer, fence,
    /// uniform, geometry buffers and query pool are the CPU's again.
    fn settle_slot(&mut self, index: usize) -> Result<(), VulkanError> {
        if !self.slots[index].in_flight {
            return Ok(());
        }
        let waiting = Instant::now();
        unsafe {
            self.device
                .wait_for_fences(&[self.slots[index].fence], true, u64::MAX)?;
        }
        self.settled_wait += waiting.elapsed();
        self.slots[index].in_flight = false;
        self.collect_marks(index);
        if self.pending_probe_is(index) {
            self.collect_probe()?;
        }
        Ok(())
    }

    /// Wait for the slot the next frame wants, leaving every other frame
    /// in flight — the overlap the ring exists for — and return that
    /// slot's index. A caller about to acquire a swapchain image wants
    /// exactly this: the acquire semaphore the next frame signals is keyed
    /// to the slot, and its previous signal belonged to the frame two back,
    /// which this just finished.
    ///
    /// # Errors
    ///
    /// The fence wait failing (a lost device).
    pub fn settle_for_next_frame(&mut self) -> Result<usize, VulkanError> {
        let index = self.next_slot();
        self.settle_slot(index)?;
        Ok(index)
    }

    /// Upload the parts of the glyph atlas that changed — [`Self::upload_masks`]
    /// for callers that manage a `vieww_gpu::Atlas` outside a [`Planner`]:
    /// only the rectangles that changed, when the atlas can say which ones
    /// those are.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    pub fn upload_atlas(&mut self, atlas: &vieww_gpu::Atlas) -> Result<(), VulkanError> {
        self.settle()?;
        self.begin_commands(0)?;
        self.record_glyphs(atlas)?;
        self.submit_and_wait(None).map(|_| ())
    }

    /// Upload every atlas `planner` holds that changed since the last upload,
    /// as **one submission of their own** — for callers that upload without
    /// rendering a frame straight after.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    pub fn upload_planner(&mut self, planner: &Planner) -> Result<(), VulkanError> {
        self.settle()?;
        self.begin_commands(0)?;
        self.record_planner_uploads(planner)?;
        self.submit_and_wait(None).map(|_| ())
    }

    /// Record every changed atlas `planner` holds into the **open** command
    /// buffer, so the frame that follows records around it and the whole
    /// frame — uploads, geometry, passes, blit — is one submission.
    ///
    /// Each changed atlas used to be its own `submit_and_wait`: a mid-frame
    /// CPU block and pipeline drain apiece, which a storm of new sparkline
    /// masks paid every frame. Recorded instead, the copies' own barriers
    /// order them ahead of the render pass that samples them, and the frame
    /// pays one submit.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_planner_uploads(&mut self, planner: &Planner) -> Result<(), VulkanError> {
        self.record_glyphs(planner.atlas())?;
        self.record_images(planner.image_atlas())?;
        self.record_masks(planner.mask_atlas())?;
        self.record_ramps(planner.ramp_atlas())
    }

    /// Bring the mask texture up to `masks`, sending only the rectangles that
    /// changed when the atlas can say which ones those are.
    ///
    /// # Why not just the whole-texture upload
    ///
    /// A non-analytic shape that changes every frame — anything rotating, a
    /// morphing path — adds a new mask every frame, and a whole-texture upload
    /// on every version change made each of those frames send the entire
    /// atlas: `vieww-gpu`'s `examples/anim_paths` measured 4.2 MB a frame for
    /// one rotating panel and 16.8 MB for three moving shapes, against masks
    /// of a few hundred kilobytes. [`vieww_gpu::MaskAtlas::changes_since`]
    /// answers which texels differ from the version this texture holds; the
    /// rest of the texture is kept (the layout transition is from
    /// `SHADER_READ_ONLY_OPTIMAL`, not `UNDEFINED`, so the driver may not
    /// discard it). Anything it cannot vouch for is a full upload, as before.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    pub fn upload_masks(&mut self, masks: &vieww_gpu::MaskAtlas) -> Result<(), VulkanError> {
        self.settle()?;
        self.begin_commands(0)?;
        self.record_masks(masks)?;
        self.submit_and_wait(None).map(|_| ())
    }

    /// [`Self::upload_masks`], recorded into the open command buffer — the
    /// folded form [`Self::record_planner_uploads`] uses.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_masks(&mut self, masks: &vieww_gpu::MaskAtlas) -> Result<(), VulkanError> {
        let held = self.texture(Slot::Masks).version;
        let side = masks.side();
        let same_size = {
            let t = self.texture(Slot::Masks);
            t.width == side && t.height == side
        };
        self.mask_upload_bytes = 0;
        let rects = match masks.changes_since(held) {
            vieww_gpu::MaskChanges::None => return Ok(()),
            vieww_gpu::MaskChanges::Regions(rects) if same_size => rects,
            _ => {
                self.record_upload(
                    Slot::Masks,
                    masks.texels(),
                    side,
                    side,
                    Some(masks.version()),
                    false,
                )?;
                self.mask_upload_bytes = u64::from(side) * u64::from(side);
                return Ok(());
            }
        };
        let version = Some(masks.version());
        self.mask_upload_bytes =
            self.record_regions(Slot::Masks, masks.texels(), 1, side, version, rects)?;
        Ok(())
    }

    /// Bring the glyph texture up to `atlas` the way [`Self::record_masks`]
    /// brings the mask texture up: only the rectangles that changed, when the
    /// atlas can say which ones those are.
    ///
    /// # Why not just the whole-texture upload
    ///
    /// The glyph atlas never evicts, so its version moves only when a *new*
    /// glyph is rasterised — and one new glyph is a few hundred coverage
    /// bytes, where the whole texture is up to 4096x4096 of `R8`, 16 MB. The
    /// frames that pay are the almost-steady ones: a line of text scrolling
    /// in, a font fallback appearing — one new glyph per frame, each a full
    /// re-upload before this existed.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_glyphs(&mut self, atlas: &vieww_gpu::Atlas) -> Result<(), VulkanError> {
        let held = self.texture(Slot::Glyphs).version;
        let side = atlas.side();
        let same_size = {
            let t = self.texture(Slot::Glyphs);
            t.width == side && t.height == side
        };
        self.glyph_upload_bytes = 0;
        let rects = match atlas.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A grow, carried.** `Regions` with a resized texture means
                // the atlas grew (a compaction would answer `Full`): every
                // texel kept its position, so the old texture's content is
                // still valid — one GPU copy carries it across, and only the
                // rectangles come from the host. A new glyph no longer
                // re-sends the whole (up to 16 MB) texture.
                self.resize_with_carry(Slot::Glyphs, side, side)?;
                rects
            }
            _ => {
                self.record_upload(
                    Slot::Glyphs,
                    atlas.texels(),
                    side,
                    side,
                    Some(atlas.version()),
                    false,
                )?;
                self.glyph_upload_bytes = u64::from(side) * u64::from(side);
                return Ok(());
            }
        };
        let version = Some(atlas.version());
        self.glyph_upload_bytes =
            self.record_regions(Slot::Glyphs, atlas.texels(), 1, side, version, rects)?;
        Ok(())
    }

    /// Bring the image texture up to `images` the way [`Self::record_masks`]
    /// brings the mask texture up. An insert's rectangle is the image plus
    /// its one-texel padding ring (linear sampling reads the ring, so it
    /// changes with the image); a frame that decodes one image sends that
    /// image's texels instead of the whole (up to 64 MB) texture.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_images(&mut self, images: &vieww_gpu::ImageAtlas) -> Result<(), VulkanError> {
        let held = self.texture(Slot::Images).version;
        let side = images.side();
        let same_size = {
            let t = self.texture(Slot::Images);
            t.width == side && t.height == side
        };
        self.image_upload_bytes = 0;
        let rects = match images.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A grow, carried** — see `record_glyphs`' arm. This is the
                // 64 MB one: a photo app's first overflow image used to
                // re-send the entire atlas.
                self.resize_with_carry(Slot::Images, side, side)?;
                rects
            }
            _ => {
                self.record_upload(
                    Slot::Images,
                    images.texels(),
                    side,
                    side,
                    Some(images.version()),
                    false,
                )?;
                self.image_upload_bytes = u64::from(side) * u64::from(side) * 4;
                return Ok(());
            }
        };
        let version = Some(images.version());
        self.image_upload_bytes =
            self.record_regions(Slot::Images, images.texels(), 4, side, version, rects)?;
        Ok(())
    }

    /// Bring the ramp texture up to `ramps` the way [`Self::record_masks`]
    /// brings the mask texture up. A new ramp is exactly one row — 4 KB, the
    /// smallest write of the four atlases — and the only unit of ramp churn:
    /// a gradient whose stops animate is a new row every frame, which
    /// whole-texture uploads paid at 4 KB times the row count, every frame,
    /// growing until the compaction.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_ramps(&mut self, ramps: &vieww_gpu::RampAtlas) -> Result<(), VulkanError> {
        let held = self.texture(Slot::Ramps).version;
        let rows = ramps.rows();
        let same_size = {
            let t = self.texture(Slot::Ramps);
            t.width == vieww_gpu::RampAtlas::WIDTH && t.height == rows
        };
        self.ramp_upload_bytes = 0;
        let rects = match ramps.changes_since(held) {
            vieww_gpu::AtlasChanges::None => return Ok(()),
            vieww_gpu::AtlasChanges::Regions(rects) if same_size => rects,
            vieww_gpu::AtlasChanges::Regions(rects) if held.is_some() => {
                // **A row doubling, carried** — see `record_glyphs`' arm. The
                // copy is the old rows; the rectangles are the new ones.
                self.resize_with_carry(Slot::Ramps, vieww_gpu::RampAtlas::WIDTH, rows)?;
                rects
            }
            _ => {
                self.record_upload(
                    Slot::Ramps,
                    as_bytes(ramps.texels()),
                    vieww_gpu::RampAtlas::WIDTH,
                    rows,
                    Some(ramps.version()),
                    false,
                )?;
                self.ramp_upload_bytes =
                    u64::from(vieww_gpu::RampAtlas::WIDTH) * u64::from(rows) * 16;
                return Ok(());
            }
        };
        let version = Some(ramps.version());
        self.ramp_upload_bytes = self.record_regions(
            Slot::Ramps,
            as_bytes(ramps.texels()),
            16,
            vieww_gpu::RampAtlas::WIDTH,
            version,
            rects,
        )?;
        Ok(())
    }

    /// The rectangle half of [`Self::record_masks`], shared by every atlas
    /// that can say which of its texels changed: `rects` — rectangles over a
    /// texture `pitch` texels wide of `stride`-byte texels — packed into
    /// `slot`'s staging buffer, one copy region per rectangle, leaving the
    /// texels outside them alone. Returns the bytes sent; an empty `rects`
    /// only stamps `version`, because a copy of no regions is not a no-op a
    /// driver is required to accept.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_regions(
        &mut self,
        slot: Slot,
        texels: &[u8],
        stride: usize,
        pitch: u32,
        version: Option<u64>,
        rects: &[[u32; 4]],
    ) -> Result<u64, VulkanError> {
        if rects.is_empty() {
            self.texture_mut(slot).version = version;
            return Ok(0);
        }

        // Pack each rectangle's rows contiguously in the staging buffer, one
        // copy region per rectangle.
        let mut packed = Vec::new();
        let mut regions = Vec::with_capacity(rects.len());
        for &[x, y, w, h] in rects {
            regions.push(vk::BufferImageCopy {
                buffer_offset: packed.len() as u64,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: color_layers(),
                image_offset: vk::Offset3D {
                    x: i32::try_from(x).unwrap_or(i32::MAX),
                    y: i32::try_from(y).unwrap_or(i32::MAX),
                    z: 0,
                },
                image_extent: vk::Extent3D {
                    width: w,
                    height: h,
                    depth: 1,
                },
            });
            for row in y..y + h {
                let from = ((row * pitch + x) as usize) * stride;
                let bytes = w as usize * stride;
                packed.extend_from_slice(&texels[from..from + bytes]);
            }
        }
        let needed = packed.len() as u64;
        let flight = self.recording;
        if self.texture(slot).staging[flight].capacity < needed {
            let old = std::mem::replace(&mut self.texture_mut(slot).staging[flight], Buffer::EMPTY);
            unsafe { destroy_buffer(&self.device, old) };
            let staging = allocate_buffer(
                &self.device,
                vk::BufferUsageFlags::TRANSFER_SRC,
                self.host_visible_memory,
                needed,
            )?;
            self.texture_mut(slot).staging[flight] = staging;
            self.texture_mut(slot).staging_idle[flight] = 0;
        } else if needed.saturating_mul(STAGING_TRIM_FACTOR)
            <= self.texture(slot).staging[flight].capacity
        {
            // **The idle decay.** A staging buffer grows to the largest
            // upload it ever served and, before this, stayed there forever:
            // one 64 MB image-atlas full upload pinned 64 MB of host-visible
            // memory per flight slot for the life of the renderer, while the
            // frames after it sent a few kilobytes of dirty rects. Here, a
            // run of frames whose partial uploads need a fraction of the
            // buffer releases it — after that flight slot's fence has
            // settled, since the frame two ago may still be copying out of
            // it — and the next upload of any size reallocates at exactly
            // what it needs, as a cold renderer would have.
            let idle = self.texture_mut(slot).staging_idle[flight].saturating_add(1);
            self.texture_mut(slot).staging_idle[flight] = idle;
            if u32::from(idle) >= self.staging_trim_frames {
                self.settle_slot(flight)?;
                let old = std::mem::replace(&mut self.texture_mut(slot).staging[flight], Buffer::EMPTY);
                unsafe { destroy_buffer(&self.device, old) };
                // Reallocated at exactly what this frame's upload needs —
                // the size a cold renderer would have chosen — rather than
                // left EMPTY for the next call to grow: the write below
                // runs either way, and a cold renderer's first upload is
                // this same pair.
                let staging = allocate_buffer(
                    &self.device,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    self.host_visible_memory,
                    needed,
                )?;
                self.texture_mut(slot).staging[flight] = staging;
                self.texture_mut(slot).staging_idle[flight] = 0;
            }
        } else {
            self.texture_mut(slot).staging_idle[flight] = 0;
        }
        write_bytes(
            &self.device,
            self.texture(slot).staging[flight].memory,
            &packed,
        )?;

        let (image, staging) = {
            let t = self.texture(slot);
            (t.image, t.staging[flight].handle)
        };
        let cmd = self.cmd();
        unsafe {
            // From the layout the last upload left it in, so the texels
            // outside `regions` are kept.
            transition(
                &self.device,
                cmd,
                image,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            self.device.cmd_copy_buffer_to_image(
                cmd,
                staging,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
            );
            transition(
                &self.device,
                cmd,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }
        self.texture_mut(slot).version = version;
        Ok(needed)
    }

    /// Recreate `slot`'s texture at `width` x `height`, carrying the old
    /// texture's content across with **one GPU image-to-image copy** — the
    /// resize half of an atlas grow, whose texels all kept their positions.
    ///
    /// # Why the old texture is retired rather than destroyed here
    ///
    /// The copy is *recorded* into the command buffer the caller is building,
    /// not executed: destroying the source now would be a use-after-free the
    /// driver may or may not catch. So the old image is pushed onto
    /// [`retired_textures`](Self::retired_textures) and freed at the next
    /// [`settle`](Self::settle), by which point every frame that could have
    /// been executing the copy has finished. The staging arrays are the
    /// slot's, not the texture's — they move to the replacement so the
    /// partial uploads that follow keep their buffers.
    ///
    /// The new texture is left in `SHADER_READ_ONLY_OPTIMAL`, the layout
    /// [`record_regions`](Self::record_regions) expects to transition from.
    /// The scene descriptor sets are rebound to the new view, as any resize
    /// must.
    fn resize_with_carry(
        &mut self,
        slot: Slot,
        width: u32,
        height: u32,
    ) -> Result<(), VulkanError> {
        let format = self.texture(slot).format;
        let (old_w, old_h) = (self.texture(slot).width, self.texture(slot).height);
        // Frames still in flight may be sampling the old texture: it is
        // retired, not destroyed, and their scene sets keep pointing at it
        // until they are next recorded (`rebind_resized`). Nothing waits.
        let mut old = {
            let fresh = create_texture(&self.device, &self.mem_props, format, width, height)?;
            std::mem::replace(self.texture_mut(slot), fresh)
        };
        // The staging arrays belong to the slot: move them across so the
        // rectangle upload that follows this keeps its buffers.
        self.texture_mut(slot).staging =
            std::mem::replace(&mut old.staging, [Buffer::EMPTY; FLIGHT_SLOTS]);
        self.texture_mut(slot).staging_idle = old.staging_idle;
        let fresh_image = self.texture(slot).image;
        self.retired_textures.push(old);
        let old_image = self.retired_textures.last().map_or(vk::Image::null(), |t| t.image);
        debug_assert!(old_image != vk::Image::null());
        let cmd = self.cmd();
        unsafe {
            transition(
                &self.device,
                cmd,
                old_image,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            );
            // UNDEFINED as the fresh texture's old layout on purpose: every
            // texel the copy does not cover is written by the rectangle
            // upload after it, or is unwritten atlas space no draw reads.
            transition(
                &self.device,
                cmd,
                fresh_image,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            self.device.cmd_copy_image(
                cmd,
                old_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                fresh_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy {
                    src_subresource: color_layers(),
                    src_offset: vk::Offset3D::default(),
                    dst_subresource: color_layers(),
                    dst_offset: vk::Offset3D::default(),
                    extent: vk::Extent3D {
                        width: old_w,
                        height: old_h,
                        depth: 1,
                    },
                }],
            );
            transition(
                &self.device,
                cmd,
                fresh_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }
        // The scene sets sample the new view, as any resize must rebind.
        self.rebind_resized(slot)
    }

    /// How many bytes the most recent [`Self::upload_masks`] sent — the whole
    /// texture after a full upload, the changed rectangles otherwise, zero
    /// when nothing changed. For tests and measurement.
    #[must_use]
    pub const fn last_mask_upload_bytes(&self) -> u64 {
        self.mask_upload_bytes
    }

    /// How many bytes the most recent glyph-atlas upload sent — the whole
    /// texture after a full upload, the changed rectangles otherwise, zero
    /// when nothing changed. For tests and measurement.
    #[must_use]
    pub const fn last_glyph_upload_bytes(&self) -> u64 {
        self.glyph_upload_bytes
    }

    /// How many bytes the most recent image-atlas upload sent — the whole
    /// texture after a full upload, the changed rectangles otherwise, zero
    /// when nothing changed. For tests and measurement.
    #[must_use]
    pub const fn last_image_upload_bytes(&self) -> u64 {
        self.image_upload_bytes
    }

    /// How many bytes the most recent ramp-atlas upload sent — the whole
    /// texture after a full upload, the changed rectangles otherwise, zero
    /// when nothing changed. For tests and measurement.
    #[must_use]
    pub const fn last_ramp_upload_bytes(&self) -> u64 {
        self.ramp_upload_bytes
    }

    /// How many layer targets are allocated, levels and scratch counted
    /// together — the number the idle shrink frees down to what frames use.
    /// For tests and measurement.
    #[must_use]
    pub const fn target_count(&self) -> usize {
        self.levels.len() + self.scratch.len()
    }

    /// The staging capacity, in bytes, of one atlas slot's largest flight
    /// buffer — the number the idle decay releases back to the bytes the
    /// partial uploads actually send. For tests and measurement.
    #[must_use]
    pub fn staging_capacity(&self, slot: StagingSlot) -> u64 {
        let mut largest = 0;
        for flight in &self.texture(slot.as_slot()).staging {
            largest = largest.max(flight.capacity);
        }
        largest
    }

    /// Narrow the idle thresholds that free the renderer's surplus memory:
    /// how many frames of unused layer targets before they are freed, and of
    /// eight-times-oversized staging before it is released. The defaults
    /// (`TARGET_TRIM_FRAMES`, `STAGING_TRIM_FRAMES`) are for a
    /// production process; a test that wants to *see* a trim renders far
    /// fewer frames than ten seconds of them.
    pub fn set_memory_trim_frames(&mut self, frames: u32) {
        self.target_trim_frames = frames.max(1);
        self.staging_trim_frames = frames.max(1);
    }

    /// [`render`](Self::render), with `planner`'s atlases uploaded first — the
    /// pairing that keeps a plan's texel offsets and the textures they
    /// address in step.
    ///
    /// # Errors
    ///
    /// As [`render`](Self::render), plus any upload failing.
    pub fn render_planned(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Result<Vec<u8>, VulkanError> {
        self.settle()?;
        let started = Instant::now();
        self.upload_planner(planner)?;
        let atlases = started.elapsed();
        let pixels = self.render(plan, width, height, clear)?;
        self.timings.upload += atlases;
        Ok(pixels)
    }

    /// [`render_planned`](Self::render_planned), redrawing only `damage` of
    /// the previous frame this renderer drew — see the module docs for what
    /// is kept and when it cannot be. `None` redraws everything, exactly as
    /// `render_planned` does. Returns the frame and how much was redrawn.
    ///
    /// `damage` is in this frame's device pixels; it must cover everything
    /// that differs from the previous frame, as the CPU rasterizer's retained
    /// path also requires.
    ///
    /// # Errors
    ///
    /// As [`render_planned`](Self::render_planned).
    pub fn render_planned_damaged(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
        damage: Option<&[PixelRect]>,
    ) -> Result<(Vec<u8>, Redrawn), VulkanError> {
        self.settle()?;
        if !plan.is_complete() {
            return Err(VulkanError::Vulkan("incomplete plan".into()));
        }
        let started = Instant::now();
        self.upload_planner(planner)?;
        let atlases = started.elapsed();
        let result = self.render_with(plan, width, height, clear, damage)?;
        self.timings.upload += atlases;
        Ok(result)
    }

    /// Where the last rendered frame's time went. Reset at the start of every
    /// frame; see [`GpuFrameTimings`] for what each field measures.
    #[must_use]
    pub const fn last_timings(&self) -> GpuFrameTimings {
        self.timings
    }

    /// Where the last measured frame's GPU time went, by kind of work — see
    /// [`GpuPassTimes`]. `None` when the queue cannot record timestamps, or
    /// no frame has been read yet.
    #[must_use]
    pub const fn last_pass_times(&self) -> Option<GpuPassTimes> {
        self.pass_times
    }

    /// Execute `plan` at `width` x `height` over `clear` and read the frame
    /// back as straight-alpha RGBA8.
    ///
    /// # Errors
    ///
    /// [`VulkanError::Vulkan`] carrying `"incomplete plan"` when the plan has
    /// gaps, and any Vulkan call failing.
    pub fn render(
        &mut self,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Result<Vec<u8>, VulkanError> {
        self.settle()?;
        if !plan.is_complete() {
            let kinds: Vec<String> = plan
                .unsupported
                .iter()
                .map(|(kind, count)| format!("{}x{}", count, kind.name()))
                .collect();
            return Err(VulkanError::Vulkan(format!(
                "incomplete plan: this backend cannot draw {}. Render this frame on the CPU \
                 rasterizer, or call `render_incomplete` if a partial result is genuinely wanted",
                kinds.join(", ")
            )));
        }
        self.render_incomplete(plan, width, height, clear)
    }

    /// [`render`](Self::render) without the completeness check. **Not** for a
    /// screen — see the module doc.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    pub fn render_incomplete(
        &mut self,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Result<Vec<u8>, VulkanError> {
        self.settle()?;
        self.render_with(plan, width, height, clear, None)
            .map(|(pixels, _)| pixels)
    }

    fn render_with(
        &mut self,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
        damage: Option<&[PixelRect]>,
    ) -> Result<(Vec<u8>, Redrawn), VulkanError> {
        let width = width.max(1);
        let height = height.max(1);
        self.timings = GpuFrameTimings::default();
        let started = Instant::now();
        self.ensure_targets(width, height, plan.target_depth + 1)?;
        self.begin_commands(self.next_slot())?;
        #[allow(clippy::cast_precision_loss)]
        let viewport = [width as f32, height as f32, 0.0, 0.0];
        write_bytes(
            &self.device,
            self.slots[self.recording].uniform.memory,
            as_bytes(&viewport),
        )?;
        self.upload_geometry(plan)?;
        self.timings.upload = started.elapsed();
        let redrawn = self.execute(plan, width, height, clear, Out::Readback, damage)?;
        let started = Instant::now();
        let pixels = self.read_back(width, height)?;
        self.timings.readback = started.elapsed();
        Ok((pixels, redrawn))
    }

    /// Carry these blocks of the kept frame by whole pixels at the start of
    /// the next frame that redraws only its damage — the GPU half of a
    /// scroll's copies (`vieww_paint::Damage::copies`). Before this, the GPU
    /// path could not carry pixels and had to redraw every row a scroll
    /// moved. Ignored, and dropped, by a frame that redraws in full.
    pub fn set_pending_copies(&mut self, copies: Vec<(PixelRect, i32, i32)>) {
        self.pending_copies = copies;
    }

    /// Execute a complete `plan` straight into an acquired swapchain image,
    /// leaving it ready to present — no readback, no host copy, no upload.
    /// `planner`'s atlases are uploaded first, as in
    /// [`render_planned`](Self::render_planned).
    ///
    /// Only for an **opaque** frame (`clear.a == 255`): the scene targets are
    /// premultiplied, and a translucent window's pixels would need
    /// unpremultiplying, which a blit cannot do. The caller checks
    /// `VulkanDevice::can_blit_scene_frames` once per swapchain.
    ///
    /// `scale` renders the plan into targets `scale` times the target image's
    /// extent and lets the final blit enlarge it (`LINEAR`); the caller checks
    /// `VulkanDevice::can_scale_scene_frames` before passing anything below
    /// 1.0. Full resolution — the default — is byte-for-byte the same command
    /// stream a 1:1 blit ever was.
    ///
    /// `damage`, in the *render* extent's pixels, redraws only those
    /// rectangles of the previous frame this renderer drew (see the module
    /// docs); the whole frame is still blitted, because each swapchain image
    /// is a different one. `None` redraws everything.
    ///
    /// # Errors
    ///
    /// An incomplete plan, a translucent clear, or any Vulkan call failing.
    /// On error `target` is still the caller's to present or abandon.
    #[cfg(feature = "vulkan-swapchain")]
    pub fn render_planned_to(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        clear: Color,
        target: &AcquiredImage,
        scale: f32,
        damage: Option<&[PixelRect]>,
    ) -> Result<Redrawn, VulkanError> {
        // The ring, not the full stop: the oldest slot is waited for and the
        // frame before stays in flight — the overlap the direct route already
        // has, extended to the layered and scaled frames that take this route.
        // Safe for the same reasons it is there: the acquire semaphore is
        // keyed to this slot and its previous wait belonged to the frame two
        // back, which just finished; everything the frame records over is its
        // slot's own; a resize and the probe path's image reads settle in full
        // where they stand. A frame the CPU reads back (`render_planned`)
        // still settles everything.
        self.settle_for_next_frame()?;
        if !plan.is_complete() {
            return Err(VulkanError::Vulkan("incomplete plan".into()));
        }
        if clear.a != 255 {
            return Err(VulkanError::Vulkan(
                "render_planned_to needs an opaque clear; use render_planned".into(),
            ));
        }
        let (width, height) = target.extent();
        if width == 0 || height == 0 {
            return Err(VulkanError::Vulkan("zero-sized swapchain image".into()));
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a swapchain extent is a small number of pixels"
        )]
        let (render_width, render_height) = if scale < 1.0 {
            (
                ((width as f32 * scale).round() as u32).max(1),
                ((height as f32 * scale).round() as u32).max(1),
            )
        } else {
            (width, height)
        };
        self.timings = GpuFrameTimings::default();
        let started = Instant::now();
        // Targets first: a resize re-begins the command buffer of its own
        // (`initialise_targets`), and the uploads below must not be recorded
        // into a buffer that is about to be reset.
        self.ensure_targets(render_width, render_height, plan.target_depth + 1)?;
        // One submission for the whole frame: the changed atlases record
        // first, the frame's passes around them.
        let flight = self.next_slot();
        self.begin_commands(flight)?;
        self.record_planner_uploads(planner)?;
        #[expect(clippy::cast_precision_loss, reason = "frame extents")]
        let viewport = [render_width as f32, render_height as f32, 0.0, 0.0];
        write_bytes(
            &self.device,
            self.slots[self.recording].uniform.memory,
            as_bytes(&viewport),
        )?;
        self.upload_geometry(plan)?;
        self.timings.upload = started.elapsed();
        let blit = BlitTarget {
            image: target.image,
            ready: target.ready,
            done: target.done,
            src_extent: [render_width, render_height],
            filter: if render_width == width && render_height == height {
                vk::Filter::NEAREST
            } else {
                vk::Filter::LINEAR
            },
            dst_extent: [width, height],
            index: target.index,
            generation: target.generation,
            partial: target.partial,
            bgra: target.bgra,
        };
        self.execute(
            plan,
            render_width,
            render_height,
            clear,
            Out::Blit(blit),
            damage,
        )
    }

    /// Execute a complete **depth-0** `plan` straight into `target`'s
    /// swapchain image — no `L[0]`, no carry, no blit.
    ///
    /// The route exists for the frame a low-end GPU spends most of its
    /// bandwidth on: an opaque, layer-less screen. There the `RGBA16F`
    /// route's costs are all overhead — a full-frame render pass over an
    /// 8-bytes-per-pixel target that loads and stores the damage's bounding
    /// box, carry copies through a scratch image, and an f16→8-bit blit of
    /// the whole screen every frame — while the direct route is one render
    /// pass into the image the compositor reads, blending in its own 8-bit
    /// format. On an Adreno 612 at 1080x2340 that is roughly a quarter of the
    /// memory traffic for the same drawing.
    ///
    /// The frame always redraws in full: a swapchain image is one of several,
    /// so the kept-`L[0]` damage model does not describe it — the `Direct`
    /// arm `execute` carries no damage, and it is private, which is why this
    /// paragraph spells it out instead of linking it. `L[0]`'s retained state
    /// is invalidated, so a later
    /// frame that falls back to `render_planned_to` (the blit route, behind
    /// the `vulkan-swapchain` feature and so spelled rather than linked)
    /// — a plan with layers, a translucent window, a resolution scale —
    /// redraws in full too.
    ///
    /// Only for an **opaque** frame (`clear.a == 255`) at the image's own
    /// extent, as the blit route also requires; the caller checks
    /// [`AcquiredImage::color_attachment`] before choosing this route.
    ///
    /// # Errors
    ///
    /// An incomplete plan, a plan with layers, a translucent clear, or any
    /// Vulkan call failing. On error `target` is still the caller's to
    /// present or abandon.
    #[cfg(feature = "vulkan-swapchain")]
    pub fn render_planned_direct(
        &mut self,
        planner: &Planner,
        plan: &ScenePlan,
        clear: Color,
        target: &AcquiredImage,
    ) -> Result<Redrawn, VulkanError> {
        if !plan.is_complete() {
            return Err(VulkanError::Vulkan("incomplete plan".into()));
        }
        if plan.target_depth != 0 {
            return Err(VulkanError::Vulkan(
                "render_planned_direct needs a depth-0 plan; use render_planned_to".into(),
            ));
        }
        if clear.a != 255 {
            return Err(VulkanError::Vulkan(
                "render_planned_direct needs an opaque clear; use render_planned".into(),
            ));
        }
        let (width, height) = target.extent();
        if width == 0 || height == 0 {
            return Err(VulkanError::Vulkan("zero-sized swapchain image".into()));
        }
        self.timings = GpuFrameTimings::default();
        // As `render_planned_to`: the oldest slot is waited for, the frame
        // before stays in flight.
        self.settle_for_next_frame()?;
        let started = Instant::now();
        self.ensure_direct(target.bgra)?;
        let framebuffer = self.direct_framebuffer(target, width, height)?;
        // One submission for the whole frame: the changed atlases record
        // first, the frame's passes around them.
        let flight = self.next_slot();
        self.begin_commands(flight)?;
        self.record_planner_uploads(planner)?;
        #[expect(clippy::cast_precision_loss, reason = "frame extents")]
        let viewport = [width as f32, height as f32, 0.0, 0.0];
        write_bytes(
            &self.device,
            self.slots[self.recording].uniform.memory,
            as_bytes(&viewport),
        )?;
        self.upload_geometry(plan)?;
        self.timings.upload = started.elapsed();
        let frame = DirectFrame {
            framebuffer,
            ready: target.ready,
            done: target.done,
        };
        self.execute(plan, width, height, clear, Out::Direct(frame), None)
    }

    /// Build the direct route for `bgra`'s swapchain format, if it is not
    /// built already. One set of render passes and pipelines per format;
    /// `settle` has already run, so anything replaced is idle.
    #[cfg_attr(
        not(feature = "vulkan-swapchain"),
        expect(dead_code, reason = "only the swapchain submits take the direct route")
    )]
    fn ensure_direct(&mut self, bgra: bool) -> Result<(), VulkanError> {
        let format = if bgra {
            vk::Format::B8G8R8A8_UNORM
        } else {
            vk::Format::R8G8B8A8_UNORM
        };
        if self
            .direct
            .as_ref()
            .is_some_and(|route| route.format == format)
        {
            return Ok(());
        }
        // A frame still in flight may be drawing through the route being
        // replaced; its framebuffers are not the CPU's to destroy yet.
        self.settle()?;
        let rp_clear = create_render_pass(
            &self.device,
            vk::AttachmentLoadOp::CLEAR,
            format,
            vk::ImageLayout::PRESENT_SRC_KHR,
        )?;
        let rp_load = create_render_pass(
            &self.device,
            vk::AttachmentLoadOp::LOAD,
            format,
            vk::ImageLayout::PRESENT_SRC_KHR,
        )?;
        let (binding, attributes) = scene_vertex_layout();
        let (shaders, scene_layout, post_layout, cache) = (
            &self.shaders,
            self.scene_layout,
            self.post_layout,
            self.pipeline_cache,
        );
        let scene_pipeline = create_pipeline(
            (&self.device, cache),
            rp_clear,
            scene_layout,
            (shaders[0], shaders[1]),
            &[binding],
            &attributes,
            true,
        )?;
        let post_replace = create_pipeline(
            (&self.device, cache),
            rp_clear,
            post_layout,
            (shaders[2], shaders[3]),
            &[],
            &[],
            false,
        )?;
        let post_over = create_pipeline(
            (&self.device, cache),
            rp_clear,
            post_layout,
            (shaders[2], shaders[3]),
            &[],
            &[],
            true,
        )?;
        if let Some(old) = self.direct.take() {
            for (_, _, view, framebuffer) in old.frames {
                unsafe {
                    self.device.destroy_image_view(view, None);
                    self.device.destroy_framebuffer(framebuffer, None);
                }
            }
            unsafe {
                self.device.destroy_render_pass(old.rp_load, None);
                self.device.destroy_render_pass(old.rp_clear, None);
                for pipeline in [old.scene_pipeline, old.post_replace, old.post_over] {
                    self.device.destroy_pipeline(pipeline, None);
                }
            }
        }
        self.direct = Some(DirectRoute {
            format,
            rp_load,
            rp_clear,
            scene_pipeline,
            post_replace,
            post_over,
            frames: Vec::new(),
        });
        Ok(())
    }

    /// The framebuffer over `target`'s image, creating it (and dropping
    /// another generation's stale ones) on first sight.
    #[cfg(feature = "vulkan-swapchain")]
    fn direct_framebuffer(
        &mut self,
        target: &AcquiredImage,
        width: u32,
        height: u32,
    ) -> Result<vk::Framebuffer, VulkanError> {
        let route = self
            .direct
            .as_mut()
            .expect("ensure_direct ran before direct_framebuffer");
        if let Some(&(_, _, _, framebuffer)) =
            route.frames.iter().find(|&&(index, generation, _, _)| {
                index == target.index && generation == target.generation
            })
        {
            return Ok(framebuffer);
        }
        let view = unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(target.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(route.format)
                    .subresource_range(color_range()),
                None,
            )?
        };
        let rp = route.rp_clear;
        let framebuffer = unsafe {
            self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(rp)
                    .attachments(&[view])
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )?
        };
        // Another generation's views and framebuffers are about to become
        // unreachable. The other images of this generation stay: the next
        // frame wants them.
        let stale: Vec<(u32, u64, vk::ImageView, vk::Framebuffer)> = route
            .frames
            .iter()
            .filter(|&&(_, generation, _, _)| generation != target.generation)
            .copied()
            .collect();
        route
            .frames
            .retain(|&(_, generation, _, _)| generation == target.generation);
        route
            .frames
            .push((target.index, target.generation, view, framebuffer));
        if !stale.is_empty() {
            // A frame still in flight may be drawing through the stale
            // generation's framebuffers — the generation moved under a
            // rebuild or an abandoned acquire, not necessarily under the
            // full settle this path once leaned on. Destroying them waits
            // for it first; a generation change is not a steady-state
            // event.
            self.settle()?;
            for (_, _, view, framebuffer) in stale {
                unsafe {
                    self.device.destroy_image_view(view, None);
                    self.device.destroy_framebuffer(framebuffer, None);
                }
            }
        }
        Ok(framebuffer)
    }

    // ── targets ────────────────────────────────────────────────────────────

    fn ensure_targets(&mut self, width: u32, height: u32, depth: usize) -> Result<(), VulkanError> {
        if self.size != (width, height) {
            unsafe { self.device.device_wait_idle() }?;
            self.settle()?;
            for target in self.levels.drain(..).chain(self.scratch.drain(..)) {
                unsafe { destroy_target(&self.device, target) };
            }
            let old = std::mem::replace(&mut self.readback, Buffer::EMPTY);
            unsafe { destroy_buffer(&self.device, old) };
            self.readback = allocate_buffer(
                &self.device,
                vk::BufferUsageFlags::TRANSFER_DST,
                self.host_visible_memory,
                u64::from(width) * u64::from(height) * 8,
            )?;
            self.reset_post_sets()?;
            self.size = (width, height);
            // New targets hold nothing of any earlier frame.
            self.retained = None;
        }
        let mut fresh = Vec::new();
        while self.levels.len() < depth {
            let level = self.levels.len();
            self.levels.push(create_target(
                &self.device,
                &self.mem_props,
                self.rp_load,
                width,
                height,
            )?);
            self.scratch.push(create_target(
                &self.device,
                &self.mem_props,
                self.rp_load,
                width,
                height,
            )?);
            fresh.push(TargetId::Level(level));
            fresh.push(TargetId::Scratch(level));
        }
        if !fresh.is_empty() {
            // New targets are cleared by a submission of their own on slot
            // 0's command buffer; a frame still in flight there owns it,
            // and the frames before this one may still be sampling the
            // targets the resize did not replace. Depth growth happens on
            // the first frames and the first layer of a scene; a full
            // settle here costs nothing that matters.
            self.settle()?;
            self.initialise_targets(&fresh)?;
            // Growth is the opposite of idle.
            self.idle_target_frames = 0;
            return Ok(());
        }
        // **The idle shrink.** `levels`/`scratch` only ever grew before this:
        // one deep-layered frame pinned its targets for the life of the
        // renderer (see `TARGET_TRIM_FRAMES`). A run of frames that used
        // fewer frees the surplus — after a settle, since earlier frames may
        // still be sampling the targets being destroyed, and with the post
        // descriptor sets invalidated because they are keyed by target ids.
        // `L[0]` is always kept, so the retained damage frame survives.
        if depth >= self.levels.len() {
            self.idle_target_frames = 0;
        } else {
            self.idle_target_frames = self.idle_target_frames.saturating_add(1);
            if self.idle_target_frames >= self.target_trim_frames && self.levels.len() > 1 {
                let keep = depth.max(1);
                self.settle()?;
                for target in self.levels.drain(keep..) {
                    unsafe { destroy_target(&self.device, target) };
                }
                for target in self.scratch.drain(keep..) {
                    unsafe { destroy_target(&self.device, target) };
                }
                self.reset_post_sets()?;
                self.idle_target_frames = 0;
            }
        }
        Ok(())
    }

    /// Clear new targets to transparent and leave them in
    /// `SHADER_READ_ONLY_OPTIMAL`, the layout every target rests in.
    fn initialise_targets(&mut self, ids: &[TargetId]) -> Result<(), VulkanError> {
        self.begin_commands(0)?;
        let cmd = self.cmd();
        for id in ids {
            let image = self.target(*id).image;
            unsafe {
                transition(
                    &self.device,
                    cmd,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                );
                self.device.cmd_clear_color_image(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue { float32: [0.0; 4] },
                    &[color_range()],
                );
                transition(
                    &self.device,
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
        }
        self.submit_and_wait(None).map(|_| ())
    }

    /// Move `(src, dst)` blocks of `L[0]` through `S[0]` — a copy within one
    /// image may not overlap itself, and a scroll's always does. Both
    /// targets rest in `SHADER_READ_ONLY_OPTIMAL` and return to it.
    fn carry(&self, copies: &[(PixelRect, PixelRect)]) {
        let cmd = self.cmd();
        let (frame, scratch) = (self.levels[0].image, self.scratch[0].image);
        let region = |src: PixelRect, dst: PixelRect| vk::ImageCopy {
            src_subresource: color_layers(),
            src_offset: vk::Offset3D {
                x: src.x0,
                y: src.y0,
                z: 0,
            },
            dst_subresource: color_layers(),
            dst_offset: vk::Offset3D {
                x: dst.x0,
                y: dst.y0,
                z: 0,
            },
            #[expect(clippy::cast_sign_loss, reason = "non-empty after clamping")]
            extent: vk::Extent3D {
                width: (src.x1 - src.x0) as u32,
                height: (src.y1 - src.y0) as u32,
                depth: 1,
            },
        };
        let to_scratch: Vec<vk::ImageCopy> = copies.iter().map(|(s, _)| region(*s, *s)).collect();
        let back: Vec<vk::ImageCopy> = copies.iter().map(|(s, d)| region(*s, *d)).collect();
        let (sro, ts, td) = (
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        unsafe {
            transition(&self.device, cmd, frame, sro, ts);
            transition(&self.device, cmd, scratch, sro, td);
            self.device
                .cmd_copy_image(cmd, frame, ts, scratch, td, &to_scratch);
            transition(&self.device, cmd, frame, ts, td);
            transition(&self.device, cmd, scratch, td, ts);
            self.device
                .cmd_copy_image(cmd, scratch, ts, frame, td, &back);
            transition(&self.device, cmd, frame, td, sro);
            transition(&self.device, cmd, scratch, ts, sro);
        }
    }

    fn target(&self, id: TargetId) -> &Target {
        match id {
            TargetId::Level(i) => &self.levels[i],
            TargetId::Scratch(i) => &self.scratch[i],
            TargetId::Dummy => &self.dummy,
        }
    }

    /// Point the scene sets at `slot`'s (new) texture view.
    ///
    /// The recording slot's set is written now; the other flight slots'
    /// sets are marked in [`stale_scene_bindings`](Self::stale_scene_bindings)
    /// and written when each is next recorded, so no frame in flight has its
    /// set changed under it. The mask texture is the exception: the post
    /// sets bind it too and share one pool, which cannot be reset while a
    /// frame uses it, so a mask resize settles every slot first and writes
    /// everything at once, as every resize once did.
    fn rebind_resized(&mut self, slot: Slot) -> Result<(), VulkanError> {
        let all: Vec<usize> = if slot == Slot::Masks {
            for index in 0..FLIGHT_SLOTS {
                self.settle_slot(index)?;
            }
            (0..FLIGHT_SLOTS).collect()
        } else {
            for index in 0..FLIGHT_SLOTS {
                if index != self.recording {
                    self.stale_scene_bindings[index] |= 1 << slot.binding();
                }
            }
            vec![self.recording]
        };
        for index in all {
            self.stale_scene_bindings[index] &= !(1 << slot.binding());
            self.write_scene_binding(index, slot);
        }
        if slot == Slot::Masks {
            self.reset_post_sets()?;
        }
        Ok(())
    }

    /// Write one texture binding of one flight slot's scene set.
    fn write_scene_binding(&self, index: usize, slot: Slot) {
        let info = [vk::DescriptorImageInfo {
            sampler: vk::Sampler::null(),
            image_view: self.texture(slot).view,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        }];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.scene_sets[index])
            .dst_binding(slot.binding())
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&info);
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
    }

    fn reset_post_sets(&mut self) -> Result<(), VulkanError> {
        if !self.post_sets.is_empty() {
            unsafe {
                self.device
                    .reset_descriptor_pool(self.post_pool, vk::DescriptorPoolResetFlags::empty())
            }?;
            self.post_sets.clear();
        }
        Ok(())
    }

    fn post_set(&mut self, src: TargetId, aux: TargetId) -> Result<vk::DescriptorSet, VulkanError> {
        if let Some(set) = self.post_sets.get(&(src, aux)) {
            return Ok(*set);
        }
        let set = unsafe {
            self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.post_pool)
                    .set_layouts(&[self.post_set_layout]),
            )
        }?[0];
        let image_info = |view: vk::ImageView| {
            [vk::DescriptorImageInfo {
                sampler: vk::Sampler::null(),
                image_view: view,
                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            }]
        };
        let src_info = image_info(self.target(src).view);
        let aux_info = image_info(self.target(aux).view);
        let mask_info = image_info(self.masks.view);
        let write = |binding: u32, info| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(info)
        };
        let writes = [
            write(0, &src_info),
            write(1, &aux_info),
            write(2, &mask_info),
        ];
        unsafe { self.device.update_descriptor_sets(&writes, &[]) };
        self.post_sets.insert((src, aux), set);
        Ok(set)
    }

    // ── textures ───────────────────────────────────────────────────────────

    fn texture(&self, slot: Slot) -> &Texture {
        match slot {
            Slot::Glyphs => &self.glyphs,
            Slot::Images => &self.images,
            Slot::Masks => &self.masks,
            Slot::Ramps => &self.ramps,
        }
    }

    fn texture_mut(&mut self, slot: Slot) -> &mut Texture {
        match slot {
            Slot::Glyphs => &mut self.glyphs,
            Slot::Images => &mut self.images,
            Slot::Masks => &mut self.masks,
            Slot::Ramps => &mut self.ramps,
        }
    }

    /// Replace a texture's contents, recreating it when the size changes. A
    /// no-op when `version` is what is already resident (unless `force`).
    fn upload(
        &mut self,
        slot: Slot,
        bytes: &[u8],
        width: u32,
        height: u32,
        version: Option<u64>,
        force: bool,
    ) -> Result<(), VulkanError> {
        self.begin_commands(0)?;
        self.record_upload(slot, bytes, width, height, version, force)?;
        self.submit_and_wait(None).map(|_| ())
    }

    /// [`Self::upload`], recorded into the open command buffer instead of a
    /// submission of its own — the folded form every frame render uses.
    ///
    /// # Errors
    ///
    /// Any Vulkan call failing.
    fn record_upload(
        &mut self,
        slot: Slot,
        bytes: &[u8],
        width: u32,
        height: u32,
        version: Option<u64>,
        force: bool,
    ) -> Result<(), VulkanError> {
        let (format, resized, current) = {
            let t = self.texture(slot);
            (
                t.format,
                t.width != width || t.height != height,
                t.version == version,
            )
        };
        if !force && !resized && current && version.is_some() {
            return Ok(());
        }
        debug_assert_eq!(
            bytes.len(),
            width as usize * height as usize * bytes_per_texel(format)
        );
        if resized {
            // As `resize_with_carry`: the old texture may still be sampled
            // by a frame in flight, so it is retired (freed at the next
            // `settle`) rather than destroyed, and nothing waits — the mask
            // texture's rebind settles for itself (`rebind_resized`). The
            // staging arrays are the slot's and move across.
            let fresh = create_texture(&self.device, &self.mem_props, format, width, height)?;
            let mut old = std::mem::replace(self.texture_mut(slot), fresh);
            self.texture_mut(slot).staging =
                std::mem::replace(&mut old.staging, [Buffer::EMPTY; FLIGHT_SLOTS]);
            self.texture_mut(slot).staging_idle = old.staging_idle;
            self.retired_textures.push(old);
        }

        let needed = bytes.len() as u64;
        let flight = self.recording;
        if self.texture(slot).staging[flight].capacity < needed {
            let old = std::mem::replace(&mut self.texture_mut(slot).staging[flight], Buffer::EMPTY);
            unsafe { destroy_buffer(&self.device, old) };
            let staging = allocate_buffer(
                &self.device,
                vk::BufferUsageFlags::TRANSFER_SRC,
                self.host_visible_memory,
                needed,
            )?;
            self.texture_mut(slot).staging[flight] = staging;
        }
        // A full upload is the largest thing this buffer serves — the run of
        // small partial uploads that would have decayed it starts over.
        self.texture_mut(slot).staging_idle[flight] = 0;
        write_bytes(
            &self.device,
            self.texture(slot).staging[flight].memory,
            bytes,
        )?;

        let (image, staging) = {
            let t = self.texture(slot);
            (t.image, t.staging[flight].handle)
        };
        let cmd = self.cmd();
        unsafe {
            // UNDEFINED as the old layout on purpose: the whole image is being
            // replaced, so the driver may discard what was there.
            transition(
                &self.device,
                cmd,
                image,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            self.device.cmd_copy_buffer_to_image(
                cmd,
                staging,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: color_layers(),
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    },
                }],
            );
            transition(
                &self.device,
                cmd,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }

        if resized {
            self.rebind_resized(slot)?;
        }
        self.texture_mut(slot).version = version;
        Ok(())
    }

    fn upload_geometry(&mut self, plan: &ScenePlan) -> Result<(), VulkanError> {
        let vertex_bytes = as_bytes(&plan.vertices);
        let index_bytes = as_bytes(&plan.indices);
        let slot = self.recording;
        let old_vertices = std::mem::replace(&mut self.slots[slot].vertices, Buffer::EMPTY);
        let old_indices = std::mem::replace(&mut self.slots[slot].indices, Buffer::EMPTY);
        self.slots[slot].vertices = grow(
            &self.device,
            old_vertices,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            self.host_visible_memory,
            vertex_bytes.len() as u64,
        )?;
        self.slots[slot].indices = grow(
            &self.device,
            old_indices,
            vk::BufferUsageFlags::INDEX_BUFFER,
            self.host_visible_memory,
            index_bytes.len() as u64,
        )?;
        write_bytes(&self.device, self.slots[slot].vertices.memory, vertex_bytes)?;
        write_bytes(&self.device, self.slots[slot].indices.memory, index_bytes)?;
        Ok(())
    }

    // ── execution ──────────────────────────────────────────────────────────

    /// Start this frame's timestamps: reset the recording slot's query pool
    /// (outside any render pass, which is where `execute` calls this) and
    /// mark the first kind.
    fn begin_marks(&mut self) {
        let cmd = self.cmd();
        if let Some(queries) = self.slots[self.recording].queries.as_mut() {
            queries.kinds.clear();
            queries.overflowed = false;
            // SAFETY: recording into this slot's command buffer; the pool is
            // this slot's own, so a frame still in flight on the other slot
            // is never reset under it.
            unsafe {
                self.device
                    .cmd_reset_query_pool(cmd, queries.pool, 0, QUERY_SLOTS);
            }
        }
        self.mark(PassKind::Draw);
    }

    /// The frame's commands from here on are `kind`'s: write a timestamp
    /// when the kind changes. One slot is kept back for [`end_marks`].
    ///
    /// [`end_marks`]: Self::end_marks
    fn mark(&mut self, kind: PassKind) {
        let cmd = self.cmd();
        let Some(queries) = self.slots[self.recording].queries.as_mut() else {
            return;
        };
        if queries.kinds.last() == Some(&kind) {
            return;
        }
        #[expect(clippy::cast_possible_truncation, reason = "at most QUERY_SLOTS")]
        let slot = queries.kinds.len() as u32;
        if slot + 1 >= QUERY_SLOTS {
            queries.overflowed = true;
            return;
        }
        // SAFETY: recording into this slot's command buffer; `slot` was
        // reset by this frame's `begin_marks` and is written once this frame.
        unsafe {
            self.device.cmd_write_timestamp(
                cmd,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                queries.pool,
                slot,
            );
        }
        queries.kinds.push(kind);
    }

    /// The frame's last timestamp, closing the last interval.
    fn end_marks(&mut self) {
        let cmd = self.cmd();
        if let Some(queries) = self.slots[self.recording].queries.as_mut() {
            #[expect(clippy::cast_possible_truncation, reason = "at most QUERY_SLOTS")]
            let slot = queries.kinds.len() as u32;
            // SAFETY: as in `mark`; `mark` always leaves this slot free.
            unsafe {
                self.device.cmd_write_timestamp(
                    cmd,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    queries.pool,
                    slot,
                );
            }
            queries.pending = true;
        }
    }

    /// Read the timestamps of the frame submitted on `index`'s slot, once
    /// its fence has signalled, into
    /// [`last_pass_times`](Self::last_pass_times). Never waits: results
    /// that are not ready (they always are after the fence) leave the
    /// previous figures in place.
    ///
    /// With frames in flight the figures are a frame behind the app's own
    /// progress — the pass times of the frame settled, read when the frame
    /// two after it records. The per-field percentiles the stages line
    /// reports are unaffected; only per-frame attribution moves, which the
    /// stages line already says it cannot promise.
    fn collect_marks(&mut self, index: usize) {
        let Some((period, mask)) = self.timestamps else {
            return;
        };
        let Some(queries) = self.slots[index].queries.as_mut() else {
            return;
        };
        if !std::mem::take(&mut queries.pending) || queries.kinds.is_empty() {
            return;
        }
        let mut ticks = vec![0u64; queries.kinds.len() + 1];
        // SAFETY: every slot read was written by the frame whose fence has
        // signalled; no `WAIT` flag, so this cannot block.
        let read = unsafe {
            self.device.get_query_pool_results(
                queries.pool,
                0,
                &mut ticks,
                vk::QueryResultFlags::TYPE_64,
            )
        };
        if read.is_err() {
            return;
        }
        let mut times = GpuPassTimes::default();
        for (index, kind) in queries.kinds.iter().enumerate() {
            let delta = ticks[index + 1].wrapping_sub(ticks[index]) & mask;
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a tick count times nanoseconds per tick, rounded"
            )]
            let nanos = (delta as f64 * f64::from(period)).round() as u64;
            let spent = Duration::from_nanos(nanos);
            match kind {
                PassKind::Draw => times.draw += spent,
                PassKind::Layers => times.layers += spent,
                PassKind::Filters => times.filters += spent,
                PassKind::Shadows => times.shadows += spent,
                PassKind::Blit => times.blit += spent,
            }
        }
        self.pass_times = Some(times);
    }

    fn begin_commands(&mut self, slot: usize) -> Result<(), VulkanError> {
        debug_assert!(
            !self.slots[slot].in_flight,
            "the command buffer of a frame still in flight was re-recorded; settle it first"
        );
        self.recording = slot;
        // Bindings a resize left for this slot (`rebind_resized`): it is
        // settled now, so its set can be written.
        let stale = std::mem::take(&mut self.stale_scene_bindings[slot]);
        if stale != 0 {
            for texture in [Slot::Glyphs, Slot::Images, Slot::Masks, Slot::Ramps] {
                if stale & (1 << texture.binding()) != 0 {
                    self.write_scene_binding(slot, texture);
                }
            }
        }
        let cmd = self.slots[slot].command_buffer;
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            self.device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        Ok(())
    }

    /// Interpret the shared traversal of a frame — see [`crate::scene_exec`]
    /// — into Vulkan commands, then submit and (unless a swapchain image is
    /// the destination) read the frame back.
    ///
    /// Everything that decides *what* a frame does lives in
    /// [`scene_exec::frame_passes`](crate::scene_exec::frame_passes): the
    /// damage a retained frame may keep, the layers a damaged frame skips,
    /// the scissored regions every draw and post pass is held to, the pass
    /// order and the push-constant payloads. This half only translates:
    /// render passes begin and end as the ops say, geometry draws through
    /// the scene pipeline, post passes through the two post pipelines, and
    /// the finished frame goes to the readback buffer or the swapchain
    /// image as `out` says.
    fn execute(
        &mut self,
        plan: &ScenePlan,
        width: u32,
        height: u32,
        clear: Color,
        out: Out,
        damage: Option<&[PixelRect]>,
    ) -> Result<Redrawn, VulkanError> {
        let recording = Instant::now();
        // The direct route always redraws in full: a swapchain image is one of
        // several, so the kept-`L[0]` damage model does not describe it, and
        // the bandwidth the route exists to save would be spent again on
        // per-image catch-up instead.
        let direct = matches!(out, Out::Direct(_));
        let exec = frame_passes(
            plan,
            width,
            height,
            clear,
            if direct { None } else { damage },
            if direct { None } else { self.retained },
        );
        // Until this frame finishes, `L[0]` is half-drawn: no later frame may
        // keep it. A direct frame never touches `L[0]`, but the screen has
        // moved on from what it holds all the same.
        self.retained = None;
        self.frame_clip = exec.frame_clip;
        let copies = std::mem::take(&mut self.pending_copies);
        let copies: Vec<(PixelRect, PixelRect)> = if self.frame_clip.is_some() {
            carried_pairs(&copies, width, height)
        } else {
            Vec::new()
        };
        // Every frame changes `L[0]`, whether or not it reaches a swapchain
        // image; an image written later has to catch up on all of them —
        // the carried rows included. A direct frame writes a whole swapchain
        // image, so every image has catching up to do.
        let changed: Option<Vec<PixelRect>> = if direct {
            None
        } else {
            self.frame_clip.as_ref().map(|clip| {
                clip.iter()
                    .copied()
                    .chain(copies.iter().map(|(_, dst)| *dst))
                    .collect()
            })
        };
        self.presents.record_change(changed.as_deref());
        self.begin_marks();
        if !copies.is_empty() {
            self.carry(&copies);
        }
        let direct_frame = match out {
            Out::Direct(frame) => Some(frame),
            _ => None,
        };
        // The pipelines this frame binds: the swapchain-format set on the
        // direct route, the `RGBA16F` set on every other.
        let (scene_pipeline, post_replace, post_over) = if direct_frame.is_some() {
            let route = self
                .direct
                .as_ref()
                .expect("a direct frame means the route exists");
            (route.scene_pipeline, route.post_replace, route.post_over)
        } else {
            (self.scene_pipeline, self.post_replace, self.post_over)
        };
        let mut rec = Recorder {
            open: None,
            width,
            height,
            frame_area: self
                .frame_clip
                .as_deref()
                .and_then(|clip| damage_area(clip, width, height))
                .unwrap_or_else(|| full_rect(width, height)),
            direct: direct_frame,
            scene_pipeline,
            post_replace,
            post_over,
        };
        // Which pipeline (and its descriptor set and buffers) is bound, so a
        // run of same-kind ops binds once — the dedup the traversal's single
        // mark per kind already implies.
        let mut bound = Bound::None;
        for op in &exec.ops {
            match op {
                SceneOp::Begin {
                    target,
                    clear,
                    kind,
                } => {
                    if let Some(kind) = kind {
                        self.mark(*kind);
                    }
                    rec.begin(self, *target, *clear);
                    bound = Bound::None;
                }
                SceneOp::ClearRegions { rects, color } => {
                    let rects: Vec<vk::ClearRect> = rects
                        .iter()
                        .map(|r| vk::ClearRect {
                            rect: pixel_scissor(*r),
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .collect();
                    if !rects.is_empty() {
                        unsafe {
                            self.device.cmd_clear_attachments(
                                self.cmd(),
                                &[vk::ClearAttachment {
                                    aspect_mask: vk::ImageAspectFlags::COLOR,
                                    color_attachment: 0,
                                    clear_value: vk::ClearValue {
                                        color: vk::ClearColorValue { float32: *color },
                                    },
                                }],
                                &rects,
                            );
                        }
                    }
                }
                SceneOp::DrawRun {
                    target,
                    first_index,
                    index_count,
                    scissor,
                } => {
                    self.mark(PassKind::Draw);
                    rec.ensure_open(self, *target);
                    if bound != Bound::Scene {
                        self.bind_scene(rec.scene_pipeline);
                        bound = Bound::Scene;
                    }
                    unsafe {
                        self.device
                            .cmd_set_scissor(self.cmd(), 0, &[pixel_scissor(*scissor)]);
                        self.device.cmd_draw_indexed(
                            self.cmd(),
                            *index_count,
                            1,
                            *first_index,
                            0,
                            0,
                        );
                    }
                }
                SceneOp::Post {
                    pass,
                    scissors,
                    kind,
                } => {
                    self.mark(*kind);
                    let set = self.post_set(pass.src, pass.aux)?;
                    rec.ensure_open(self, pass.into);
                    if bound != Bound::Post(pass.over) {
                        unsafe {
                            self.device.cmd_bind_pipeline(
                                self.cmd(),
                                vk::PipelineBindPoint::GRAPHICS,
                                if pass.over {
                                    rec.post_over
                                } else {
                                    rec.post_replace
                                },
                            );
                        }
                        bound = Bound::Post(pass.over);
                    }
                    unsafe {
                        self.device.cmd_bind_descriptor_sets(
                            self.cmd(),
                            vk::PipelineBindPoint::GRAPHICS,
                            self.post_layout,
                            0,
                            &[set],
                            &[],
                        );
                        self.device.cmd_push_constants(
                            self.cmd(),
                            self.post_layout,
                            vk::ShaderStageFlags::FRAGMENT,
                            0,
                            as_bytes(std::slice::from_ref(&pass.push)),
                        );
                        for scissor in scissors {
                            self.device
                                .cmd_set_scissor(self.cmd(), 0, &[pixel_scissor(*scissor)]);
                            self.device.cmd_draw(self.cmd(), 3, 1, 0, 0);
                        }
                    }
                }
            }
        }
        rec.end(self);
        self.mark(PassKind::Blit);

        let cmd = self.cmd();
        // A direct frame's render pass already left its image in
        // `PRESENT_SRC_KHR`; there is nothing to copy out or blit. (The blit
        // mark stays, so a frame's GPU attribution still adds up — the
        // interval it opens is simply empty.)
        if !direct {
            let frame = self.levels[0].image;
            unsafe {
                transition(
                    &self.device,
                    cmd,
                    frame,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                );
            }
            match out {
                Out::Readback => unsafe {
                    self.device.cmd_copy_image_to_buffer(
                        cmd,
                        frame,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        self.readback.handle,
                        &[vk::BufferImageCopy {
                            buffer_offset: 0,
                            buffer_row_length: 0,
                            buffer_image_height: 0,
                            image_subresource: color_layers(),
                            image_offset: vk::Offset3D::default(),
                            image_extent: vk::Extent3D {
                                width,
                                height,
                                depth: 1,
                            },
                        }],
                    );
                },
                Out::Direct(_) => {}
                Out::Blit(target) => unsafe {
                    // The frame straight into the presentable image. The blit
                    // converts RGBA16F to the swapchain's 8-bit UNORM (and to
                    // BGRA order when that is the format) on the GPU; the frame
                    // is opaque (see `render_planned_to`), so premultiplied and
                    // straight are the same numbers and nothing is lost. The
                    // source corner is the render extent, the destination the
                    // swapchain image's — the same numbers at full resolution,
                    // an enlargement when a resolution scale is on.
                    //
                    // **Only what changed since this image was last written**,
                    // once this device has shown that its images keep what was
                    // written to them (`present_history`); until then, and on a
                    // scaled frame, all of it.
                    let [dst_width, dst_height] = target.dst_extent;
                    let plan = self.presents.plan(
                        target.index,
                        target.generation,
                        (dst_width, dst_height),
                        target.partial,
                        target.src_extent == target.dst_extent,
                    );
                    if plan.read_before || plan.read_after || plan.compare_frame {
                        // The probe and readback buffers are one pair, shared
                        // by every frame: a frame that reads an image waits
                        // for all earlier frames rather than race another
                        // probe. Probing runs while trust is being
                        // established and in check mode; a trusted steady
                        // frame never takes this.
                        self.settle()?;
                    }
                    if plan.read_before || plan.read_after {
                        self.ensure_probe_buffers(
                            u64::from(dst_width) * u64::from(dst_height) * 4,
                        )?;
                    }
                    let image_copy = vk::BufferImageCopy {
                        buffer_offset: 0,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: color_layers(),
                        image_offset: vk::Offset3D::default(),
                        image_extent: vk::Extent3D {
                            width: dst_width,
                            height: dst_height,
                            depth: 1,
                        },
                    };
                    if plan.read_before {
                        // What the image kept since it was last written.
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            vk::ImageLayout::PRESENT_SRC_KHR,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        );
                        self.device.cmd_copy_image_to_buffer(
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            self.probe_before.handle,
                            &[image_copy],
                        );
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        );
                    } else {
                        // A partial copy keeps the image's pixels, so its layout
                        // moves on from where the last present left it; a full
                        // one lets the driver discard them.
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            if plan.region.is_some() {
                                vk::ImageLayout::PRESENT_SRC_KHR
                            } else {
                                vk::ImageLayout::UNDEFINED
                            },
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        );
                    }
                    #[expect(clippy::cast_possible_wrap, reason = "frame extents")]
                    let corner = |extent: [u32; 2]| vk::Offset3D {
                        x: extent[0] as i32,
                        y: extent[1] as i32,
                        z: 1,
                    };
                    let regions: Vec<vk::ImageBlit> = match &plan.region {
                        None => vec![vk::ImageBlit {
                            src_subresource: color_layers(),
                            src_offsets: [vk::Offset3D::default(), corner(target.src_extent)],
                            dst_subresource: color_layers(),
                            dst_offsets: [vk::Offset3D::default(), corner(target.dst_extent)],
                        }],
                        Some(rects) => rects
                            .iter()
                            .map(|r| {
                                let offsets = [
                                    vk::Offset3D {
                                        x: r.x0,
                                        y: r.y0,
                                        z: 0,
                                    },
                                    vk::Offset3D {
                                        x: r.x1,
                                        y: r.y1,
                                        z: 1,
                                    },
                                ];
                                vk::ImageBlit {
                                    src_subresource: color_layers(),
                                    src_offsets: offsets,
                                    dst_subresource: color_layers(),
                                    dst_offsets: offsets,
                                }
                            })
                            .collect(),
                    };
                    if !regions.is_empty() {
                        self.device.cmd_blit_image(
                            cmd,
                            frame,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            target.image,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            &regions,
                            target.filter,
                        );
                    }
                    if plan.read_after {
                        // What the image now holds, for the next frame that
                        // writes it to check against.
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        );
                        self.device.cmd_copy_image_to_buffer(
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            self.probe_after.handle,
                            &[image_copy],
                        );
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk::ImageLayout::PRESENT_SRC_KHR,
                        );
                    } else {
                        transition(
                            &self.device,
                            cmd,
                            target.image,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::ImageLayout::PRESENT_SRC_KHR,
                        );
                    }
                    if plan.compare_frame {
                        // The frame itself, to compare the image with.
                        self.device.cmd_copy_image_to_buffer(
                            cmd,
                            frame,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            self.readback.handle,
                            &[image_copy],
                        );
                    }
                    present_history::count_copy(
                        &plan,
                        (dst_width, dst_height),
                        self.presents.trust(),
                        target.partial,
                    );
                    if plan.read_before || plan.read_after || plan.compare_frame {
                        self.pending_probe = Some(PendingProbe {
                            index: target.index,
                            before: plan.read_before,
                            after: plan.read_after,
                            compare: plan.compare_frame,
                            extent: (dst_width, dst_height),
                            bgra: target.bgra,
                            slot: self.recording,
                        });
                    }
                },
            }
            // `L[0]` returns to the layout every target rests in.
            let frame = self.levels[0].image;
            unsafe {
                transition(
                    &self.device,
                    cmd,
                    frame,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
        }
        self.end_marks();
        self.timings.record = recording.elapsed();
        self.frame_clip = None;
        // What waiting for the previous frame cost this one, when they
        // overlap — see `settle`.
        let carried = std::mem::take(&mut self.settled_wait);
        // The direct route's render pass is the first thing to touch the
        // swapchain image, so the acquire semaphore is waited where that
        // pass begins; the blit waits at `TRANSFER`, as it always did.
        let (wait, signal, stage) = match out {
            Out::Readback => (None, None, vk::PipelineStageFlags::TRANSFER),
            Out::Blit(target) => (
                Some(target.ready),
                Some(target.done),
                vk::PipelineStageFlags::TRANSFER,
            ),
            Out::Direct(frame) => (
                Some(frame.ready),
                Some(frame.done),
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ),
        };
        if self.overlap && !matches!(out, Out::Readback) {
            self.timings.submit = self.submit(wait, signal, stage)?;
            self.timings.wait = carried;
            self.slots[self.recording].in_flight = true;
        } else {
            self.timings.submit = self.submit(wait, signal, stage)?;
            let slot = self.recording;
            self.slots[slot].in_flight = true;
            self.timings.wait = carried + self.wait_for_fence()?;
            self.slots[slot].in_flight = false;
            self.collect_marks(slot);
            self.collect_probe()?;
        }
        if !direct {
            self.retained = Some(exec.retained);
        }
        self.frames += 1;
        Ok(exec.redrawn)
    }

    /// Bind the scene pipeline, its descriptor set and the geometry buffers —
    /// the one bind a run of [`SceneOp::DrawRun`] shares. The set, the
    /// vertex buffer and the index buffer are the recording slot's own.
    fn bind_scene(&self, pipeline: vk::Pipeline) {
        let cmd = self.cmd();
        let slot = &self.slots[self.recording];
        unsafe {
            self.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_layout,
                0,
                &[self.scene_sets[self.recording]],
                &[],
            );
            self.device
                .cmd_bind_vertex_buffers(cmd, 0, &[slot.vertices.handle], &[0]);
            self.device
                .cmd_bind_index_buffer(cmd, slot.indices.handle, 0, vk::IndexType::UINT32);
        }
    }

    /// Submit the recorded command buffer and block until it completes.
    /// Returns how long the submit took and how long the fence wait took, so
    /// a frame's caller can attribute them ([`GpuFrameTimings`]); the staging
    /// uploads that share this ignore them.
    fn submit_and_wait(
        &mut self,
        wait: Option<vk::Semaphore>,
    ) -> Result<(Duration, Duration), VulkanError> {
        let submitted = self.submit(wait, None, vk::PipelineStageFlags::TRANSFER)?;
        Ok((submitted, self.wait_for_fence()?))
    }

    /// Submit the recorded command buffer against the slot's fence, waiting
    /// on `wait` before the pipeline `stage` names — the blit's transfers, or
    /// a direct frame's first colour writes — and signalling `signal` when it
    /// is done. Every submission also joins the internal ordering chain: it
    /// waits the semaphore the previous submission signalled at
    /// `COLOR_ATTACHMENT_OUTPUT` — after which come this frame's carries,
    /// draws, atlas uploads and blit, everything that reads or writes what a
    /// previous submission may still be touching — and signals the chain
    /// slot of its own parity for the next one. Returns how long the submit
    /// took.
    fn submit(
        &mut self,
        wait: Option<vk::Semaphore>,
        signal: Option<vk::Semaphore>,
        stage: vk::PipelineStageFlags,
    ) -> Result<Duration, VulkanError> {
        let slot = self.recording;
        debug_assert!(
            !self.slots[slot].in_flight,
            "a submission reused the fence of a frame still in flight; settle it first"
        );
        let cmd = self.slots[slot].command_buffer;
        let mut waits: Vec<vk::Semaphore> = Vec::with_capacity(2);
        let mut stages: Vec<vk::PipelineStageFlags> = Vec::with_capacity(2);
        if let Some(wait) = wait {
            waits.push(wait);
            stages.push(stage);
        }
        if let Some(chain) = self.chain_open {
            waits.push(chain);
            stages.push(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT);
        }
        let parity = usize::try_from(self.submissions % 2).expect("two chain slots");
        let chain_signal = self.chain[parity];
        let signals: Vec<vk::Semaphore> = signal.into_iter().chain([chain_signal]).collect();
        let submitting = Instant::now();
        let submitted = unsafe {
            self.device.end_command_buffer(cmd)?;
            self.device.reset_fences(&[self.slots[slot].fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default()
                    .command_buffers(&[cmd])
                    .wait_semaphores(&waits)
                    .wait_dst_stage_mask(&stages)
                    .signal_semaphores(&signals)],
                self.slots[slot].fence,
            )
        };
        self.chain_open = Some(chain_signal);
        self.submissions += 1;
        submitted?;
        Ok(submitting.elapsed())
    }

    /// Block until the recording slot's last submission signals; how long it
    /// took.
    fn wait_for_fence(&mut self) -> Result<Duration, VulkanError> {
        let waiting = Instant::now();
        let fence = self.slots[self.recording].fence;
        unsafe {
            self.device.wait_for_fences(&[fence], true, u64::MAX)?;
        }
        Ok(waiting.elapsed())
    }

    /// Wait for the frame left in flight, if there is one: after this, every
    /// resource the renderer owns — command buffer, fence, uniform, vertex
    /// and index buffers, atlases, the kept frame `L[0]` — is the CPU's
    /// again, and the frame's GPU timings are read
    /// ([`last_pass_times`](Self::last_pass_times)).
    ///
    /// # Frames in flight
    ///
    /// `render_planned_to` (the swapchain-image submit, behind the
    /// `vulkan-swapchain` feature — this doc builds without it, so the name
    /// is spelled rather than linked) submits a frame into a
    /// swapchain image and returns without waiting for it: the present waits
    /// on the GPU instead (`AcquiredImage`'s `done` semaphore — the type is
    /// the swapchain module's, likewise behind the feature), and the
    /// application builds, lays out and plans its next frame while the GPU
    /// draws this one. That is the overlap Flutter's raster thread and Qt's
    /// render thread exist for, and the LG Stylo 5 is why it matters: about
    /// 5 ms of CPU work and 12 ms of GPU work per frame, run one after the
    /// other, were a 17.3 ms frame against a 16.7 ms budget (Device Farm run
    /// 39538891917).
    ///
    /// Up to `FLIGHT_SLOTS` frames are in flight. A frame's own resources
    /// are per slot, so the frame after it records and submits while the GPU
    /// still draws it; the GPU halves of the two run one after the other,
    /// ordered by the internal chain (see [`SceneRenderer`]'s field docs),
    /// because a retained frame's damage renders into what the frame before
    /// left in `L[0]`. A caller that wants the machine quiet — before
    /// presenting through any path but the frame's own, before resizing —
    /// calls this and waits for all of them; a caller about to render the
    /// next frame calls [`settle_for_next_frame`](Self::settle_for_next_frame)
    /// and keeps the overlap.
    ///
    /// The time spent here is charged to the next frame's
    /// [`GpuFrameTimings::wait`], which therefore means "waiting for the
    /// oldest frame in flight" while frames overlap; [`GpuPassTimes`] is
    /// the GPU's own time.
    ///
    /// # Errors
    ///
    /// The fence wait failing (a lost device).
    pub fn settle(&mut self) -> Result<(), VulkanError> {
        for index in 0..FLIGHT_SLOTS {
            self.settle_slot(index)?;
        }
        // The retired atlas textures: every frame that could still be
        // executing has now finished, so the copies that carried their
        // content into the replacements have too, and they can be freed.
        for retired in self.retired_textures.drain(..) {
            unsafe { destroy_texture(&self.device, retired) };
        }
        Ok(())
    }

    /// Whether a frame into a swapchain image is left in flight
    /// ([`settle`](Self::settle)) — on by default. Off waits for every frame
    /// before returning, as this renderer always did: the comparison a phone
    /// with a doubtful driver needs, and nothing else.
    ///
    /// # Errors
    ///
    /// Settling a frame in flight failing.
    pub fn set_overlap(&mut self, on: bool) -> Result<(), VulkanError> {
        self.settle()?;
        self.overlap = on;
        Ok(())
    }

    /// Whether frames into a swapchain image overlap the caller's work — see
    /// [`set_overlap`](Self::set_overlap).
    #[must_use]
    pub const fn overlaps(&self) -> bool {
        self.overlap
    }

    /// Host-visible buffers of at least `bytes` for a swapchain image's
    /// reads (`present_history`).
    fn ensure_probe_buffers(&mut self, bytes: u64) -> Result<(), VulkanError> {
        for slot in [&mut self.probe_before, &mut self.probe_after] {
            if slot.capacity < bytes {
                let fresh = allocate_buffer(
                    &self.device,
                    vk::BufferUsageFlags::TRANSFER_DST,
                    self.host_visible_memory,
                    bytes,
                )?;
                let old = std::mem::replace(slot, fresh);
                // SAFETY: every frame that could read `old` has been waited
                // for — the caller settled before recording this one.
                unsafe { destroy_buffer(&self.device, old) };
            }
        }
        Ok(())
    }

    /// The bytes of a probe buffer.
    fn probe_bytes(&self, buffer: &Buffer, len: usize) -> Result<Vec<u8>, VulkanError> {
        let mut out = vec![0u8; len];
        // SAFETY: host-visible memory of at least `len` bytes, written by a
        // frame whose fence has signalled.
        unsafe {
            let ptr =
                self.device
                    .map_memory(buffer.memory, 0, len as u64, vk::MemoryMapFlags::empty())?
                    as *const u8;
            std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr(), len);
            self.device.unmap_memory(buffer.memory);
        }
        Ok(out)
    }

    /// Look at the last frame's swapchain-image reads, now that the GPU
    /// has finished it: what the image kept, what it now holds, and — in
    /// check mode — whether that is the frame.
    fn collect_probe(&mut self) -> Result<(), VulkanError> {
        let Some(probe) = self.pending_probe.take() else {
            return Ok(());
        };
        let (width, height) = probe.extent;
        let len = width as usize * height as usize * 4;
        let before = if probe.before {
            Some(present_history::hash_bytes(
                &self.probe_bytes(&self.probe_before, len)?,
            ))
        } else {
            None
        };
        let after = if probe.after {
            Some(self.probe_bytes(&self.probe_after, len)?)
        } else {
            None
        };
        self.presents.verified(
            probe.index,
            before,
            after.as_deref().map(present_history::hash_bytes),
        );
        if let (true, Some(image)) = (probe.compare, after.as_deref()) {
            let frame = self.read_back(width, height)?;
            // The blit's float-to-8-bit conversion and the oracle's may
            // round a channel apart by one; a trail is not that subtle.
            let differs = |a: u8, b: u8| a.abs_diff(b) > 1;
            let bad = image
                .as_chunks::<4>()
                .0
                .iter()
                .zip(frame.as_chunks::<4>().0)
                .filter(|(i, f)| {
                    let i = if probe.bgra {
                        [i[2], i[1], i[0], i[3]]
                    } else {
                        **i
                    };
                    (0..3).any(|c| differs(i[c], f[c]))
                })
                .count();
            self.presents.frame_mismatch(bad as u64);
        }
        Ok(())
    }

    /// Premultiplied RGBA16F → straight RGBA8, rounded exactly as
    /// `NativeRenderer`'s `Premul::to_straight_u8`.
    fn read_back(&self, width: u32, height: u32) -> Result<Vec<u8>, VulkanError> {
        let texels = width as usize * height as usize;
        let mut half = vec![0u16; texels * 4];
        unsafe {
            let ptr = self.device.map_memory(
                self.readback.memory,
                0,
                (texels * 8) as u64,
                vk::MemoryMapFlags::empty(),
            )? as *const u16;
            std::ptr::copy_nonoverlapping(ptr, half.as_mut_ptr(), texels * 4);
            self.device.unmap_memory(self.readback.memory);
        }
        // The shared readback conversion — see `scene_exec`'s module doc for
        // why "what a finished frame reads back as" is one definition.
        Ok(crate::scene_exec::straight_rgba8_from_rgba16f(&half))
    }
}

/// Tracks the open render pass, so consecutive operations on one target
/// share it.
struct Recorder {
    open: Option<TargetId>,
    width: u32,
    height: u32,
    /// The render area of a pass into `L[0]`: the bounds of the frame's
    /// damage when it redraws only that, the whole frame otherwise.
    ///
    /// On a tiled GPU (every phone) the render area is what a pass loads
    /// from and stores back to memory, tile by tile. Full-frame, a pass over
    /// the RGBA16F frame moved ~20 MB each way however little it drew —
    /// ~30 ms of "draw" on the Redmi's Adreno 612 for one changed row, the
    /// same as for a whole screen. Everything a damaged frame draws into
    /// `L[0]` is scissored inside the damage, so the area loses nothing.
    frame_area: vk::Rect2D,
    /// The swapchain image a direct frame renders `Level(0)` into, with the
    /// render passes and pipelines for its format — `None` on the `L[0]`
    /// routes, where `Recorder` draws into the renderer's own targets.
    direct: Option<DirectFrame>,
    /// The scene pipeline this frame binds: the swapchain-format one when
    /// `direct`, the `RGBA16F` one otherwise.
    scene_pipeline: vk::Pipeline,
    post_replace: vk::Pipeline,
    post_over: vk::Pipeline,
}

impl Recorder {
    fn begin(&mut self, renderer: &SceneRenderer, target: TargetId, clear: Option<[f32; 4]>) {
        self.end(renderer);
        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: clear.unwrap_or([0.0; 4]),
            },
        }];
        let (render_pass, framebuffer, area) = if target == TargetId::Level(0) {
            match self.direct {
                Some(frame) => {
                    let route = renderer
                        .direct
                        .as_ref()
                        .expect("a direct frame means the route exists");
                    (
                        if clear.is_some() {
                            route.rp_clear
                        } else {
                            route.rp_load
                        },
                        frame.framebuffer,
                        full_rect(self.width, self.height),
                    )
                }
                None => (
                    if clear.is_some() {
                        renderer.rp_clear
                    } else {
                        renderer.rp_load
                    },
                    renderer.target(target).framebuffer,
                    self.frame_area,
                ),
            }
        } else {
            (
                if clear.is_some() {
                    renderer.rp_clear
                } else {
                    renderer.rp_load
                },
                renderer.target(target).framebuffer,
                full_rect(self.width, self.height),
            )
        };
        let info = vk::RenderPassBeginInfo::default()
            .render_pass(render_pass)
            .framebuffer(framebuffer)
            .render_area(area)
            .clear_values(&clear_values);
        #[allow(clippy::cast_precision_loss)]
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        unsafe {
            renderer.device.cmd_begin_render_pass(
                renderer.cmd(),
                &info,
                vk::SubpassContents::INLINE,
            );
            renderer
                .device
                .cmd_set_viewport(renderer.cmd(), 0, &[viewport]);
        }
        self.open = Some(target);
    }

    fn ensure_open(&mut self, renderer: &SceneRenderer, target: TargetId) {
        if self.open != Some(target) {
            self.begin(renderer, target, None);
        }
    }

    fn end(&mut self, renderer: &SceneRenderer) {
        if self.open.take().is_some() {
            unsafe { renderer.device.cmd_end_render_pass(renderer.cmd()) };
        }
    }
}

impl std::fmt::Debug for SceneRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneRenderer")
            .field("size", &self.size)
            .field("targets", &self.levels.len())
            .field("vertex_capacity", &self.slots[0].vertices.capacity)
            .finish_non_exhaustive()
    }
}

impl Drop for SceneRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for target in self.levels.drain(..).chain(self.scratch.drain(..)) {
                destroy_target(&self.device, target);
            }
            destroy_target(
                &self.device,
                std::mem::replace(&mut self.dummy, Target::NULL),
            );
            for buffer in [
                std::mem::replace(&mut self.readback, Buffer::EMPTY),
                std::mem::replace(&mut self.probe_before, Buffer::EMPTY),
                std::mem::replace(&mut self.probe_after, Buffer::EMPTY),
            ] {
                destroy_buffer(&self.device, buffer);
            }
            for slot in Slot::ALL {
                let texture = std::mem::replace(self.texture_mut(slot), Texture::NULL);
                destroy_texture(&self.device, texture);
            }
            for retired in self.retired_textures.drain(..) {
                destroy_texture(&self.device, retired);
            }
            for flight in self.slots.iter_mut() {
                destroy_buffer(
                    &self.device,
                    std::mem::replace(&mut flight.uniform, Buffer::EMPTY),
                );
                destroy_buffer(
                    &self.device,
                    std::mem::replace(&mut flight.vertices, Buffer::EMPTY),
                );
                destroy_buffer(
                    &self.device,
                    std::mem::replace(&mut flight.indices, Buffer::EMPTY),
                );
                self.device.destroy_fence(flight.fence, None);
                if let Some(queries) = flight.queries.take() {
                    self.device.destroy_query_pool(queries.pool, None);
                }
            }
            for semaphore in self.chain {
                self.device.destroy_semaphore(semaphore, None);
            }
            self.device.destroy_command_pool(self.command_pool, None);
            for pipeline in [self.scene_pipeline, self.post_replace, self.post_over] {
                self.device.destroy_pipeline(pipeline, None);
            }
            self.device.destroy_pipeline_layout(self.scene_layout, None);
            self.device.destroy_pipeline_layout(self.post_layout, None);
            for module in self.shaders.drain(..) {
                self.device.destroy_shader_module(module, None);
            }
            self.device.destroy_descriptor_pool(self.scene_pool, None);
            self.device.destroy_descriptor_pool(self.post_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.scene_set_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.post_set_layout, None);
            self.device.destroy_render_pass(self.rp_load, None);
            self.device.destroy_render_pass(self.rp_clear, None);
            if let Some(direct) = self.direct.take() {
                for (_, _, view, framebuffer) in direct.frames {
                    self.device.destroy_image_view(view, None);
                    self.device.destroy_framebuffer(framebuffer, None);
                }
                self.device.destroy_render_pass(direct.rp_load, None);
                self.device.destroy_render_pass(direct.rp_clear, None);
                for pipeline in [direct.scene_pipeline, direct.post_replace, direct.post_over] {
                    self.device.destroy_pipeline(pipeline, None);
                }
            }
        }
    }
}

// ── construction helpers ───────────────────────────────────────────────────

/// `vieww_gpu::Vertex`, field by field: 2 + 4 + 2 + 1 + 4 + 2 + 4 + 4 + 4.
/// Offsets in floats: position 0, color 2, uv 6, kind 8, params 9,
/// local 13, extra 15, shape 19, mask 23 — WGSL location 7 is `mask`
/// (23), location 8 is `shape` (19). One definition, because the direct
/// route builds the same pipeline against another format.
fn scene_vertex_layout() -> (
    vk::VertexInputBindingDescription,
    [vk::VertexInputAttributeDescription; 9],
) {
    // `Vertex::color` packs to UNORM8 bytes: a straight-alpha colour is
    // `k/255` per channel by construction, and UNORM8 decodes to exactly
    // `k/255` — the same f32s the shader always received, in four bytes
    // instead of sixteen. Every other attribute is f32 (byte offsets now,
    // not float indices, since the colour slot broke the uniform stride).
    let attribute = |location: u32, format: vk::Format, offset: u32| {
        vk::VertexInputAttributeDescription {
            location,
            binding: 0,
            format,
            offset,
        }
    };
    let f = |n: u32| n * std::mem::size_of::<f32>() as u32;
    (
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Vertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        },
        [
            attribute(0, vk::Format::R32G32_SFLOAT, f(0)),
            attribute(1, vk::Format::R8G8B8A8_UNORM, f(2)),
            attribute(2, vk::Format::R32G32_SFLOAT, f(3)),
            attribute(3, vk::Format::R32_SFLOAT, f(5)),
            attribute(4, vk::Format::R32G32B32A32_SFLOAT, f(6)),
            attribute(5, vk::Format::R32G32_SFLOAT, f(10)),
            attribute(6, vk::Format::R32G32B32A32_SFLOAT, f(12)),
            attribute(7, vk::Format::R32G32B32A32_SFLOAT, f(20)),
            attribute(8, vk::Format::R32G32B32A32_SFLOAT, f(16)),
        ],
    )
}

fn create_render_pass(
    device: &ash::Device,
    load: vk::AttachmentLoadOp,
    format: vk::Format,
    final_layout: vk::ImageLayout,
) -> Result<vk::RenderPass, VulkanError> {
    // Every target rests in SHADER_READ_ONLY_OPTIMAL between passes, so a
    // later pass finds it in the layout it needs. A clearing pass may discard
    // the old contents (UNDEFINED); a loading pass may not. The direct
    // route's passes rest in PRESENT_SRC_KHR instead — the layout the
    // compositor leaves a swapchain image in and reads it back from.
    //
    // **The layout is not the synchronisation, which is what the two
    // dependencies below are for.** A layer is drawn into its own target in
    // one pass and sampled from the next, and nothing in a render pass orders
    // those against each other on its own: a driver is free to start the
    // second pass's fragment work before the first pass's colour writes have
    // landed. A software rasteriser runs the passes one after another and
    // forgives the omission — every one of these suites passed on llvmpipe —
    // while real hardware overlaps them. On a GeForce 920MX that showed up as
    // four failures in `vulkan_compositor.rs`, all of them a pass reading what
    // an earlier pass had written: `shaped-clips` sampled a mask that was not
    // there yet and left 2413 pixels unclipped, and backdrop blur, shadow
    // spread and nested layers came back a few steps off.
    let initial = if load == vk::AttachmentLoadOp::CLEAR {
        vk::ImageLayout::UNDEFINED
    } else if final_layout == vk::ImageLayout::PRESENT_SRC_KHR {
        vk::ImageLayout::PRESENT_SRC_KHR
    } else {
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
    };
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(load)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(initial)
        .final_layout(final_layout)];
    let color_ref = [vk::AttachmentReference {
        attachment: 0,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref)];
    let dependencies = [
        // Reads of this target by an earlier pass finish before this one
        // writes it (write-after-read).
        vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: vk::PipelineStageFlags::FRAGMENT_SHADER,
            dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            src_access_mask: vk::AccessFlags::SHADER_READ,
            dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                | vk::AccessFlags::COLOR_ATTACHMENT_READ,
            dependency_flags: vk::DependencyFlags::BY_REGION,
        },
        // This pass's writes are complete and visible before any later pass
        // samples the target (read-after-write). The one that was missing.
        vk::SubpassDependency {
            src_subpass: 0,
            dst_subpass: vk::SUBPASS_EXTERNAL,
            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            dst_stage_mask: vk::PipelineStageFlags::FRAGMENT_SHADER,
            src_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            dst_access_mask: vk::AccessFlags::SHADER_READ,
            dependency_flags: vk::DependencyFlags::BY_REGION,
        },
    ];
    Ok(unsafe {
        device.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&attachments)
                .subpasses(&subpasses)
                .dependencies(&dependencies),
            None,
        )
    }?)
}

fn create_post_pool(device: &ash::Device) -> Result<vk::DescriptorPool, VulkanError> {
    // Sets are keyed by (src, aux) target pairs — a handful per level, so
    // this is far beyond `MAX_TARGET_DEPTH`'s need.
    Ok(unsafe {
        device.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&[vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 3 * 1024,
                }])
                .max_sets(1024),
            None,
        )
    }?)
}

fn create_pipeline(
    (device, cache): (&ash::Device, vk::PipelineCache),
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    (vs, fs): (vk::ShaderModule, vk::ShaderModule),
    bindings: &[vk::VertexInputBindingDescription],
    attributes: &[vk::VertexInputAttributeDescription],
    premultiplied_over: bool,
) -> Result<vk::Pipeline, VulkanError> {
    let vs_name = CString::new("vs_main").expect("no interior nul");
    let fs_name = CString::new("fs_main").expect("no interior nul");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vs)
            .name(&vs_name),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fs)
            .name(&fs_name),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(bindings)
        .vertex_attribute_descriptions(attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    // Premultiplied source-over, or no blending at all.
    let blend = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(premultiplied_over)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let color_blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend);
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0);
    let pipelines = unsafe { device.create_graphics_pipelines(cache, &[info], None) }
        .map_err(|(_, e)| VulkanError::Vulkan(e.to_string()))?;
    Ok(pipelines[0])
}

fn allocate_image(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    format: vk::Format,
    (width, height): (u32, u32),
    usage: vk::ImageUsageFlags,
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), VulkanError> {
    let image = unsafe {
        device.create_image(
            &vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED),
            None,
        )
    }?;
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(pick_memory_type(
                    mem_props,
                    requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )?),
            None,
        )
    }?;
    unsafe { device.bind_image_memory(image, memory, 0) }?;
    let view = unsafe {
        device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(color_range()),
            None,
        )
    }?;
    Ok((image, memory, view))
}

fn create_target(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    render_pass: vk::RenderPass,
    width: u32,
    height: u32,
) -> Result<Target, VulkanError> {
    let (image, memory, view) = allocate_image(
        device,
        mem_props,
        TARGET_FORMAT,
        (width, height),
        vk::ImageUsageFlags::COLOR_ATTACHMENT
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    )?;
    let framebuffer = unsafe {
        device.create_framebuffer(
            &vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&[view])
                .width(width)
                .height(height)
                .layers(1),
            None,
        )
    }?;
    Ok(Target {
        image,
        memory,
        view,
        framebuffer,
    })
}

fn create_texture(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    format: vk::Format,
    width: u32,
    height: u32,
) -> Result<Texture, VulkanError> {
    let (image, memory, view) = allocate_image(
        device,
        mem_props,
        format,
        (width, height),
        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
    )?;
    Ok(Texture {
        image,
        memory,
        view,
        format,
        width,
        height,
        version: None,
        staging: [Buffer::EMPTY; FLIGHT_SLOTS],
        staging_idle: [0; FLIGHT_SLOTS],
    })
}

const fn bytes_per_texel(format: vk::Format) -> usize {
    match format {
        vk::Format::R8_UNORM => 1,
        vk::Format::R8G8B8A8_UNORM => 4,
        vk::Format::R32G32B32A32_SFLOAT => 16,
        _ => 8,
    }
}

unsafe fn destroy_target(device: &ash::Device, target: Target) {
    if target.image == vk::Image::null() {
        return;
    }
    device.destroy_framebuffer(target.framebuffer, None);
    device.destroy_image_view(target.view, None);
    device.destroy_image(target.image, None);
    device.free_memory(target.memory, None);
}

unsafe fn destroy_texture(device: &ash::Device, texture: Texture) {
    for buffer in texture.staging {
        destroy_buffer(device, buffer);
    }
    if texture.image == vk::Image::null() {
        return;
    }
    device.destroy_image_view(texture.view, None);
    device.destroy_image(texture.image, None);
    device.free_memory(texture.memory, None);
}

const fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

const fn color_layers() -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    }
}

unsafe fn transition(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    from: vk::ImageLayout,
    to: vk::ImageLayout,
) {
    let access = |layout: vk::ImageLayout| match layout {
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => vk::AccessFlags::TRANSFER_WRITE,
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL => vk::AccessFlags::TRANSFER_READ,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => vk::AccessFlags::SHADER_READ,
        _ => vk::AccessFlags::empty(),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(from)
        .new_layout(to)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(color_range())
        .src_access_mask(access(from))
        .dst_access_mask(access(to));
    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::ALL_COMMANDS,
        vk::PipelineStageFlags::ALL_COMMANDS,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier],
    );
}

const fn full_rect(width: u32, height: u32) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D { width, height },
    }
}

/// Each `(src, dx, dy)` as a `(src, dst)` pair clipped jointly to the frame:
/// the source shrinks to the rows that land somewhere, and the destination is
/// always that source moved — never clipped on its own, which would slide
/// the carried rows.
fn carried_pairs(
    copies: &[(PixelRect, i32, i32)],
    width: u32,
    height: u32,
) -> Vec<(PixelRect, PixelRect)> {
    #[expect(clippy::cast_possible_wrap, reason = "frame extents are small")]
    let frame = PixelRect {
        x0: 0,
        y0: 0,
        x1: width as i32,
        y1: height as i32,
    };
    copies
        .iter()
        .filter_map(|&(src, dx, dy)| {
            let landing = PixelRect {
                x0: frame.x0 - dx,
                y0: frame.y0 - dy,
                x1: frame.x1 - dx,
                y1: frame.y1 - dy,
            };
            let src = src.intersect(frame).intersect(landing);
            if src.is_empty() || (dx == 0 && dy == 0) {
                return None;
            }
            let dst = PixelRect {
                x0: src.x0 + dx,
                y0: src.y0 + dy,
                x1: src.x1 + dx,
                y1: src.y1 + dy,
            };
            Some((src, dst))
        })
        .collect()
}

/// The bounds of `clip`, clamped to the frame, as a render area — `None`
/// when nothing is left, which leaves the area whole (a damaged frame with
/// no damage draws nothing into it either way).
fn damage_area(clip: &[PixelRect], width: u32, height: u32) -> Option<vk::Rect2D> {
    #[expect(clippy::cast_possible_wrap, reason = "frame extents are small")]
    let frame = PixelRect {
        x0: 0,
        y0: 0,
        x1: width as i32,
        y1: height as i32,
    };
    let bounds = clip
        .iter()
        .map(|r| r.intersect(frame))
        .filter(|r| !r.is_empty())
        .reduce(|a, b| PixelRect {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        })?;
    Some(pixel_scissor(bounds))
}

/// `rect` (already inside the frame) as a scissor.
const fn pixel_scissor(rect: PixelRect) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D {
            x: rect.x0,
            y: rect.y0,
        },
        extent: vk::Extent2D {
            width: rect.width(),
            height: rect.height(),
        },
    }
}

fn allocate_buffer(
    device: &ash::Device,
    usage: vk::BufferUsageFlags,
    memory_type: u32,
    size: u64,
) -> Result<Buffer, VulkanError> {
    let size = size.max(4);
    let handle = unsafe {
        device.create_buffer(
            &vk::BufferCreateInfo::default()
                .size(size)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE),
            None,
        )
    }?;
    let requirements = unsafe { device.get_buffer_memory_requirements(handle) };
    let memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type),
            None,
        )
    }?;
    unsafe { device.bind_buffer_memory(handle, memory, 0) }?;
    Ok(Buffer {
        handle,
        memory,
        capacity: size,
    })
}

/// Keep `existing` if it is big enough; otherwise replace it with one of at
/// least double the capacity. Never shrinks — a list that scrolled once will
/// scroll again.
fn grow(
    device: &ash::Device,
    existing: Buffer,
    usage: vk::BufferUsageFlags,
    memory_type: u32,
    needed: u64,
) -> Result<Buffer, VulkanError> {
    if existing.handle != vk::Buffer::null() && existing.capacity >= needed.max(4) {
        return Ok(existing);
    }
    let mut capacity = existing.capacity.max(1024);
    while capacity < needed {
        capacity *= 2;
    }
    unsafe { destroy_buffer(device, existing) };
    allocate_buffer(device, usage, memory_type, capacity)
}

fn write_bytes(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    bytes: &[u8],
) -> Result<(), VulkanError> {
    if bytes.is_empty() {
        return Ok(());
    }
    unsafe {
        let ptr = device
            .map_memory(memory, 0, bytes.len() as u64, vk::MemoryMapFlags::empty())?
            .cast::<u8>();
        ptr.copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
        device.unmap_memory(memory);
    }
    Ok(())
}

unsafe fn destroy_buffer(device: &ash::Device, buffer: Buffer) {
    if buffer.handle != vk::Buffer::null() {
        device.destroy_buffer(buffer.handle, None);
    }
    if buffer.memory != vk::DeviceMemory::null() {
        device.free_memory(buffer.memory, None);
    }
}

fn as_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: only ever called with plain `repr(C)` float/integer data
    // (`Vertex`, `u32`, `f32`, `PostPush`), which has no padding and whose
    // every byte is initialised.
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

fn pick_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    filter: u32,
    flags: vk::MemoryPropertyFlags,
) -> Result<u32, VulkanError> {
    (0..props.memory_type_count)
        .find(|&i| {
            (filter & (1 << i)) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(flags)
        })
        .ok_or_else(|| VulkanError::Vulkan("no suitable memory type".into()))
}

#[cfg(test)]
mod tests {
    use crate::scene_exec::half_to_f32;

    /// A damaged frame's passes into `L[0]` load and store only the damage's
    /// bounds, clamped to the frame — on a tiled GPU, that is the cost.
    #[test]
    fn the_frame_render_area_is_the_damage_bounds() {
        use super::{damage_area, PixelRect};
        let r = |x0, y0, x1, y1| PixelRect { x0, y0, x1, y1 };
        let area = damage_area(&[r(10, 100, 50, 140), r(-5, 300, 2000, 320)], 1080, 2340).unwrap();
        assert_eq!((area.offset.x, area.offset.y), (0, 100));
        assert_eq!((area.extent.width, area.extent.height), (1080, 220));
        assert!(damage_area(&[r(2000, 0, 2100, 10)], 1080, 2340).is_none());
    }

    /// A scroll's copy keeps its delta when the frame edge cuts it: the
    /// source shrinks to the rows that land on the frame, the destination is
    /// that source moved, and a copy that lands nowhere is dropped.
    #[test]
    fn carried_pairs_clip_source_and_destination_together() {
        use super::{carried_pairs, PixelRect};
        let r = |x0, y0, x1, y1| PixelRect { x0, y0, x1, y1 };
        let pairs = carried_pairs(&[(r(0, 10, 100, 200), 0, -30)], 100, 150);
        assert_eq!(pairs, vec![(r(0, 30, 100, 150), r(0, 0, 100, 120))]);
        assert!(carried_pairs(&[(r(0, 0, 10, 10), 0, 500)], 100, 150).is_empty());
        assert!(carried_pairs(&[(r(0, 0, 10, 10), 0, 0)], 100, 150).is_empty());
    }

    #[test]
    fn damage_becomes_disjoint_and_keeps_its_union() {
        use super::{disjoint, PixelRect};
        let r = |x0, y0, x1, y1| PixelRect { x0, y0, x1, y1 };
        let rects = [
            r(0, 0, 10, 10),
            r(5, 5, 15, 15),
            r(2, 2, 8, 8),
            r(20, 0, 30, 4),
        ];
        let out = disjoint(rects);
        let area: i64 = out
            .iter()
            .map(|p| i64::from(p.width()) * i64::from(p.height()))
            .sum();
        // 100 + 100 - 25 overlap; the third is inside the first; the fourth apart.
        assert_eq!(area, 100 + 100 - 25 + 40);
        for (i, a) in out.iter().enumerate() {
            for b in &out[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
        // Every input pixel is still covered.
        for rect in rects {
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    assert!(
                        out.iter()
                            .any(|p| (p.x0..p.x1).contains(&x) && (p.y0..p.y1).contains(&y)),
                        "({x}, {y}) lost"
                    );
                }
            }
        }
    }

    #[test]
    fn half_floats_decode() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x7c00), f32::INFINITY);
        // 0x3555 ≈ 1/3, the value a three-box average of full coverage makes.
        assert!((half_to_f32(0x3555) - 1.0 / 3.0).abs() < 1e-4);
    }
}
