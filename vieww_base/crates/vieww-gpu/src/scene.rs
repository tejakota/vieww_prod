//! Turning a `vieww_paint::Scene` into something a GPU backend can execute.
//!
//! # Where this sits, and why it is here rather than in a backend
//!
//! `vieww-hal`'s Vulkan backend could walk a `Scene` itself. Then Metal would
//! walk it again, and D3D12 a third time, and the three would disagree about
//! what a `Clip` means long before any of them disagreed about a Vulkan call.
//! Everything on this page is backend-independent by construction — path
//! flattening, tessellation, batching, clip resolution, layer structure,
//! colour conversion — so it is computed exactly once, on the CPU, and handed
//! to whichever backend is executing the frame.
//!
//! ```text
//!   vieww_paint::Scene          (what to draw, in vieww's own vocabulary)
//!         │
//!         ▼   Planner::plan()   ← this module: one implementation, no GPU
//!   ScenePlan { vertices, indices, runs, steps, unsupported }
//!         │
//!         ▼
//!   Vulkan │ Metal │ D3D12      ← backends: targets, pipelines, submit
//! ```
//!
//! # The plan is a program, not a mesh
//!
//! A frame with no layers is still one vertex buffer and a handful of
//! `draw_indexed` calls. But a UI is not only fills: a sheet fades as a
//! *group*, a toolbar multiplies over a photo, a glass panel blurs what is
//! behind it, a card casts a shadow. None of those is expressible as geometry
//! drawn straight into the frame, so the plan carries an ordered list of
//! [`Step`]s a backend executes in order:
//!
//! ```text
//!   Draw        runs[a..b] into the current target
//!   PushLayer   open an offscreen target (optionally seeded with the backdrop,
//!               then filtered — CSS backdrop-filter)
//!   PopLayer    filter → shaped-clip multiply → blend (28 modes) × alpha into
//!               the parent
//!   Shadow      colour × cached coverage [× clip], source-over, one pass
//! ```
//!
//! Every step mirrors, operation for operation, what
//! `vieww_paint::native::NativeRenderer` does for the same command, because
//! that renderer is the oracle `vieww-hal`'s parity suite compares against.
//! Where the CPU renderer's *definition* of something lives in its own code —
//! the antialiased coverage of a rounded clip, a shadow's silhouette, a
//! gradient's colour ramp, an image's mip pyramid — the planner takes it
//! through `vieww_paint::native::GpuSeam` rather than re-deriving it, so a
//! disagreement between the two renderers is always an *executor* bug.
//!
//! # Coverage contract
//!
//! [`ScenePlan::unsupported`] is the hard coverage boundary, and it is now a
//! list of **resource limits**, not missing features: an atlas that cannot
//! grow further, a layer stack deeper than [`MAX_TARGET_DEPTH`], an image
//! larger than the image atlas. A plan whose `unsupported` is empty is the
//! whole scene. A backend must not show an incomplete plan — see
//! [`ScenePlan::is_complete`].

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use vieww_foundation::{
    BlendMode, Color, GradientGeometry, Offset, Path, PathVerb, Rect, Transform,
};
use vieww_paint::native::{ColorGlyphParts, GlyphAlpha, GlyphCoverage, GpuSeam, SeamShape};
use vieww_paint::{Clip, Command, Paint, Scene};

use crate::atlas::{Atlas, AtlasKey};
use crate::image_atlas::{ImageAtlas, ImageSlot};
use crate::mask_atlas::{MaskAtlas, MaskKey, MaskSlot};
use crate::ramp_atlas::RampAtlas;
use crate::tessellate::{tessellate_fill, tessellate_stroke, TessellationCache};

/// What a vertex's fragment computes. Carried in [`DrawConstants::kind`].
pub mod material {
    /// A flat colour.
    pub const SOLID: f32 = 0.0;
    /// Colour × glyph-atlas coverage, fetched at an integer texel.
    pub const GLYPH: f32 = 1.0;
    /// A bilinear (optionally mip-blended) image sample.
    pub const IMAGE: f32 = 2.0;
    /// A per-fragment gradient: geometry in `params`, ramp row in `extra`.
    pub const GRADIENT: f32 = 3.0;
    /// A flat colour × analytic rounded-rect coverage (see [`Vertex::shape`](super::Vertex::shape)).
    pub const SHAPE_SOLID: f32 = 4.0;
    /// A per-fragment gradient × analytic rounded-rect coverage.
    pub const SHAPE_GRADIENT: f32 = 5.0;
}

/// The half of the old vertex every vertex of one draw item shares.
///
/// `#[repr(C)]` because it is uploaded verbatim: one table per frame, a
/// storage buffer the scene shader indexes with [`Vertex::draw`]. Six
/// `vec4`s — 96 bytes, written once per draw item instead of once per
/// vertex. This is the flat payload the 96-byte vertex carried in every one
/// of its (at least four) copies; the shader's arithmetic is bit-for-bit what
/// it was, because every field holds the same f32s the vertex attributes
/// used to decode to.
///
/// | field          | SOLID | GLYPH                    | IMAGE                        | GRADIENT                         | SHAPE_SOLID / SHAPE_GRADIENT      |
/// |----------------|-------|--------------------------|------------------------------|----------------------------------|-----------------------------------|
/// | `color`        | paint | run colour               | unused (white)               | unused (white)                   | paint / unused                    |
/// | `uv`           | —     | glyph atlas fetch offset | —                            | —                                | `[0]` = shape radius in device px |
/// | `kind`         | SOLID | GLYPH                    | IMAGE                        | GRADIENT                         | SHAPE_SOLID / SHAPE_GRADIENT      |
/// | `params`       | —     | —                        | lower mip patch `x, y, w, h` | geometry (see `gradient_params`) | geometry for SHAPE_GRADIENT       |
/// | `extra`        | —     | —                        | upper mip patch `x, y, w, h` | `row, dither, kind, 0`           | `row, dither, kind, 0`            |
/// | `shape`        | —     | —                        | —                            | —                                | device-space rect `l, t, r, b`    |
/// | `mask`         | `ox, oy, enabled, _` for a shaped clip; `mask[3]` is the mip blend for IMAGE |
///
/// `uv` and `kind` share a `vec4` (`uv_kind`: `xy` the uv, `z` the
/// material, `w` spare) so the table is six aligned `vec4`s and the WGSL
/// struct mirrors it field for field.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct DrawConstants {
    /// Straight-alpha RGBA, pre-decoded to `k/255` per channel — exactly the
    /// f32s the packed UNORM8 vertex colour used to decode to on every
    /// backend, computed the same way on this side of the upload. The shader
    /// premultiplies.
    pub color: [f32; 4],
    /// `xy`: the uv. `z`: the material ([`material`]). `w`: spare.
    pub uv_kind: [f32; 4],
    pub params: [f32; 4],
    pub extra: [f32; 4],
    /// The analytic shape's device-space edges, for the SHAPE materials: a
    /// rounded rectangle's `left, top, right, bottom`, with its radius in
    /// `uv[0]`. Unused (zero) by every other material.
    pub shape: [f32; 4],
    pub mask: [f32; 4],
}

impl DrawConstants {
    /// A SOLID material's entry: a paint colour over a (possibly shaped)
    /// clip mask.
    const fn solid(color: [f32; 4], mask: [f32; 4]) -> Self {
        Self {
            color,
            uv_kind: [0.0, 0.0, material::SOLID, 0.0],
            params: [0.0; 4],
            extra: [0.0; 4],
            shape: [0.0; 4],
            mask,
        }
    }
}

/// One vertex: what varies corner to corner, plus which [`DrawConstants`]
/// entry every vertex of its draw item shares.
///
/// `#[repr(C)]` because it is uploaded verbatim and a backend's vertex
/// attribute descriptions are offsets into exactly this layout. Three
/// fields, 20 bytes, no padding to get wrong.
///
/// The old vertex was 96 bytes (27 floats before the packed colour): the
/// flat half — colour, uv, material, params, extra, shape, mask — rode in
/// every copy, four per quad and hundreds per tessellated mesh, though a
/// triangle's three vertices always agreed on it. It moved behind
/// [`Vertex::draw`]'s index into [`ScenePlan::constants`], which is why the
/// stride is what it is: the per-vertex upload a card-list frame pays drops
/// by three quarters against the 108-byte plan figure, and the table it
/// fetches from is read once per triangle and cached, not once per vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 2],
    /// The paint-space coordinate the IMAGE, GRADIENT and SHAPE materials
    /// interpolate across the draw — the inverse transform of `position`,
    /// normalised to the paint bounds. Zero for the materials that do not
    /// look at it.
    pub local: [f32; 2],
    /// The index of this draw item's [`DrawConstants`] in
    /// [`ScenePlan::constants`].
    pub draw: u32,
}

/// A contiguous run of indices sharing one scissor rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRun {
    /// Offset of this run's first index into [`ScenePlan::indices`].
    pub first_index: u32,
    /// How many indices this run covers — always a multiple of three.
    pub index_count: u32,
    /// The scissor rectangle in device pixels (integral), or `None` for the
    /// whole target. Already intersected with the frame, every enclosing clip
    /// and every enclosing layer.
    pub scissor: Option<Rect>,
}

/// An integral device-pixel rectangle, `x0..x1` × `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl PixelRect {
    pub const EMPTY: Self = Self {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

    /// `rect` rounded outward — the same floor/ceil `NativeRenderer` sizes its
    /// buffers with.
    #[must_use]
    #[expect(clippy::cast_possible_truncation, reason = "device pixel coordinates")]
    pub fn round_out(rect: Rect) -> Self {
        Self {
            x0: rect.left.floor() as i32,
            y0: rect.top.floor() as i32,
            x1: rect.right.ceil() as i32,
            y1: rect.bottom.ceil() as i32,
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        if self.x1 <= self.x0 || self.y1 <= self.y0 {
            Self {
                x1: self.x0,
                y1: self.y0,
                ..self
            }
        } else {
            self
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            (self.x1 - self.x0) as u32
        }
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            (self.y1 - self.y0) as u32
        }
    }

    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
        .normalized()
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    #[must_use]
    pub fn inflate(self, by: i32) -> Self {
        if self.is_empty() {
            return self;
        }
        Self {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
        }
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "device pixel coordinates")]
    pub fn to_rect(self) -> Rect {
        Rect::new(
            self.x0 as f32,
            self.y0 as f32,
            self.x1 as f32,
            self.y1 as f32,
        )
    }
}

/// An `ImageFilter`, resolved into what a backend executes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterOp {
    /// Radius of each of the three horizontal+vertical box passes; `None` for
    /// no blur. `vieww_paint::native::box_radius_for_sigma` of the sigma.
    pub box_radius: Option<u32>,
    /// A 4×5 colour matrix on unpremultiplied RGBA, applied after the blur.
    pub color_matrix: Option<[f32; 20]>,
}

/// A mask in [`MaskAtlas`], addressed by fetch offset. A mask always covers
/// the whole region its step (or its run's scissor) touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskRef {
    /// `device pixel - offset = atlas texel`.
    pub offset: [f32; 2],
}

impl From<MaskSlot> for MaskRef {
    fn from(slot: MaskSlot) -> Self {
        Self {
            offset: slot.fetch_offset(),
        }
    }
}

/// One instruction in a frame's program. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Draw `runs[first_run .. first_run + run_count]` into the current target.
    Draw { first_run: usize, run_count: usize },
    /// Open an offscreen target covering `rect` (transparent outside `rect`).
    ///
    /// With `backdrop: Some(filter)` the target starts as a copy of the
    /// parent's pixels inside `rect`, and `filter` runs on that copy **before**
    /// any of the layer's own content draws — CSS `backdrop-filter`.
    PushLayer {
        rect: PixelRect,
        backdrop: Option<FilterOp>,
    },
    /// Close the innermost layer.
    ///
    /// In order: run `filter` over `rect`; multiply `mask` over `rect`; then
    /// composite `layer × alpha` into the parent through `blend`, touching only
    /// `composite` (the layer's ink inside `rect`, or nothing when `None`).
    PopLayer {
        rect: PixelRect,
        composite: Option<PixelRect>,
        filter: Option<FilterOp>,
        mask: Option<MaskRef>,
        blend: BlendMode,
        alpha: f32,
    },
    /// A blurred rounded-rect shadow, drawn source-over into the current
    /// target within `scissor`: `color × coverage`, one pass.
    ///
    /// `coverage` is the shadow's *finished* coverage over `patch` — blurred,
    /// complemented for an inset shadow, cropped to the surface — computed
    /// once by the CPU renderer's own code (`GpuSeam::shadow_coverage`) and
    /// cached in the mask atlas by shape, not position, so a card that
    /// scrolls keeps it. It used to be the unblurred caster, blurred here by
    /// six passes in a cleared scratch target: eight render passes a shadow,
    /// each a whole-target load and store on a tiling GPU, every frame.
    ///
    /// A shadow under a shaped clip arrives with the clip already folded
    /// into `coverage` (`plan_shadow`'s multiply), so there is no second
    /// mask to carry — which is the whole point: one mask is what a vertex
    /// holds, so every shadow, clipped or not, can join the geometry run
    /// as a quad. This step is what `VIEWW_GPU_SHADOW_PASS=1` forces back
    /// on, for an A/B on one device.
    Shadow {
        patch: PixelRect,
        scissor: PixelRect,
        coverage: MaskRef,
        /// Premultiplied.
        color: [f32; 4],
    },
}

/// A command this planner could not express, by kind.
///
/// Every remaining entry is a resource limit. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unsupported {
    /// A glyph that could not be given atlas texels (the atlas is at
    /// [`Atlas::MAX_SIDE`]), or any glyph in a plan built without a
    /// [`Planner`].
    Glyphs,
    /// An image larger than, or not fitting into, the image atlas.
    Image,
    /// A shadow whose masks do not fit the mask atlas, or whose work target
    /// would exceed [`MAX_TARGET_DEPTH`].
    Shadow,
    /// A gradient whose ramp does not fit the ramp atlas.
    Gradient,
    /// A shaped clip whose mask does not fit the mask atlas.
    ShapedClip,
    /// A layer nested deeper than [`MAX_TARGET_DEPTH`].
    Layer,
}

impl Unsupported {
    /// A short, stable name for reports and test failures.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Glyphs => "glyphs",
            Self::Image => "image",
            Self::Shadow => "shadow",
            Self::Gradient => "gradient",
            Self::ShapedClip => "shaped-clip",
            Self::Layer => "layer",
        }
    }
}

/// How many render targets a backend may be asked to hold at once: the frame
/// itself plus offscreen layers plus one shadow work target.
///
/// A full-frame target per level is what a backend allocates, so this is a
/// memory bound as much as a feature bound. Thirty-two is deeper than any
/// fixture in this workspace nests (the layout stress suite's deepest stack
/// is twenty-four).
pub const MAX_TARGET_DEPTH: usize = 32;

/// A whole frame, ready for a backend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScenePlan {
    /// Every draw's vertices, concatenated.
    pub vertices: Vec<Vertex>,
    /// Every draw's indices, already offset into [`vertices`](Self::vertices).
    pub indices: Vec<u32>,
    /// One entry per draw item, addressed by [`Vertex::draw`] — the flat
    /// half of the frame's geometry, uploaded as one storage buffer. Exactly
    /// as long as there are draw items: a mesh that emitted nothing leaves
    /// no entry behind.
    pub constants: Vec<DrawConstants>,
    /// The draw calls, in paint order. [`Step::Draw`] addresses them.
    pub runs: Vec<DrawRun>,
    /// The frame's program. See the module doc.
    pub steps: Vec<Step>,
    /// The deepest target index any step uses (0 = the frame). A backend
    /// allocates `target_depth + 1` targets.
    pub target_depth: usize,
    /// What this planner could not express, by kind and count. Empty means
    /// the plan is the whole scene.
    pub unsupported: BTreeMap<Unsupported, usize>,
}

impl ScenePlan {
    /// `true` when every command in the scene is in the plan.
    ///
    /// **The gate a caller must check before showing a GPU frame.** An
    /// incomplete plan is not a slightly worse frame; it is one with pieces
    /// missing.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unsupported.is_empty()
    }

    /// Total triangles across every run.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The constants entry a vertex's [`draw`](Vertex::draw) index addresses.
    ///
    /// The CPU-side lens onto a draw item's flat payload — what the vertex
    /// carried inline before the table existed, and what the scene shader
    /// fetches per draw. Panics on an out-of-range index, which only a
    /// hand-built (or corrupted) plan can produce.
    #[must_use]
    pub fn draw_constants(&self, vertex: &Vertex) -> &DrawConstants {
        &self.constants[vertex.draw as usize]
    }

    /// How many `draw_indexed` calls executing this plan's geometry costs.
    #[must_use]
    pub fn draw_call_count(&self) -> usize {
        self.runs.len()
    }

    /// How many offscreen compositing steps (layers and shadows) the plan has.
    #[must_use]
    pub fn offscreen_step_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, Step::PushLayer { .. } | Step::Shadow { .. }))
            .count()
    }

    fn note(&mut self, what: Unsupported) {
        *self.unsupported.entry(what).or_insert(0) += 1;
    }
}

/// Assembles geometry into runs and runs into [`Step::Draw`]s.
struct Builder {
    plan: ScenePlan,
    /// Index of the first run not yet sealed into a `Draw` step.
    open_run: usize,
}

impl Builder {
    fn new() -> Self {
        Self {
            plan: ScenePlan::default(),
            open_run: 0,
        }
    }

    fn push_mesh(
        &mut self,
        constants: DrawConstants,
        corners: impl Iterator<Item = ([f32; 2], [f32; 2])>,
        indices: &[u32],
        scissor: Rect,
    ) {
        let base = u32::try_from(self.plan.vertices.len()).unwrap_or(u32::MAX);
        let before = self.plan.vertices.len();
        // The mesh's own entry, shared by every vertex below — the index a
        // vertex's `draw` carries into the frame's constants table.
        let draw = u32::try_from(self.plan.constants.len()).unwrap_or(u32::MAX);
        self.plan.constants.push(constants);
        self.plan
            .vertices
            .extend(corners.map(|(position, local)| Vertex {
                position,
                local,
                draw,
            }));
        if self.plan.vertices.len() == before || indices.is_empty() {
            self.plan.vertices.truncate(before);
            self.plan.constants.pop();
            return;
        }
        let first_index = u32::try_from(self.plan.indices.len()).unwrap_or(u32::MAX);
        self.plan.indices.extend(indices.iter().map(|i| base + i));
        let added = u32::try_from(indices.len()).unwrap_or(u32::MAX);
        let scissor = Some(scissor);
        let growable = self.plan.runs.len() > self.open_run;
        match self.plan.runs.last_mut() {
            // Only runs that are not sealed into an earlier step may grow.
            Some(run) if growable && run.scissor == scissor => {
                run.index_count += added;
            }
            _ => self.plan.runs.push(DrawRun {
                first_index,
                index_count: added,
                scissor,
            }),
        }
    }

    fn quad(
        &mut self,
        constants: DrawConstants,
        corners: [([f32; 2], [f32; 2]); 4],
        scissor: Rect,
    ) {
        self.push_mesh(constants, corners.into_iter(), &[0, 1, 2, 0, 2, 3], scissor);
    }

    fn seal(&mut self) {
        let len = self.plan.runs.len();
        if len > self.open_run {
            self.plan.steps.push(Step::Draw {
                first_run: self.open_run,
                run_count: len - self.open_run,
            });
        }
        self.open_run = len;
    }

    fn step(&mut self, step: Step) {
        self.seal();
        self.plan.steps.push(step);
    }

    fn finish(mut self) -> ScenePlan {
        self.seal();
        self.plan
    }
}

/// A scene planner with the state a real frame needs.
///
/// Hold one per surface and keep it between frames: the atlases and the
/// rasterizer caches live here, and a steady frame adds to none of them.
#[derive(Debug, Default)]
pub struct Planner {
    atlas: Atlas,
    images: ImageAtlas,
    masks: MaskAtlas,
    ramps: RampAtlas,
    glyphs: GlyphCoverage,
    seam: GpuSeam,
    tessellation: TessellationCache,
}

impl Planner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The glyph coverage atlas (`R8`).
    #[must_use]
    pub const fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The image atlas (straight-alpha `RGBA8`).
    #[must_use]
    pub const fn image_atlas(&self) -> &ImageAtlas {
        &self.images
    }

    /// The clip/shadow mask atlas (`R8`).
    #[must_use]
    pub const fn mask_atlas(&self) -> &MaskAtlas {
        &self.masks
    }

    /// The gradient ramp atlas (premultiplied `RGBA32F`).
    #[must_use]
    pub const fn ramp_atlas(&self) -> &RampAtlas {
        &self.ramps
    }

    /// The tessellation cache's counters, so a test or a benchmark can see
    /// whether lyon actually ran. A steady frame's whole claim is that it did
    /// not.
    #[must_use]
    pub fn tessellation_stats(&self) -> (u64, u64, u64, usize, usize) {
        (
            self.tessellation.hits(),
            self.tessellation.misses(),
            self.tessellation.evictions(),
            self.tessellation.bytes(),
            self.tessellation.len(),
        )
    }

    /// Plan `scene` for execution at `width` x `height` device pixels.
    ///
    /// Never fails: what cannot be expressed is recorded in
    /// [`ScenePlan::unsupported`].
    pub fn plan(&mut self, scene: &Scene, width: f32, height: f32) -> ScenePlan {
        self.atlas.begin_frame();
        self.masks.begin_frame();
        self.ramps.begin_frame();
        self.images.begin_frame();
        plan_with(scene, width, height, Some(self))
    }
}

/// Plan `scene` without atlases.
///
/// Geometry, rectangular clips and layers without shaped clips plan normally;
/// anything that needs an atlas — glyphs, images, gradients, shaped clips,
/// shadows — is reported as unsupported. Kept because most of the planner is
/// much easier to test without a font. **Not the entry point for a real
/// frame**: use [`Planner`].
#[must_use]
pub fn plan(scene: &Scene, width: f32, height: f32) -> ScenePlan {
    plan_with(scene, width, height, None)
}

#[derive(Clone, Copy)]
enum FrameKind {
    Root,
    /// A layer that needs no target of its own: `Normal`, opaque, unfiltered,
    /// unshaped. Drawing its children straight into the parent is exactly
    /// equivalent, because source-over is associative — confined to the
    /// layer's rectangle by scissor, as `NativeRenderer` confines them to its
    /// buffer.
    PassThrough,
    Offscreen {
        backdrop: bool,
    },
    /// A layer that exceeded [`MAX_TARGET_DEPTH`]; everything inside is
    /// skipped (and was reported once, at the push).
    Skipped,
}

struct Frame {
    kind: FrameKind,
    rect: PixelRect,
    /// Where this frame's target has been written, as `NativeRenderer`'s
    /// `Target::ink` tracks it.
    ink: PixelRect,
    /// Target index this frame's content draws into.
    level: usize,
    // PopLayer inputs, for offscreen frames.
    alpha: f32,
    blend: BlendMode,
    filter: vieww_foundation::ImageFilter,
    clip: Clip,
}

fn plan_with(
    scene: &Scene,
    width: f32,
    height: f32,
    mut planner: Option<&mut Planner>,
) -> ScenePlan {
    let surface = Rect::new(0.0, 0.0, width, height);
    let mut out = Builder::new();
    let mut stack: Vec<Frame> = vec![Frame {
        kind: FrameKind::Root,
        rect: PixelRect::round_out(surface),
        ink: PixelRect::EMPTY,
        level: 0,
        alpha: 1.0,
        blend: BlendMode::Normal,
        filter: vieww_foundation::ImageFilter::NONE,
        clip: Clip::NONE,
    }];

    for command in scene.commands() {
        if stack.iter().any(|f| matches!(f.kind, FrameKind::Skipped)) {
            match command {
                Command::PushLayer { .. } => stack.push(skipped_frame()),
                Command::PopLayer if stack.len() > 1 => {
                    stack.pop();
                }
                _ => {}
            }
            continue;
        }

        match command {
            Command::PushLayer {
                bounds,
                alpha,
                blend,
                clip,
                filter,
            } => {
                let top = stack.last().expect("root frame");
                // `NativeRenderer`: bounds ∩ parent buffer ∩ the layer's clip
                // rectangle ∩ surface, rounded out.
                let dev = bounds
                    .intersect(top.rect.to_rect())
                    .intersect(clip.bounds().map_or(surface, |b| b.intersect(surface)))
                    .intersect(surface);
                let rect = PixelRect::round_out(dev);
                let needs_target = *alpha < 1.0
                    || *blend != BlendMode::Normal
                    || !filter.is_noop()
                    || filter.backdrop
                    || !clip.shapes().is_empty();
                if !needs_target {
                    let level = top.level;
                    stack.push(Frame {
                        kind: FrameKind::PassThrough,
                        rect,
                        ink: PixelRect::EMPTY,
                        level,
                        alpha: 1.0,
                        blend: BlendMode::Normal,
                        filter: *filter,
                        clip: Clip::NONE,
                    });
                    continue;
                }
                let level = top.level + 1;
                // One more level is reserved for a shadow's work target.
                if level + 1 >= MAX_TARGET_DEPTH {
                    out.plan.note(Unsupported::Layer);
                    stack.push(skipped_frame());
                    continue;
                }
                out.plan.target_depth = out.plan.target_depth.max(level);
                let backdrop = filter.backdrop.then(|| filter_op(filter));
                out.step(Step::PushLayer { rect, backdrop });
                stack.push(Frame {
                    kind: FrameKind::Offscreen {
                        backdrop: filter.backdrop,
                    },
                    rect,
                    // A backdrop copy writes every pixel of the buffer.
                    ink: if filter.backdrop {
                        rect
                    } else {
                        PixelRect::EMPTY
                    },
                    level,
                    alpha: *alpha,
                    blend: *blend,
                    filter: *filter,
                    clip: clip.clone(),
                });
            }
            Command::PopLayer => {
                if stack.len() <= 1 {
                    continue;
                }
                let frame = stack.pop().expect("checked above");
                match frame.kind {
                    FrameKind::Root | FrameKind::Skipped => {}
                    FrameKind::PassThrough => {
                        let parent = stack.last_mut().expect("root frame");
                        parent.ink = parent.ink.union(frame.ink);
                    }
                    FrameKind::Offscreen { backdrop } => {
                        let mut ink = frame.ink;
                        let filter = (!backdrop && !frame.filter.is_noop()).then(|| {
                            let op = filter_op(&frame.filter);
                            if !ink.is_empty() {
                                if let Some(r) = op.box_radius {
                                    ink = ink.inflate(3 * r as i32).intersect(frame.rect);
                                }
                                if op.color_matrix.is_some() {
                                    ink = frame.rect;
                                }
                            }
                            op
                        });
                        let mut mask = None;
                        if !frame.clip.shapes().is_empty() && !frame.rect.is_empty() {
                            match planner.as_mut() {
                                Some(p) => match p.clip_mask(&frame.clip, frame.rect) {
                                    Some(m) => mask = Some(m),
                                    None => out.plan.note(Unsupported::ShapedClip),
                                },
                                None => out.plan.note(Unsupported::ShapedClip),
                            }
                        }
                        let composite = ink.intersect(frame.rect);
                        let composite = (!composite.is_empty()).then_some(composite);
                        out.step(Step::PopLayer {
                            rect: frame.rect,
                            composite,
                            filter,
                            mask,
                            blend: frame.blend,
                            alpha: frame.alpha.clamp(0.0, 1.0),
                        });
                        if let Some(region) = composite {
                            let parent = stack.last_mut().expect("root frame");
                            parent.ink = parent.ink.union(region);
                        }
                    }
                }
            }
            Command::DrawShadow {
                rect,
                radius,
                shadow,
                transform,
                clip,
                ..
            } => {
                let Some(p) = planner.as_mut() else {
                    out.plan.note(Unsupported::Shadow);
                    continue;
                };
                // Drawn straight into the current target: a shadow needs no
                // target of its own any more, so it neither deepens the plan
                // nor counts against `MAX_TARGET_DEPTH`.
                let top = stack.last().expect("root frame");
                let local_surface = top.rect.to_rect().intersect(surface);
                match p.plan_shadow(*rect, *radius, shadow, *transform, clip, local_surface) {
                    ShadowPlan::Nothing => {}
                    ShadowPlan::Gap => out.plan.note(Unsupported::Shadow),
                    ShadowPlan::Step(step, touched) => {
                        // **A shadow is geometry, not a pass.** Its step is
                        // `color x coverage`, source-over, the coverage a
                        // mask-atlas region addressed `device - offset =
                        // texel` - exactly what a masked solid primitive is in
                        // the scene pipeline (`emit_covered`'s fills use the
                        // same mask slot, the same convention). As a step it
                        // sealed the open geometry run and issued a post draw
                        // of its own: a card list paid one sealed run plus one
                        // shadow draw per card. As a quad it joins the run.
                        // A clipped shadow used to stay a pass - two masks
                        // where a vertex carries one - until `plan_shadow`
                        // learned to fold the clip into the coverage.
                        // `VIEWW_GPU_SHADOW_PASS=1` keeps every shadow a
                        // pass, for an A/B on one device.
                        match &step {
                            Step::Shadow {
                                patch,
                                scissor,
                                coverage,
                                ..
                            } if !shadow_as_pass() => {
                                #[expect(clippy::cast_precision_loss, reason = "device pixels")]
                                let (x0, y0, x1, y1) = (
                                    patch.x0 as f32,
                                    patch.y0 as f32,
                                    patch.x1 as f32,
                                    patch.y1 as f32,
                                );
                                let mask = [coverage.offset[0], coverage.offset[1], 1.0, 0.0];
                                out.push_mesh(
                                    DrawConstants::solid(decoded_rgba(shadow.color), mask),
                                    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                                        .into_iter()
                                        .map(|p| (p, [0.0; 2])),
                                    &[0, 1, 2, 0, 2, 3],
                                    scissor.to_rect(),
                                );
                            }
                            _ => out.step(step),
                        }
                        let top = stack.last_mut().expect("root frame");
                        top.ink = top.ink.union(touched);
                    }
                }
            }
            primitive => {
                let top = stack.last().expect("root frame");
                let Some(target) = clip_region(primitive.clip(), top.rect, surface) else {
                    continue;
                };
                let mask = if primitive.clip().shapes().is_empty() {
                    [0.0; 4]
                } else {
                    let Some(p) = planner.as_mut() else {
                        out.plan.note(Unsupported::ShapedClip);
                        continue;
                    };
                    match p.clip_mask(primitive.clip(), target) {
                        Some(m) => [m.offset[0], m.offset[1], 1.0, 0.0],
                        None => {
                            out.plan.note(Unsupported::ShapedClip);
                            continue;
                        }
                    }
                };
                let before = out.plan.vertices.len();
                plan_primitive(&mut out, planner.as_deref_mut(), primitive, target, mask);
                // Ink: the device bounds of what was emitted, confined to the
                // scissor — `NativeRenderer` marks the bounding box of the
                // coverage it composites.
                if out.plan.vertices.len() > before {
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for v in &out.plan.vertices[before..] {
                        // An analytic shape's quad is inflated by the one-pixel
                        // ramp, but its *coverage* never extends past the
                        // shape — the SDF is zero at half a pixel out. The ink
                        // is the coverage's bounds, and using the quad's would
                        // grow every layer's composite region by a pixel of
                        // nothing: harmless for source-over, wrong for the
                        // blend modes that treat a transparent source as an
                        // instruction (Clear erases wherever it is composited,
                        // so a ring of `blend(Clear, transparent, dst)`
                        // erases a ring the CPU never touches).
                        let c = &out.plan.constants[v.draw as usize];
                        let analytic = c.uv_kind[2] == material::SHAPE_SOLID
                            || c.uv_kind[2] == material::SHAPE_GRADIENT;
                        let (px, py) = if analytic {
                            ([c.shape[0], c.shape[2]], [c.shape[1], c.shape[3]])
                        } else {
                            (
                                [v.position[0], v.position[0]],
                                [v.position[1], v.position[1]],
                            )
                        };
                        x0 = x0.min(px[0]);
                        y0 = y0.min(py[0]);
                        x1 = x1.max(px[1]);
                        y1 = y1.max(py[1]);
                    }
                    let bounds = PixelRect::round_out(Rect::new(x0, y0, x1, y1));
                    let top = stack.last_mut().expect("root frame");
                    top.ink = top.ink.union(bounds.intersect(target));
                }
            }
        }
    }

    out.finish()
}

fn skipped_frame() -> Frame {
    Frame {
        kind: FrameKind::Skipped,
        rect: PixelRect::EMPTY,
        ink: PixelRect::EMPTY,
        level: 0,
        alpha: 1.0,
        blend: BlendMode::Normal,
        filter: vieww_foundation::ImageFilter::NONE,
        clip: Clip::NONE,
    }
}

fn filter_op(filter: &vieww_foundation::ImageFilter) -> FilterOp {
    FilterOp {
        box_radius: (filter.blur_sigma > 0.0)
            .then(|| vieww_paint::native::box_radius_for_sigma(filter.blur_sigma)),
        color_matrix: filter.color_matrix,
    }
}

/// The scissor a primitive draws within: its clip's rectangle ∩ the enclosing
/// target's rectangle. `None` when nothing can be drawn — which is a complete
/// rendering of the command, not a gap.
fn clip_region(clip: &Clip, layer: PixelRect, surface: Rect) -> Option<PixelRect> {
    let bounds = clip.bounds().map_or(surface, |b| b.intersect(surface));
    let region = PixelRect::round_out(bounds).intersect(layer);
    (!region.is_empty()).then_some(region)
}

enum ShadowPlan {
    Nothing,
    Gap,
    Step(Step, PixelRect),
}

fn hash_path(path: &Path, state: &mut impl Hasher) {
    for verb in path.verbs() {
        match *verb {
            PathVerb::MoveTo(p) => (0u8, p.dx.to_bits(), p.dy.to_bits()).hash(state),
            PathVerb::LineTo(p) => (1u8, p.dx.to_bits(), p.dy.to_bits()).hash(state),
            PathVerb::CubicTo(a, b, c) => {
                (2u8, [a.dx, a.dy, b.dx, b.dy, c.dx, c.dy].map(f32::to_bits)).hash(state)
            }
            PathVerb::Close => 3u8.hash(state),
        }
    }
}

struct ClipMaskKey<'a>(&'a Clip, PixelRect);

impl Hash for ClipMaskKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "clip".hash(state);
        self.1.hash(state);
        for shape in self.0.shapes() {
            hash_path(shape, state);
            0xffu8.hash(state);
        }
    }
}

/// The largest shape-coverage patch (in pixels) worth placing in the mask
/// atlas. Above it the shape is tessellated with hard edges instead: a
/// full-window rotated backdrop would otherwise claim most of a 4096² atlas
/// for one draw, and its edges are a sliver of its area.
const MAX_SHAPE_MASK_PIXELS: u64 = 1366 * 1024;

impl Planner {
    /// The CPU renderer's own coverage for a non-analytic shape over
    /// `region`, multiplied by the clip mask the draw already carries, as one
    /// mask in the atlas. `None` when nothing is inked, the patch is too big
    /// to be worth it, or the atlas is full — the caller then falls back to
    /// hard-edged tessellation, which is what every such shape had before.
    fn shape_mask(
        &mut self,
        shape: SeamShape<'_>,
        transform: Transform,
        region: PixelRect,
        clip: &Clip,
        clip_mask: Option<[f32; 2]>,
    ) -> Option<(MaskRef, PixelRect)> {
        // **Translation-invariant when it can be.** A shape's coverage
        // depends on its geometry, the transform's linear part, the
        // sub-pixel phase of its position and — only where the clip cuts it —
        // the clip. Keyed by the absolute position, every scroll step missed
        // for every shape on screen and re-rasterised it on the CPU (85% of
        // the planner's time, ~13 ms a frame on the Redmi). Keyed by phase,
        // the same mask comes back one whole-pixel shift later, and is placed
        // there by moving the slot. A shaped clip folds its own coverage in,
        // so it keeps the exact key.
        let shift = if clip_mask.is_none() {
            let (ix, iy) = (transform.tx.floor(), transform.ty.floor());
            let fits = ix.abs() < 1.0e6 && iy.abs() < 1.0e6;
            fits.then(|| {
                #[expect(clippy::cast_possible_truncation, reason = "bounded above")]
                let (ix, iy) = (ix as i32, iy as i32);
                (ix, iy)
            })
        } else {
            None
        };
        let contained = shift.is_some() && {
            let linear =
                (transform.a.abs() + transform.c.abs()).max(transform.b.abs() + transform.d.abs());
            let local = match shape {
                SeamShape::Rect(r) => r,
                SeamShape::Fill(path) => path.bounds(),
                SeamShape::Stroke(path, stroke) => {
                    let reach = stroke.reach();
                    path.bounds().inflate(reach)
                }
            };
            let device = transform.apply_rect(local).inflate(2.0 + linear);
            let r = region.to_rect();
            device.left >= r.left
                && device.top >= r.top
                && device.right <= r.right
                && device.bottom <= r.bottom
        };
        let key = {
            let mut h = std::hash::DefaultHasher::new();
            "shape".hash(&mut h);
            match shape {
                SeamShape::Rect(r) => {
                    (0u8, [r.left, r.top, r.right, r.bottom].map(f32::to_bits)).hash(&mut h)
                }
                SeamShape::Fill(path) => {
                    1u8.hash(&mut h);
                    hash_path(path, &mut h);
                }
                SeamShape::Stroke(path, stroke) => {
                    2u8.hash(&mut h);
                    hash_path(path, &mut h);
                    // `Debug` of the stroke prints every field, floats in
                    // their shortest round-trip form — a complete key.
                    format!("{stroke:?}").hash(&mut h);
                }
            }
            match shift {
                Some((ix, iy)) => {
                    #[expect(clippy::cast_precision_loss, reason = "bounded integer shift")]
                    let phase = [transform.tx - ix as f32, transform.ty - iy as f32];
                    [
                        transform.a,
                        transform.b,
                        transform.c,
                        transform.d,
                        phase[0],
                        phase[1],
                    ]
                    .map(f32::to_bits)
                    .hash(&mut h);
                    if contained {
                        "contained".hash(&mut h);
                    } else {
                        PixelRect {
                            x0: region.x0 - ix,
                            y0: region.y0 - iy,
                            x1: region.x1 - ix,
                            y1: region.y1 - iy,
                        }
                        .hash(&mut h);
                    }
                }
                None => {
                    [
                        transform.a,
                        transform.b,
                        transform.c,
                        transform.d,
                        transform.tx,
                        transform.ty,
                    ]
                    .map(f32::to_bits)
                    .hash(&mut h);
                    region.hash(&mut h);
                }
            }
            if clip_mask.is_some() {
                ClipMaskKey(clip, region).hash(&mut h);
            }
            MaskKey(h.finish())
        };
        let rect_of = |slot: MaskSlot| PixelRect {
            x0: slot.device_x,
            y0: slot.device_y,
            x1: slot.device_x + i32::try_from(slot.width).unwrap_or(i32::MAX),
            y1: slot.device_y + i32::try_from(slot.height).unwrap_or(i32::MAX),
        };
        // Slots of a shifted key hold their device position relative to the
        // whole-pixel shift; this puts one back where the shape is now.
        let (sx, sy) = shift.unwrap_or((0, 0));
        let placed = move |mut slot: MaskSlot| {
            slot.device_x += sx;
            slot.device_y += sy;
            slot
        };
        if let Some(slot) = self.masks.get(key) {
            let slot = placed(slot);
            return Some((MaskRef::from(slot), rect_of(slot)));
        }
        // On a miss, the clip's coverage over `region`, read out of the atlas
        // before the build borrows it.
        let clip_bytes = clip_mask.map(|offset| {
            let side = self.masks.side() as usize;
            let texels = self.masks.texels();
            let (w, h) = (region.width() as usize, region.height() as usize);
            let mut out = vec![0u8; w * h];
            #[expect(clippy::cast_possible_truncation, reason = "integral offsets")]
            let (ox, oy) = (offset[0] as i32, offset[1] as i32);
            for y in 0..h {
                for x in 0..w {
                    let tx = usize::try_from(region.x0 + x as i32 - ox).ok();
                    let ty = usize::try_from(region.y0 + y as i32 - oy).ok();
                    if let (Some(tx), Some(ty)) = (tx, ty) {
                        if tx < side {
                            out[y * w + x] = texels.get(ty * side + tx).copied().unwrap_or(0);
                        }
                    }
                }
            }
            out
        });
        let seam = &self.seam;
        let slot = self.masks.get_or_insert(key, || {
            let mut patch = seam.shape_coverage(shape, transform, region.to_rect())?;
            if u64::from(patch.width) * u64::from(patch.height) > MAX_SHAPE_MASK_PIXELS {
                return None;
            }
            if let Some(clip) = &clip_bytes {
                let w = region.width() as usize;
                for y in 0..patch.height as usize {
                    for x in 0..patch.width as usize {
                        // The patch lies inside `region` by construction.
                        let rx = usize::try_from(patch.x - region.x0).unwrap_or(0) + x;
                        let ry = usize::try_from(patch.y - region.y0).unwrap_or(0) + y;
                        let c = clip.get(ry * w + rx).copied().unwrap_or(0);
                        let i = y * patch.width as usize + x;
                        let product = u16::from(patch.alpha[i]) * u16::from(c);
                        #[expect(clippy::cast_possible_truncation, reason = "<= 255")]
                        let byte = ((product + 127) / 255) as u8;
                        patch.alpha[i] = byte;
                    }
                }
            }
            Some((
                patch.x - sx,
                patch.y - sy,
                patch.width,
                patch.height,
                patch.alpha,
            ))
        })?;
        let slot = placed(slot);
        Some((MaskRef::from(slot), rect_of(slot)))
    }

    /// The mask for `clip`'s shapes over `region`. `None` only when the atlas
    /// is full; an empty region never reaches here.
    fn clip_mask(&mut self, clip: &Clip, region: PixelRect) -> Option<MaskRef> {
        let key = MaskKey::of(&ClipMaskKey(clip, region));
        let seam = &self.seam;
        self.masks
            .get_or_insert(key, || {
                let patch = seam.clip_mask(clip, region.to_rect())?;
                Some((patch.x, patch.y, patch.width, patch.height, patch.alpha))
            })
            .map(MaskRef::from)
    }

    fn plan_shadow(
        &mut self,
        rect: Rect,
        radius: f32,
        shadow: &vieww_foundation::Shadow,
        transform: Transform,
        clip: &Clip,
        local_surface: Rect,
    ) -> ShadowPlan {
        let placement = self
            .seam
            .shadow_placement(rect, radius, shadow, transform, local_surface);
        // Keyed by what the coverage depends on — the shape, the blur, the
        // sub-pixel phase — and not where it is: the same card one scroll
        // step on, or the next card down, is a hit.
        let coverage_key = {
            let mut h = std::hash::DefaultHasher::new();
            ("shadow-coverage", placement.key).hash(&mut h);
            MaskKey(h.finish())
        };
        let slot = match self.masks.get(coverage_key) {
            Some(slot) => slot,
            None => {
                let Some(coverage) = self.seam.shadow_coverage(&placement, radius, shadow) else {
                    return ShadowPlan::Nothing;
                };
                match self.masks.get_or_insert(coverage_key, || {
                    Some((
                        coverage.x,
                        coverage.y,
                        coverage.width,
                        coverage.height,
                        coverage.alpha,
                    ))
                }) {
                    Some(slot) => slot,
                    None => return ShadowPlan::Gap,
                }
            }
        };
        // The slot's device position is the canonical one; the placement's
        // whole-pixel shift puts it where this shadow is.
        let (sx, sy) = placement.shift;
        let x0 = slot.device_x + sx;
        let y0 = slot.device_y + sy;
        #[expect(clippy::cast_possible_wrap, reason = "mask extents fit a frame")]
        let patch = PixelRect {
            x0,
            y0,
            x1: x0 + slot.width as i32,
            y1: y0 + slot.height as i32,
        };
        // `NativeRenderer` composites the patch through the clip's *mask* only:
        // a rectangle-only clip does not cut a shadow (the patch is confined to
        // the enclosing buffer instead). Mirrored exactly — this planner's job
        // is parity with the oracle, and a change to that rule belongs in the
        // oracle first.
        // The coverage is the shadow's whole extent, which can reach past
        // the surface by the blur's reach (the CPU renderer cuts it where it
        // draws it, as this does).
        let (scissor, coverage, patch) = if clip.shapes().is_empty() {
            let visible = patch.intersect(PixelRect::round_out(local_surface));
            if visible.is_empty() {
                return ShadowPlan::Nothing;
            }
            #[expect(clippy::cast_precision_loss, reason = "texel coordinates < 2^24")]
            let plain = MaskRef {
                offset: [
                    (x0 - slot.atlas_x as i32) as f32,
                    (y0 - slot.atlas_y as i32) as f32,
                ],
            };
            (visible, plain, patch)
        } else {
            let Some(region) = clip_region(clip, patch, local_surface) else {
                return ShadowPlan::Nothing;
            };
            // **The clip, folded into the coverage** — `shape_mask`'s
            // multiply, for a coverage the atlas already holds: the
            // coverage's bytes and the clip's are read out of the atlas
            // (the blur is never recomputed on a miss) and inserted as one
            // mask. One mask where the pass had two is what makes a
            // clipped shadow a quad: a vertex carries a single mask slot.
            // The multiply rounds to the nearest 1/255 where the oracle
            // multiplies exactly in f32 — up to 1/255 apart, inside the
            // parity contract's 2/255.
            let key = {
                let mut h = std::hash::DefaultHasher::new();
                ("shadow-clipped", placement.key, x0, y0).hash(&mut h);
                ClipMaskKey(clip, region).hash(&mut h);
                MaskKey(h.finish())
            };
            let combined = match self.masks.get(key) {
                Some(slot) => slot,
                None => {
                    let Some(clip_ref) = self.clip_mask(clip, region) else {
                        return ShadowPlan::Gap;
                    };
                    // The clip's insert may have compacted or grown the
                    // atlas, which relocates the coverage; its slot is
                    // re-read so the multiply below addresses the texels
                    // the coverage occupies *now*. Device positions survive
                    // a move, atlas positions do not.
                    let Some(coverage) = self.masks.get(coverage_key) else {
                        return ShadowPlan::Gap;
                    };
                    let mut bytes = Vec::with_capacity((region.width() * region.height()) as usize);
                    {
                        let side = self.masks.side() as usize;
                        let texels = self.masks.texels();
                        let coverage_at = [
                            (x0 - coverage.atlas_x as i32) as f32,
                            (y0 - coverage.atlas_y as i32) as f32,
                        ];
                        let read = |offset: [f32; 2], x: i32, y: i32| {
                            #[expect(clippy::cast_possible_truncation, reason = "integral offsets")]
                            let (ox, oy) = (offset[0] as i32, offset[1] as i32);
                            usize::try_from((y - oy) * side as i32 + (x - ox))
                                .ok()
                                .and_then(|i| texels.get(i).copied())
                                .unwrap_or(0)
                        };
                        for y in region.y0..region.y1 {
                            for x in region.x0..region.x1 {
                                let product = u16::from(read(coverage_at, x, y))
                                    * u16::from(read(clip_ref.offset, x, y));
                                #[expect(clippy::cast_possible_truncation, reason = "<= 255")]
                                let byte = ((product + 127) / 255) as u8;
                                bytes.push(byte);
                            }
                        }
                    }
                    match self.masks.get_or_insert(key, || {
                        Some((region.x0, region.y0, region.width(), region.height(), bytes))
                    }) {
                        Some(slot) => slot,
                        None => return ShadowPlan::Gap,
                    }
                }
            };
            #[expect(clippy::cast_possible_wrap, reason = "mask extents fit a frame")]
            let rect = PixelRect {
                x0: combined.device_x,
                y0: combined.device_y,
                x1: combined.device_x + combined.width as i32,
                y1: combined.device_y + combined.height as i32,
            };
            (rect, MaskRef::from(combined), rect)
        };
        let tint = shadow.color;
        let a = f32::from(tint.a) / 255.0;
        let color = [
            f32::from(tint.r) / 255.0 * a,
            f32::from(tint.g) / 255.0 * a,
            f32::from(tint.b) / 255.0 * a,
            a,
        ];
        ShadowPlan::Step(
            Step::Shadow {
                patch,
                scissor,
                coverage,
                color,
            },
            scissor,
        )
    }
}

/// `VIEWW_GPU_SHADOW_PASS=1`: every shadow stays its own post pass rather than
/// joining the geometry run (the A/B switch for the batching above).
fn shadow_as_pass() -> bool {
    static PASS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PASS.get_or_init(|| std::env::var("VIEWW_GPU_SHADOW_PASS").is_ok_and(|v| v.trim() == "1"))
}

/// Emit one fill, stroke, glyph run or image.
fn plan_primitive(
    out: &mut Builder,
    mut planner: Option<&mut Planner>,
    command: &Command,
    target: PixelRect,
    mask: [f32; 4],
) {
    let scissor = target.to_rect();
    match command {
        Command::FillRect {
            rect,
            paint,
            transform,
            ..
        } => {
            if paint.is_invisible() {
                return;
            }
            // A rect under an axis-aligned uniform transform is the one
            // shape whose edges can be covered analytically, and it is also
            // the most-drawn shape in the framework — every panel, card and
            // button background. Under any other transform the tessellated
            // path below is unchanged.
            if !rect.is_empty() && is_axis_aligned_uniform(*transform) {
                let device = device_rect(*rect, *transform);
                if !device.is_empty() {
                    emit_shape_painted(
                        out, planner, device, 0.0, *paint, *rect, *transform, scissor, mask,
                    );
                    return;
                }
            }
            if let Some(p) = planner.as_deref_mut() {
                let shape = SeamShape::Rect(*rect);
                if emit_covered(
                    out,
                    p,
                    shape,
                    *paint,
                    *rect,
                    *transform,
                    command.clip(),
                    target,
                    mask,
                ) {
                    return;
                }
            }
            // The rect is baked into a path under the transform's sub-pixel
            // phase alone, the whole-pixel shift added to the mesh after the
            // lookup — the FillPath site's own convention, for the same
            // reason: a rect that only moved by whole pixels is the same key.
            let (sx, sy) = device_shift(*transform);
            let phase = Transform {
                tx: transform.tx - sx,
                ty: transform.ty - sy,
                ..*transform
            };
            let path = rect_path(*rect, phase);
            let mut mesh = match planner.as_deref_mut() {
                Some(p) => p.tessellation.fill(&path, TOLERANCE),
                None => tessellate_fill(&path, TOLERANCE),
            };
            for p in &mut mesh.positions {
                *p = [p[0] + sx, p[1] + sy];
            }
            emit_painted(
                out,
                planner,
                &mesh.positions,
                &mesh.indices,
                *paint,
                *rect,
                *transform,
                scissor,
                mask,
            );
        }
        Command::FillPath {
            path,
            paint,
            transform,
            ..
        } => {
            if paint.is_invisible() {
                return;
            }
            // A rounded-rect path under an axis-aligned uniform transform:
            // recognised by reconstruction (see `as_rounded_rect`), covered
            // by the SDF rather than by triangles. This is the rounded card
            // that motivated the whole analytic path — the shape a premium
            // interface is made of.
            if is_axis_aligned_uniform(*transform) {
                if let Some((rect, radius)) = as_rounded_rect(path) {
                    if !rect.is_empty() {
                        let device = device_rect(rect, *transform);
                        let scale = transform.a.abs();
                        let radius = (radius * scale).max(0.0);
                        emit_shape_painted(
                            out, planner, device, radius, *paint, rect, *transform, scissor, mask,
                        );
                        return;
                    }
                }
            }
            if let Some(p) = planner.as_deref_mut() {
                let shape = SeamShape::Fill(path);
                let bounds = path.bounds();
                if emit_covered(
                    out,
                    p,
                    shape,
                    *paint,
                    bounds,
                    *transform,
                    command.clip(),
                    target,
                    mask,
                ) {
                    return;
                }
            }
            // The transform is baked into the path before tessellating, so the
            // cache key is the device-space geometry — and the stroke's key
            // stays transform-free because its site tessellates in local space
            // instead. One convention per site, each for the cache's benefit.
            //
            // **Baked without the translation's whole-pixel part, which is
            // added to the mesh after the lookup.** The key is then the linear
            // part and the sub-pixel phase, so a fill moved by whole pixels —
            // a scroll — is the same key at every step and lyon runs once for
            // the whole scroll: the mask key's own trick, for the mesh. A fill
            // that reaches this fallback at all is the rare case, and a rare
            // case that misses on every scroll step is a rare case that keeps
            // paying for itself.
            let (sx, sy) = device_shift(*transform);
            let phase = Transform {
                tx: transform.tx - sx,
                ty: transform.ty - sy,
                ..*transform
            };
            let device_path = transformed(path, phase);
            let mut mesh = match planner.as_deref_mut() {
                Some(p) => p.tessellation.fill(&device_path, TOLERANCE),
                None => tessellate_fill(&device_path, TOLERANCE),
            };
            for p in &mut mesh.positions {
                *p = [p[0] + sx, p[1] + sy];
            }
            emit_painted(
                out,
                planner,
                &mesh.positions,
                &mesh.indices,
                *paint,
                path.bounds(),
                *transform,
                scissor,
                mask,
            );
        }
        Command::StrokePath {
            path,
            stroke,
            paint,
            transform,
            ..
        } => {
            if paint.is_invisible() || stroke.is_invisible() {
                return;
            }
            // **In local space, then transformed — as the CPU stroker does.**
            // Tessellating the already-transformed path used the stroke width
            // in *device* units, so under any scale (a 2x display, a zoomed
            // canvas) the GPU stroke came out a different width from the
            // CPU's; and dashes were not applied at all. Both are silent
            // wrongness, not gaps. The dash split is the CPU renderer's own
            // (`GpuSeam`'s `dashed_path`), in the same space it measures in.
            // `NativeRenderer`'s gradient box for a stroke, so a gradient
            // stroke maps its ramp over the same unit square.
            let bounds = path
                .bounds()
                .inflate(stroke.reach().max(stroke.width * 0.5 + 1.0));
            if let Some(p) = planner.as_deref_mut() {
                let shape = SeamShape::Stroke(path, stroke);
                if emit_covered(
                    out,
                    p,
                    shape,
                    *paint,
                    bounds,
                    *transform,
                    command.clip(),
                    target,
                    mask,
                ) {
                    return;
                }
            }
            let dashed = vieww_paint::native::dashed_path(path, &stroke.style.dash);
            let local = dashed.as_ref().unwrap_or(path);
            let scale = (transform.a * transform.d - transform.b * transform.c)
                .abs()
                .sqrt()
                .max(1e-3);
            // **Local space is what makes this a cache hit under a scroll**: the
            // key is the path and the stroke in the space the stroker works in,
            // with no transform folded into either, so the same sparkline
            // moving eight pixels a frame is the same key every frame and lyon
            // runs once for the whole scroll.
            let mut mesh = match planner.as_deref_mut() {
                Some(p) => p.tessellation.stroke(local, stroke, TOLERANCE / scale),
                None => tessellate_stroke(local, stroke, TOLERANCE / scale),
            };
            for p in &mut mesh.positions {
                let d = transform.apply(Offset::new(p[0], p[1]));
                *p = [d.dx, d.dy];
            }
            emit_painted(
                out,
                planner,
                &mesh.positions,
                &mesh.indices,
                *paint,
                bounds,
                *transform,
                scissor,
                mask,
            );
        }
        Command::DrawGlyphs { run, transform, .. } => match planner {
            Some(planner) => planner.plan_glyphs(out, run, *transform, scissor, mask),
            None => out.plan.note(Unsupported::Glyphs),
        },
        Command::DrawImage {
            rect,
            image,
            transform,
            ..
        } => match planner {
            Some(planner) => planner.plan_image(out, image, *rect, *transform, scissor, mask),
            None => out.plan.note(Unsupported::Image),
        },
        Command::PushLayer { .. } | Command::PopLayer | Command::DrawShadow { .. } => {
            unreachable!("handled by plan_with");
        }
    }
}

/// A non-analytic shape drawn as one quad over its inked pixels, with the
/// CPU renderer's own coverage — clip included — in the mask atlas. This is
/// what antialiases a rotated rect, an arbitrary path and every stroke the
/// way `NativeRenderer` does: the edge *is* the CPU's edge, not a triangle
/// rasteriser's. `false` when the mask could not be made (see
/// [`Planner::shape_mask`]); the caller tessellates instead.
#[allow(clippy::too_many_arguments)]
fn emit_covered(
    out: &mut Builder,
    planner: &mut Planner,
    shape: SeamShape<'_>,
    paint: Paint,
    local_bounds: Rect,
    transform: Transform,
    clip: &Clip,
    target: PixelRect,
    mask: [f32; 4],
) -> bool {
    let clip_mask = (mask[2] > 0.5).then_some([mask[0], mask[1]]);
    let Some((coverage, inked)) = planner.shape_mask(shape, transform, target, clip, clip_mask)
    else {
        return false;
    };
    #[expect(clippy::cast_precision_loss, reason = "device pixels")]
    let (x0, y0, x1, y1) = (
        inked.x0 as f32,
        inked.y0 as f32,
        inked.x1 as f32,
        inked.y1 as f32,
    );
    let quad = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    emit_painted(
        out,
        Some(planner),
        &quad,
        &[0, 1, 2, 0, 2, 3],
        paint,
        local_bounds,
        transform,
        target.to_rect(),
        [coverage.offset[0], coverage.offset[1], 1.0, 0.0],
    );
    true
}

#[allow(clippy::too_many_arguments)]
fn emit_painted(
    out: &mut Builder,
    planner: Option<&mut Planner>,
    positions: &[[f32; 2]],
    indices: &[u32],
    paint: Paint,
    local_bounds: Rect,
    transform: Transform,
    scissor: Rect,
    mask: [f32; 4],
) {
    if positions.is_empty() || indices.is_empty() {
        return;
    }
    let Some(gradient) = paint.gradient else {
        out.push_mesh(
            DrawConstants::solid(decoded_rgba(paint.color), mask),
            positions.iter().map(|p| (*p, [0.0; 2])),
            indices,
            scissor,
        );
        return;
    };
    let Some(planner) = planner else {
        out.plan.note(Unsupported::Gradient);
        return;
    };
    let Some(inverse) = transform.invert() else {
        // A degenerate transform covers no pixels.
        return;
    };
    let Some(row) = planner.ramps.row(&gradient) else {
        out.plan.note(Unsupported::Gradient);
        return;
    };
    let (params, kind) = gradient_params(gradient.geometry);
    let inv_w = 1.0 / local_bounds.width().max(1e-6);
    let inv_h = 1.0 / local_bounds.height().max(1e-6);
    #[expect(clippy::cast_precision_loss, reason = "ramp rows < 2^24")]
    let extra = [
        row as f32,
        f32::from(u8::from(gradient.dither())),
        kind,
        0.0,
    ];
    let constants = DrawConstants {
        color: [1.0; 4],
        uv_kind: [0.0, 0.0, material::GRADIENT, 0.0],
        params,
        extra,
        shape: [0.0; 4],
        mask,
    };
    out.push_mesh(
        constants,
        positions.iter().map(|p| {
            let local = inverse.apply(Offset::new(p[0], p[1]));
            (
                *p,
                [
                    (local.dx - local_bounds.left) * inv_w,
                    (local.dy - local_bounds.top) * inv_h,
                ],
            )
        }),
        indices,
        scissor,
    );
}

/// A gradient's geometry for the shader, and its kind (0 linear, 1 radial,
/// 2 sweep). Units are the paint's unit square, as `NativeRenderer` uses.
fn gradient_params(geometry: GradientGeometry) -> ([f32; 4], f32) {
    match geometry {
        GradientGeometry::Linear { start, end } => ([start.dx, start.dy, end.dx, end.dy], 0.0),
        GradientGeometry::Radial { center, radius } => ([center.dx, center.dy, radius, 0.0], 1.0),
        GradientGeometry::Sweep {
            center,
            start_angle,
            end_angle,
        } => ([center.dx, center.dy, start_angle, end_angle], 2.0),
    }
}

// ------------------------------------------------------ analytic shape fills
//
// The coverage contract above says every definition comes from the CPU
// renderer through `GpuSeam` — except this one, which is new arithmetic the
// CPU does not have: a signed-distance coverage for rects and rounded rects,
// so geometry edges stop being the one place the GPU is visibly worse than
// the CPU. It replaced nothing: tessellated edges rasterised hard, on every
// shape, everywhere, and `TRACKER.md`'s 2026-10-09 entry records the closure.

/// A transform this module can turn into an analytic shape: axis-aligned, so
/// a rect stays a rect in device space, and uniformly scaled, so a circle's
/// corner stays circular (an anisotropic scale would make it an ellipse, and
/// a rotation makes every edge oblique — both fall back to tessellation,
/// which is what they always had).
fn is_axis_aligned_uniform(transform: Transform) -> bool {
    const EPS: f32 = 1e-9;
    transform.b.abs() < EPS
        && transform.c.abs() < EPS
        && (transform.a.abs() - transform.d.abs()).abs() < EPS
        && transform.a.abs() > EPS
}

/// The device-space rect a `rect` becomes under an axis-aligned `transform`,
/// normalised so `left <= right` and `top <= bottom` (a flip is still a
/// rect).
fn device_rect(rect: Rect, transform: Transform) -> Rect {
    let a = transform.apply(Offset::new(rect.left, rect.top));
    let b = transform.apply(Offset::new(rect.right, rect.bottom));
    Rect::new(
        a.dx.min(b.dx),
        a.dy.min(b.dy),
        a.dx.max(b.dx),
        a.dy.max(b.dy),
    )
}

/// Recognise a path built by [`Path::rounded_rect`](vieww_foundation::Path::rounded_rect)
/// — including its radius-zero case, [`Path::rect`](vieww_foundation::Path::rect)
/// — and recover the `(rect, radius)` it was built from.
///
/// Reconstruction, not pattern-matching: the candidate `Path::rounded_rect`
/// for the recovered parameters is compared verb-for-verb and
/// point-for-point, so a path that merely looks similar (a different arc
/// constant, one extra segment, a rounded rect rotated in its own verbs)
/// fails rather than approximating. The reward for the strictness is that
/// the analytic emission can be exact rather than close.
fn as_rounded_rect(path: &Path) -> Option<(Rect, f32)> {
    let bounds = path.bounds();
    if bounds.is_empty() {
        return None;
    }
    // The first verb is the pen at `(left + radius, top)` — the one place
    // the radius is recorded in the geometry.
    let PathVerb::MoveTo(start) = *path.verbs().first()? else {
        return None;
    };
    let radius = (start.dx - bounds.left).max(0.0);
    let candidate = Path::rounded_rect(bounds, radius);
    let verbs = path.verbs();
    let expected = candidate.verbs();
    if verbs.len() != expected.len() {
        return None;
    }
    const EPS: f32 = 1e-3;
    let near = |a: Offset, b: Offset| (a.dx - b.dx).abs() < EPS && (a.dy - b.dy).abs() < EPS;
    for (verb, want) in verbs.iter().zip(expected) {
        match (verb, want) {
            (PathVerb::MoveTo(a), PathVerb::MoveTo(b))
            | (PathVerb::LineTo(a), PathVerb::LineTo(b)) => {
                if !near(*a, *b) {
                    return None;
                }
            }
            (PathVerb::CubicTo(a1, a2, a3), PathVerb::CubicTo(b1, b2, b3)) => {
                if !near(*a1, *b1) || !near(*a2, *b2) || !near(*a3, *b3) {
                    return None;
                }
            }
            (PathVerb::Close, PathVerb::Close) => {}
            _ => return None,
        }
    }
    Some((bounds, radius))
}

/// The signed distance from `p` to a rounded rectangle, in the same units as
/// `p` — negative inside. The WGSL `shape_coverage` is this function, and
/// this is the one that can be tested: the scene tests below assert its
/// properties, and `vieww-hal`'s Vulkan parity suite asserts the *shader's*
/// pixels against it, so the two cannot drift apart silently.
#[must_use]
pub fn shape_distance(p: Offset, rect: Rect, radius: f32) -> f32 {
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let hx = ((rect.right - rect.left) * 0.5 - radius).max(0.0);
    let hy = ((rect.bottom - rect.top) * 0.5 - radius).max(0.0);
    let qx = (p.dx - cx).abs() - hx;
    let qy = (p.dy - cy).abs() - hy;
    let dx = qx.max(0.0);
    let dy = qy.max(0.0);
    // The corner distance plus the radius; for the straight-edge regions the
    // `max` zeroes one axis and this is the plain perpendicular distance.
    (dx * dx + dy * dy).sqrt() + qx.max(qy).min(0.0) - radius
}

/// Emit one analytic shape fill — a rounded rectangle whose coverage the
/// shader computes from [`shape_distance`] instead of from triangles.
///
/// The quad is the rect inflated by one device pixel, which is the whole of
/// the SDF's ramp: coverage is `clamp(0.5 - distance, 0, 1)`, so it is zero
/// one pixel outside the shape, one inside it, and linear between — the same
/// one-pixel band the CPU rasterizer's scanline coverage produces, which is
/// what makes the census's edge comparison meaningful rather than
/// best-effort.
///
/// Solid and gradient paints take the same path here as in
/// [`emit_painted`]: the gradient's per-fragment parameters and ramp row are
/// unchanged — only the coverage multiplier is new, and the shape's rect
/// rides in [`Vertex::shape`] where the tessellated gradient never looks.
#[allow(clippy::too_many_arguments)]
fn emit_shape_painted(
    out: &mut Builder,
    planner: Option<&mut Planner>,
    rect: Rect,
    radius: f32,
    paint: Paint,
    local_bounds: Rect,
    transform: Transform,
    scissor: Rect,
    mask: [f32; 4],
) {
    // One device pixel of ramp either side: the quad the SDF needs to have
    // a fragment in at all.
    let quad = rect.inflate(1.0);
    let shape = [rect.left, rect.top, rect.right, rect.bottom];

    let Some(gradient) = paint.gradient else {
        let constants = DrawConstants {
            color: decoded_rgba(paint.color),
            uv_kind: [radius, 0.0, material::SHAPE_SOLID, 0.0],
            params: [0.0; 4],
            extra: [0.0; 4],
            shape,
            mask,
        };
        out.quad(
            constants,
            [
                ([quad.left, quad.top], [0.0; 2]),
                ([quad.right, quad.top], [0.0; 2]),
                ([quad.right, quad.bottom], [0.0; 2]),
                ([quad.left, quad.bottom], [0.0; 2]),
            ],
            scissor,
        );
        return;
    };

    let Some(planner) = planner else {
        out.plan.note(Unsupported::Gradient);
        return;
    };
    let Some(inverse) = transform.invert() else {
        // A degenerate transform covers no pixels.
        return;
    };
    let Some(row) = planner.ramps.row(&gradient) else {
        out.plan.note(Unsupported::Gradient);
        return;
    };
    let (params, kind) = gradient_params(gradient.geometry);
    let inv_w = 1.0 / local_bounds.width().max(1e-6);
    let inv_h = 1.0 / local_bounds.height().max(1e-6);
    #[expect(clippy::cast_precision_loss, reason = "ramp rows < 2^24")]
    let extra = [
        row as f32,
        f32::from(u8::from(gradient.dither())),
        kind,
        0.0,
    ];
    let corner = |x: f32, y: f32| {
        let local = inverse.apply(Offset::new(x, y));
        (
            [x, y],
            [
                (local.dx - local_bounds.left) * inv_w,
                (local.dy - local_bounds.top) * inv_h,
            ],
        )
    };
    let constants = DrawConstants {
        color: [1.0; 4],
        uv_kind: [radius, 0.0, material::SHAPE_GRADIENT, 0.0],
        params,
        extra,
        shape,
        mask,
    };
    out.quad(
        constants,
        [
            corner(quad.left, quad.top),
            corner(quad.right, quad.top),
            corner(quad.right, quad.bottom),
            corner(quad.left, quad.bottom),
        ],
        scissor,
    );
}

impl Planner {
    /// One glyph run: colour glyphs first, then coverage glyphs.
    fn plan_glyphs(
        &mut self,
        out: &mut Builder,
        run: &vieww_foundation::GlyphRun,
        transform: Transform,
        scissor: Rect,
        mask: [f32; 4],
    ) {
        for glyph in run.glyphs.iter() {
            match self.seam.color_glyph(
                &run.font,
                glyph.id,
                run.origin,
                glyph.offset,
                run.size,
                transform,
            ) {
                Some(ColorGlyphParts::Layers(layers)) => {
                    for (layer_glyph, color) in layers {
                        self.plan_coverage_glyph(
                            out,
                            run,
                            layer_glyph,
                            glyph.offset,
                            color,
                            transform,
                            scissor,
                            mask,
                        );
                    }
                }
                Some(ColorGlyphParts::Bitmap { image, rect }) => {
                    self.plan_image(out, &image, rect, transform, scissor, mask);
                }
                None => {
                    self.plan_coverage_glyph(
                        out,
                        run,
                        glyph.id,
                        glyph.offset,
                        run.color,
                        transform,
                        scissor,
                        mask,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_coverage_glyph(
        &mut self,
        out: &mut Builder,
        run: &vieww_foundation::GlyphRun,
        glyph_id: u16,
        offset: Offset,
        color: Color,
        transform: Transform,
        scissor: Rect,
        mask: [f32; 4],
    ) {
        let inked = match self
            .glyphs
            .glyph(&run.font, glyph_id, run.size, run.origin, offset, transform)
        {
            // No outline, or no pixels at this size: both are complete
            // renderings (a space draws nothing).
            GlyphAlpha::NoOutline | GlyphAlpha::Blank => return,
            GlyphAlpha::Inked(inked) => inked,
        };
        let device = transform.apply(Offset::new(
            run.origin.dx + offset.dx,
            run.origin.dy + offset.dy,
        ));
        let key = AtlasKey {
            font: run.font.id(),
            glyph: glyph_id,
            size: run.size.to_bits(),
            transform: [transform.a, transform.b, transform.c, transform.d].map(f32::to_bits),
            phase: (
                (device.dx - device.dx.floor()).to_bits(),
                (device.dy - device.dy.floor()).to_bits(),
            ),
        };
        let slot = match self.atlas.get(&key) {
            Some(slot) => Some(slot),
            None => {
                let bytes = inked.alpha();
                self.atlas
                    .insert(key, inked.width(), inked.height(), &bytes)
            }
        };
        let Some(slot) = slot else {
            // A word with a letter missing looks like a font bug; report it.
            out.plan.note(Unsupported::Glyphs);
            return;
        };
        #[expect(clippy::cast_precision_loss, reason = "device pixels")]
        let (x0, y0) = (inked.x() as f32, inked.y() as f32);
        #[expect(clippy::cast_precision_loss, reason = "device pixels")]
        let (x1, y1) = (x0 + slot.width as f32, y0 + slot.height as f32);
        #[expect(clippy::cast_precision_loss, reason = "texel coordinates")]
        let fetch = [
            inked.x() as f32 - slot.x as f32,
            inked.y() as f32 - slot.y as f32,
        ];
        let constants = DrawConstants {
            color: decoded_rgba(color),
            uv_kind: [fetch[0], fetch[1], material::GLYPH, 0.0],
            params: [0.0; 4],
            extra: [0.0; 4],
            shape: [0.0; 4],
            mask,
        };
        out.quad(
            constants,
            [
                ([x0, y0], [0.0; 2]),
                ([x1, y0], [0.0; 2]),
                ([x1, y1], [0.0; 2]),
                ([x0, y1], [0.0; 2]),
            ],
            scissor,
        );
    }

    /// An image quad under its transform, sampled per fragment exactly as
    /// `NativeRenderer::paint_image` samples: bilinear on premultiplied texels,
    /// clamped to the image, blended between two mip levels when minified.
    fn plan_image(
        &mut self,
        out: &mut Builder,
        image: &vieww_foundation::Image,
        rect: Rect,
        transform: Transform,
        scissor: Rect,
        mask: [f32; 4],
    ) {
        if rect.width() <= 0.0 || rect.height() <= 0.0 || image.width() == 0 || image.height() == 0
        {
            return;
        }
        let Some(ratio) = vieww_paint::native::minification_ratio(image, rect, transform) else {
            return;
        };
        let patch = |slot: ImageSlot| {
            #[expect(clippy::cast_precision_loss, reason = "texel coordinates")]
            let p = [
                slot.x as f32,
                slot.y as f32,
                slot.width as f32,
                slot.height as f32,
            ];
            p
        };
        let (lower, upper, frac) = if ratio >= 2.0 {
            let chain = self.seam.image_mips(image);
            if chain.is_empty() {
                (image.clone(), image.clone(), 0.0)
            } else {
                #[expect(clippy::cast_precision_loss, reason = "short chains")]
                let max_level = chain.len() as f32;
                let level = ratio.log2().clamp(0.0, max_level).min(max_level);
                let lower = level.floor();
                let frac = level - lower;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "0..=len"
                )]
                let li = lower as usize;
                let lower_image = if li == 0 {
                    image.clone()
                } else {
                    chain[li - 1].clone()
                };
                let upper_image = chain[li.min(chain.len() - 1)].clone();
                (lower_image, upper_image, frac)
            }
        } else {
            (image.clone(), image.clone(), 0.0)
        };
        let Some(lower_slot) = self.images.insert(&lower) else {
            out.plan.note(Unsupported::Image);
            return;
        };
        let upper_slot = if frac > 0.0 {
            match self.images.insert(&upper) {
                Some(slot) => slot,
                None => {
                    out.plan.note(Unsupported::Image);
                    return;
                }
            }
        } else {
            lower_slot
        };
        // `insert` may have grown the atlas; slots keep their texel positions,
        // so re-reading is unnecessary — `x`/`y` are stable.
        let params = patch(lower_slot);
        let extra = patch(upper_slot);
        let mask = [mask[0], mask[1], mask[2], frac];
        let corner = |local: [f32; 2]| {
            let p = transform.apply(Offset::new(
                rect.left + local[0] * rect.width(),
                rect.top + local[1] * rect.height(),
            ));
            ([p.dx, p.dy], local)
        };
        let constants = DrawConstants {
            color: [1.0; 4],
            uv_kind: [0.0, 0.0, material::IMAGE, 0.0],
            params,
            extra,
            shape: [0.0; 4],
            mask,
        };
        out.quad(
            constants,
            [
                corner([0.0, 0.0]),
                corner([1.0, 0.0]),
                corner([1.0, 1.0]),
                corner([0.0, 1.0]),
            ],
            scissor,
        );
    }
}

/// Curve flattening tolerance, in device pixels. See `tessellate`.
const TOLERANCE: f32 = 0.25;

/// Straight-alpha RGBA in 0..1, as the f32s the shader's arithmetic wants:
/// `k/255` per channel, decoded here exactly as every backend's UNORM8
/// vertex attribute used to decode it — the constants table carries the
/// numbers directly now, and they are the numbers that were computed then.
fn decoded_rgba(color: Color) -> [f32; 4] {
    [
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        f32::from(color.a) / 255.0,
    ]
}

/// A rectangle as a closed path, with the command's transform applied.
fn rect_path(rect: Rect, transform: Transform) -> Path {
    let mut path = Path::new();
    path.move_to(transform.apply(Offset::new(rect.left, rect.top)));
    path.line_to(transform.apply(Offset::new(rect.right, rect.top)));
    path.line_to(transform.apply(Offset::new(rect.right, rect.bottom)));
    path.line_to(transform.apply(Offset::new(rect.left, rect.bottom)));
    path.close();
    path
}

/// A transform's translation, as its whole-pixel part: the part a mesh
/// tessellated without it is the same mesh at, moved. `(0.0, 0.0)` when the
/// translation is not a small finite number, where no shift is worth keying
/// around.
///
/// The fill sites bake `transform - shift` into the path before the
/// tessellation cache, and add `shift` to the mesh's positions after it, so
/// the key holds the linear part and the sub-pixel phase alone — a fill
/// scrolled by whole pixels is the same key at every step. The mask key
/// ([`Planner::shape_mask`]) splits the translation the same way, for the
/// same reason.
fn device_shift(transform: Transform) -> (f32, f32) {
    let (tx, ty) = (transform.tx.floor(), transform.ty.floor());
    if tx.abs() < 1.0e6 && ty.abs() < 1.0e6 {
        (tx, ty)
    } else {
        (0.0, 0.0)
    }
}

/// `path` with `transform` applied to every point.
fn transformed(path: &Path, transform: Transform) -> Path {
    if transform.is_identity() {
        return path.clone();
    }
    let mut out = Path::new();
    for verb in path.verbs() {
        match *verb {
            PathVerb::MoveTo(p) => {
                out.move_to(transform.apply(p));
            }
            PathVerb::LineTo(p) => {
                out.line_to(transform.apply(p));
            }
            PathVerb::CubicTo(a, b, c) => {
                out.cubic_to(transform.apply(a), transform.apply(b), transform.apply(c));
            }
            PathVerb::Close => {
                out.close();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_foundation::{Gradient, Image, ImageFilter, Shadow};
    use vieww_paint::Stroke;

    /// The 45-degree rotation the tests below use — clippy's `approx_constant`
    /// is right that spelling the constant out is a worse `0.707…` than the
    /// name is, so it lives here once with the name it deserves.
    const INV_SQRT_2: f32 = std::f32::consts::FRAC_1_SQRT_2;

    fn solid(scene: &mut Scene, rect: Rect, color: Color, clip: Clip) {
        scene.push_command(Command::FillRect {
            rect,
            paint: Paint::solid(color),
            transform: Transform::IDENTITY,
            clip,
        });
    }

    fn opaque() -> Color {
        Color::rgba(10, 20, 30, 255)
    }

    fn layer(scene: &mut Scene, alpha: f32, blend: BlendMode, filter: ImageFilter, clip: Clip) {
        scene.push_command(Command::PushLayer {
            bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
            alpha,
            blend,
            clip,
            filter,
        });
    }

    fn rounded_clip() -> Clip {
        let mut clip = Clip::NONE;
        clip.add_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        let mut shape = Path::new();
        shape.move_to(Offset::new(0.0, 0.0));
        shape.cubic_to(
            Offset::new(10.0, 0.0),
            Offset::new(10.0, 10.0),
            Offset::new(0.0, 10.0),
        );
        shape.close();
        clip.add_path(shape);
        clip
    }

    #[test]
    fn an_empty_scene_plans_to_nothing_and_is_complete() {
        let plan = plan(&Scene::default(), 100.0, 100.0);
        assert!(plan.is_complete());
        assert_eq!(plan.draw_call_count(), 0);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn a_rectangle_becomes_two_triangles_in_one_draw_step() {
        let mut scene = Scene::default();
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );
        let plan = plan(&scene, 100.0, 100.0);
        assert_eq!(plan.triangle_count(), 2);
        assert_eq!(plan.draw_call_count(), 1);
        assert_eq!(
            plan.steps,
            vec![Step::Draw {
                first_run: 0,
                run_count: 1
            }]
        );
    }

    // ------------------------------------------------ analytic shape emission

    /// The one fill a whole interface is mostly made of: a rect, planned as
    /// the analytic shape rather than as tessellated triangles.
    #[test]
    fn a_rect_fill_is_an_analytic_shape_not_a_tessellation() {
        let mut scene = Scene::default();
        solid(
            &mut scene,
            Rect::new(10.0, 20.0, 30.0, 40.0),
            opaque(),
            Clip::NONE,
        );
        let plan = plan(&scene, 100.0, 100.0);

        assert_eq!(plan.triangle_count(), 2, "one quad, no tessellation");
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).uv_kind[2] == material::SHAPE_SOLID));
        // The shape the shader measures against is the rect itself…
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).shape == [10.0, 20.0, 30.0, 40.0]));
        // …and the quad carrying it is that rect inflated by the one-pixel
        // ramp, because outside that the SDF's coverage is identically zero
        // and a fragment there would be wasted.
        let xs: [f32; 4] = std::array::from_fn(|i| plan.vertices[i].position[0]);
        let ys: [f32; 4] = std::array::from_fn(|i| plan.vertices[i].position[1]);
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), 9.0);
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 31.0);
        assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), 19.0);
        assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), 41.0);
        // A rect has no corners to round.
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).uv_kind[0] == 0.0));
    }

    /// A scale is still a shape — the rect and its (zero) radius are in
    /// device units, and a 2× display doubles both.
    #[test]
    fn a_scaled_rect_scales_the_shape_not_just_the_quad() {
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            paint: Paint::solid(opaque()),
            transform: Transform {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                tx: 5.0,
                ty: 5.0,
            },
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).shape == [5.0, 5.0, 25.0, 25.0]));
    }

    /// The rounded card: the path `Path::rounded_rect` builds is recognised,
    /// and its radius reaches the shader in device units.
    #[test]
    fn a_rounded_rect_fill_is_recognised_and_planned_as_a_shape() {
        let mut scene = Scene::default();
        let rect = Rect::new(10.0, 10.0, 50.0, 40.0);
        scene.push_command(Command::FillPath {
            path: Path::rounded_rect(rect, 6.0),
            paint: Paint::solid(opaque()),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);

        assert_eq!(plan.triangle_count(), 2);
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).uv_kind[2] == material::SHAPE_SOLID));
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).shape == [10.0, 10.0, 50.0, 40.0]));
        assert!(plan
            .vertices
            .iter()
            .all(|v| (plan.draw_constants(v).uv_kind[0] - 6.0).abs() < 1e-6));
    }

    /// `path` rebuilt verb-for-verb through the public builders, with the
    /// first cubic's middle control point nudged by half a pixel — the shape
    /// of a rounded rect with the geometry of one, and not one.
    fn nudged_impostor(path: &Path) -> Path {
        let mut out = Path::new();
        let mut nudged = false;
        for verb in path.verbs() {
            match *verb {
                PathVerb::MoveTo(p) => {
                    out.move_to(p);
                }
                PathVerb::LineTo(p) => {
                    out.line_to(p);
                }
                PathVerb::CubicTo(a, mut b, c) => {
                    if !nudged {
                        b.dx += 0.5;
                        b.dy += 0.5;
                        nudged = true;
                    }
                    out.cubic_to(a, b, c);
                }
                PathVerb::Close => {
                    out.close();
                }
            }
        }
        out
    }

    /// Recognition is reconstruction, not vibes: a path that is *almost* a
    /// rounded rect — one control point nudged — is not one, and takes the
    /// tessellated path every arbitrary path always took.
    #[test]
    fn a_path_that_is_almost_but_not_a_rounded_rect_tessellates() {
        let rect = Rect::new(10.0, 10.0, 50.0, 40.0);
        let impostor = nudged_impostor(&Path::rounded_rect(rect, 6.0));
        let mut scene = Scene::default();
        scene.push_command(Command::FillPath {
            path: impostor,
            paint: Paint::solid(opaque()),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).uv_kind[2] == material::SOLID));
    }

    /// A rotation is not axis-aligned, so the shape falls back — the
    /// tessellated material is still there and still correct for everything
    /// the analytic path cannot express.
    #[test]
    fn a_rotated_rect_falls_back_to_the_tessellated_material() {
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(10.0, 10.0, 30.0, 20.0),
            paint: Paint::solid(opaque()),
            transform: Transform {
                a: INV_SQRT_2,
                b: INV_SQRT_2,
                c: -INV_SQRT_2,
                d: INV_SQRT_2,
                tx: 0.0,
                ty: 0.0,
            },
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).uv_kind[2] == material::SOLID));
    }

    // ------------------------------------------------------------- the SDF

    /// The SDF's shape contract: negative inside, positive outside, zero on
    /// the boundary — and exactly perpendicular distance on the straight
    /// edges, which is what makes the one-pixel coverage ramp agree with the
    /// CPU rasterizer's analytic edge coverage there.
    #[test]
    fn the_signed_distance_is_signed_and_exactly_perpendicular_on_edges() {
        let rect = Rect::new(10.0, 20.0, 50.0, 60.0);
        let radius = 8.0;

        // Deep inside: negative.
        assert!(shape_distance(Offset::new(30.0, 40.0), rect, radius) < -1.0);
        // Deep outside: positive.
        assert!(shape_distance(Offset::new(0.0, 0.0), rect, radius) > 1.0);
        // On a straight edge: exactly zero, and exactly the perpendicular
        // distance just inside and out.
        assert!((shape_distance(Offset::new(30.0, 20.0), rect, radius)).abs() < 1e-6);
        assert!((shape_distance(Offset::new(30.0, 18.0), rect, radius) - 2.0).abs() < 1e-6);
        assert!((shape_distance(Offset::new(30.0, 22.0), rect, radius) + 2.0).abs() < 1e-6);
        // On the straight part of the left edge too.
        assert!((shape_distance(Offset::new(10.0, 40.0), rect, radius)).abs() < 1e-6);
    }

    /// The corner arc is a true quarter circle: a point on the arc's centre
    /// path is at distance `-radius`-ish inside, and the arc centre itself
    /// (the point the radius pivots around) is exactly `radius` inside.
    #[test]
    fn the_corner_distance_is_measured_from_the_arc_centre() {
        let rect = Rect::new(0.0, 0.0, 40.0, 30.0);
        let radius = 5.0;
        // The top-right arc's centre.
        let centre = Offset::new(35.0, 5.0);
        assert!((shape_distance(centre, rect, radius) + radius).abs() < 1e-6);
        // A point on the arc itself: zero.
        let on_arc = Offset::new(35.0 + radius * INV_SQRT_2, 5.0 - radius * INV_SQRT_2);
        assert!(shape_distance(on_arc, rect, radius).abs() < 1e-5);
    }

    /// A radius larger than half the shorter side is clamped by the
    /// constructor; the SDF never sees it. But the SDF's own degenerate
    /// regime — a rect narrower than `2 * radius` — must still behave: the
    /// straight-edge segments vanish and the shape becomes a capsule.
    #[test]
    fn a_radius_larger_than_the_shape_stays_finite_and_monotone() {
        let rect = Rect::new(0.0, 0.0, 10.0, 40.0);
        let radius = 20.0;
        for x in [-5.0, 0.0, 5.0, 10.0, 15.0] {
            for y in [-5.0, 0.0, 20.0, 40.0, 45.0] {
                let d = shape_distance(Offset::new(x, y), rect, radius);
                assert!(d.is_finite(), "({x}, {y})");
            }
        }
        // The mid-line is the deepest inside, either side of it is shallower.
        let mid = shape_distance(Offset::new(5.0, 20.0), rect, radius);
        let off = shape_distance(Offset::new(5.0, 10.0), rect, radius);
        assert!(mid < off);
    }

    // ------------------------------------------------- the rounded-rect probe

    #[test]
    fn rounded_rect_paths_are_recognised_and_impostors_are_not() {
        let rect = Rect::new(0.0, 0.0, 40.0, 30.0);
        assert_eq!(
            as_rounded_rect(&Path::rounded_rect(rect, 6.0)),
            Some((rect, 6.0))
        );
        // Radius zero is Path::rect, and it is recognised too.
        assert_eq!(
            as_rounded_rect(&Path::rect(rect)),
            Some((Rect::new(0.0, 0.0, 40.0, 30.0), 0.0))
        );
        // A circle is a rounded rect taken to its limit: recognised, with
        // the radius the constructor clamped it to.
        assert_eq!(
            as_rounded_rect(&Path::rounded_rect(Rect::new(0.0, 0.0, 40.0, 40.0), 100.0)),
            Some((Rect::new(0.0, 0.0, 40.0, 40.0), 20.0))
        );

        // Not rounded rects: an open path, a triangle, an impostor.
        let open = {
            let source = Path::rounded_rect(rect, 6.0);
            let mut out = Path::new();
            for verb in source.verbs() {
                match *verb {
                    PathVerb::MoveTo(p) => {
                        out.move_to(p);
                    }
                    PathVerb::LineTo(p) => {
                        out.line_to(p);
                    }
                    PathVerb::CubicTo(a, b, c) => {
                        out.cubic_to(a, b, c);
                    }
                    PathVerb::Close => {}
                }
            }
            out
        };
        assert_eq!(as_rounded_rect(&open), None);

        let mut triangle = Path::new();
        triangle.move_to(Offset::new(0.0, 0.0));
        triangle.line_to(Offset::new(10.0, 0.0));
        triangle.line_to(Offset::new(5.0, 10.0));
        triangle.close();
        assert_eq!(as_rounded_rect(&triangle), None);

        let nudged = nudged_impostor(&Path::rounded_rect(rect, 6.0));
        assert_eq!(as_rounded_rect(&nudged), None);
    }

    #[test]
    fn shapes_sharing_a_scissor_collapse_into_one_run() {
        let mut scene = Scene::default();
        for i in 0..20i32 {
            let x = i as f32 * 4.0;
            solid(
                &mut scene,
                Rect::new(x, 0.0, x + 3.0, 3.0),
                opaque(),
                Clip::NONE,
            );
        }
        let plan = plan(&scene, 200.0, 100.0);
        assert_eq!(plan.draw_call_count(), 1, "20 unclipped fills, one draw");
        assert_eq!(plan.triangle_count(), 40);
    }

    #[test]
    fn a_change_of_clip_starts_a_new_run() {
        let mut clipped = Clip::NONE;
        clipped.add_rect(Rect::new(0.0, 0.0, 5.0, 5.0));
        let mut scene = Scene::default();
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        solid(&mut scene, r, opaque(), Clip::NONE);
        solid(&mut scene, r, opaque(), clipped.clone());
        solid(&mut scene, r, opaque(), clipped);
        solid(&mut scene, r, opaque(), Clip::NONE);
        let plan = plan(&scene, 100.0, 100.0);
        assert_eq!(plan.draw_call_count(), 3);
    }

    #[test]
    fn every_run_addresses_indices_that_exist_and_steps_cover_every_run_once() {
        let mut clipped = Clip::NONE;
        clipped.add_rect(Rect::new(2.0, 2.0, 8.0, 8.0));
        let mut scene = Scene::default();
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );
        layer(
            &mut scene,
            0.5,
            BlendMode::Normal,
            ImageFilter::NONE,
            Clip::NONE,
        );
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            clipped,
        );
        scene.push_command(Command::PopLayer);
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );

        let plan = plan(&scene, 100.0, 100.0);
        let mut covered = 0;
        for run in &plan.runs {
            assert_eq!(run.index_count % 3, 0);
            assert!(run.first_index as usize + run.index_count as usize <= plan.indices.len());
            covered += run.index_count as usize;
        }
        assert_eq!(covered, plan.indices.len());
        let mut next = 0;
        for step in &plan.steps {
            if let Step::Draw {
                first_run,
                run_count,
            } = step
            {
                assert_eq!(*first_run, next, "draw steps address runs in order");
                next += run_count;
            }
        }
        assert_eq!(next, plan.runs.len());
    }

    #[test]
    fn a_group_opacity_layer_is_an_offscreen_pass_not_folded_alpha() {
        // Folding the group alpha into each child is wrong wherever children
        // overlap: two opaque squares at 50% group opacity are uniformly 50%,
        // not 75% where they cross. The CPU renderer composites the group.
        let mut scene = Scene::default();
        layer(
            &mut scene,
            0.5,
            BlendMode::Normal,
            ImageFilter::NONE,
            Clip::NONE,
        );
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );
        solid(
            &mut scene,
            Rect::new(5.0, 5.0, 15.0, 15.0),
            opaque(),
            Clip::NONE,
        );
        scene.push_command(Command::PopLayer);

        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete());
        assert_eq!(plan.target_depth, 1);
        assert!(plan
            .vertices
            .iter()
            .all(|v| plan.draw_constants(v).color[3] == 1.0));
        match plan.steps.as_slice() {
            [Step::PushLayer {
                rect,
                backdrop: None,
            }, Step::Draw { .. }, Step::PopLayer {
                alpha,
                composite,
                blend,
                ..
            }] => {
                assert_eq!(
                    *rect,
                    PixelRect {
                        x0: 0,
                        y0: 0,
                        x1: 50,
                        y1: 50
                    }
                );
                assert!((alpha - 0.5).abs() < 1e-6);
                assert_eq!(*blend, BlendMode::Normal);
                assert_eq!(
                    *composite,
                    Some(PixelRect {
                        x0: 0,
                        y0: 0,
                        x1: 15,
                        y1: 15
                    })
                );
            }
            other => panic!("unexpected steps: {other:?}"),
        }
    }

    #[test]
    fn an_opaque_normal_layer_needs_no_target_but_still_confines_its_children() {
        let mut scene = Scene::default();
        layer(
            &mut scene,
            1.0,
            BlendMode::Normal,
            ImageFilter::NONE,
            Clip::NONE,
        );
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 80.0, 80.0),
            opaque(),
            Clip::NONE,
        );
        scene.push_command(Command::PopLayer);
        let plan = plan(&scene, 100.0, 100.0);
        assert_eq!(plan.target_depth, 0);
        assert_eq!(plan.offscreen_step_count(), 0);
        assert_eq!(plan.runs[0].scissor, Some(Rect::new(0.0, 0.0, 50.0, 50.0)));
    }

    #[test]
    fn every_blend_mode_and_filter_is_planned_not_reported() {
        for blend in [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Xor,
            BlendMode::Luminosity,
        ] {
            for filter in [
                ImageFilter::NONE,
                ImageFilter::blur(4.0),
                ImageFilter::backdrop_blur(3.0),
            ] {
                let mut scene = Scene::default();
                layer(&mut scene, 0.8, blend, filter, Clip::NONE);
                solid(
                    &mut scene,
                    Rect::new(10.0, 10.0, 20.0, 20.0),
                    opaque(),
                    Clip::NONE,
                );
                scene.push_command(Command::PopLayer);
                let plan = plan(&scene, 100.0, 100.0);
                assert!(
                    plan.is_complete(),
                    "{blend:?} {filter:?}: {:?}",
                    plan.unsupported
                );
                let pop = plan
                    .steps
                    .iter()
                    .find(|s| matches!(s, Step::PopLayer { .. }))
                    .unwrap();
                let Step::PopLayer {
                    blend: b,
                    filter: f,
                    composite,
                    ..
                } = pop
                else {
                    unreachable!()
                };
                assert_eq!(*b, blend);
                if filter.backdrop {
                    assert!(
                        f.is_none(),
                        "a backdrop filter runs at push, never again at pop"
                    );
                    assert_eq!(
                        *composite,
                        Some(PixelRect {
                            x0: 0,
                            y0: 0,
                            x1: 50,
                            y1: 50
                        })
                    );
                } else if filter.blur_sigma > 0.0 {
                    let r = f.unwrap().box_radius.unwrap() as i32;
                    assert_eq!(
                        *composite,
                        Some(
                            PixelRect {
                                x0: 10 - 3 * r,
                                y0: 10 - 3 * r,
                                x1: 20 + 3 * r,
                                y1: 20 + 3 * r
                            }
                            .intersect(PixelRect {
                                x0: 0,
                                y0: 0,
                                x1: 50,
                                y1: 50
                            })
                        ),
                        "the blur grows the composited region by its reach"
                    );
                }
            }
        }
    }

    #[test]
    fn layers_deeper_than_the_target_limit_are_reported_once_each() {
        let mut scene = Scene::default();
        for _ in 0..(MAX_TARGET_DEPTH + 3) {
            layer(
                &mut scene,
                0.9,
                BlendMode::Normal,
                ImageFilter::NONE,
                Clip::NONE,
            );
        }
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );
        for _ in 0..(MAX_TARGET_DEPTH + 3) {
            scene.push_command(Command::PopLayer);
        }
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            Clip::NONE,
        );
        let plan = plan(&scene, 100.0, 100.0);
        assert_eq!(plan.unsupported.get(&Unsupported::Layer), Some(&1));
        assert!(plan.target_depth < MAX_TARGET_DEPTH);
        assert_eq!(
            plan.runs.len(),
            1,
            "planning resumes after the skipped stack"
        );
    }

    #[test]
    fn a_clipped_rotated_shape_is_one_quad_whose_mask_already_holds_the_clip() {
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(0.0, 0.0, 12.0, 12.0),
            paint: Paint::solid(Color::RED),
            transform: Transform::rotate(0.4),
            clip: rounded_clip(),
        });
        let mut planner = Planner::new();
        let plan = planner.plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        assert_eq!(plan.vertices.len(), 4);
        // The clip's own mask and the shape's (clip multiplied in).
        assert_eq!(planner.mask_atlas().len(), 2);
        // A second frame of the same scene builds nothing new.
        let version = planner.mask_atlas().version();
        let again = planner.plan(&scene, 100.0, 100.0);
        assert_eq!(again.vertices, plan.vertices);
        assert_eq!(planner.mask_atlas().version(), version);
    }

    #[test]
    fn a_shape_too_big_for_a_mask_is_tessellated_rather_than_dropped() {
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(-2000.0, -2000.0, 2000.0, 2000.0),
            paint: Paint::solid(Color::RED),
            transform: Transform::rotate(0.3),
            clip: Clip::NONE,
        });
        let mut planner = Planner::new();
        let plan = planner.plan(&scene, 3000.0, 3000.0);
        assert!(plan.is_complete());
        assert!(!plan.vertices.is_empty());
        assert!(
            plan.vertices
                .iter()
                .all(|v| plan.draw_constants(v).mask[2] == 0.0),
            "tessellated, no mask"
        );
        assert_eq!(planner.mask_atlas().len(), 0);
    }

    #[test]
    fn a_shaped_clip_becomes_a_mask_with_a_planner_and_a_gap_without_one() {
        let mut scene = Scene::default();
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            opaque(),
            rounded_clip(),
        );

        let bare = plan(&scene, 100.0, 100.0);
        assert_eq!(bare.unsupported.get(&Unsupported::ShapedClip), Some(&1));
        assert_eq!(
            bare.triangle_count(),
            0,
            "never widened to its bounding box"
        );

        let mut planner = Planner::new();
        let full = planner.plan(&scene, 100.0, 100.0);
        assert!(full.is_complete(), "{:?}", full.unsupported);
        assert!(full
            .vertices
            .iter()
            .all(|v| full.draw_constants(v).mask[2] == 1.0));
        assert_eq!(planner.mask_atlas().len(), 1);
        // A steady second frame reuses the mask and changes no texels.
        let version = planner.mask_atlas().version();
        let _ = planner.plan(&scene, 100.0, 100.0);
        assert_eq!(planner.mask_atlas().version(), version);
    }

    #[test]
    fn every_shadow_variant_is_a_complete_shadow_step() {
        let variants = [
            (
                Shadow::new(Color::BLACK, Offset::new(0.0, 4.0), 12.0),
                Transform::IDENTITY,
            ),
            (
                Shadow::inset(Color::BLACK, Offset::new(0.0, 4.0), 12.0),
                Transform::IDENTITY,
            ),
            (
                Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 8.0).spread(1.0),
                Transform::rotate(0.4),
            ),
            (
                Shadow::new(Color::BLACK, Offset::ZERO, 6.0),
                Transform::scale(1.5, 0.5),
            ),
        ];
        for (shadow, transform) in variants {
            let mut scene = Scene::default();
            scene.push_command(Command::DrawShadow {
                rect: Rect::new(30.0, 30.0, 60.0, 60.0),
                radius: 6.0,
                shadow,
                transform,
                clip: Clip::NONE,
                covered: false,
            });
            let mut planner = Planner::new();
            let plan = planner.plan(&scene, 100.0, 100.0);
            assert!(plan.is_complete(), "{shadow:?}: {:?}", plan.unsupported);
            // An unclipped shadow joins the geometry run as a masked quad;
            // with `VIEWW_GPU_SHADOW_PASS=1` (or a clip) it is its own step.
            match plan.steps.first() {
                Some(Step::Shadow { patch, scissor, .. }) => {
                    assert!(!patch.is_empty() && !scissor.is_empty(), "{shadow:?}");
                }
                Some(Step::Draw { .. }) => assert!(!plan.vertices.is_empty(), "{shadow:?}"),
                other => panic!("{shadow:?}: {other:?}"),
            }
            assert_eq!(
                plan.target_depth, 0,
                "a shadow draws into the current target, with no work target"
            );
        }
    }

    /// The point of keying shadows by shape: a list of identical cards is
    /// one blurred coverage in the atlas, and scrolling it by whole pixels
    /// adds nothing and uploads nothing.
    #[test]
    fn identical_shadows_share_one_coverage_and_scrolling_keeps_it() {
        let cards = |scroll: f32| {
            let mut scene = Scene::default();
            for row in 0..4 {
                #[expect(clippy::cast_precision_loss, reason = "four rows")]
                // Clear of the surface edge at every scroll below: a shadow
                // the surface crops keeps an absolute key, by design.
                let top = 60.0 + row as f32 * 60.0 - scroll;
                scene.push_command(Command::DrawShadow {
                    rect: Rect::new(20.0, top, 180.0, top + 40.0),
                    radius: 8.0,
                    shadow: Shadow::new(Color::BLACK, Offset::new(0.0, 4.0), 10.0),
                    transform: Transform::IDENTITY,
                    clip: Clip::NONE,
                    covered: false,
                });
            }
            scene
        };
        let mut planner = Planner::new();
        let first = planner.plan(&cards(0.0), 200.0, 400.0);
        assert!(first.is_complete(), "{:?}", first.unsupported);
        // Four unclipped shadows are four quads in one geometry run - not
        // four sealed runs and four shadow passes.
        assert_eq!(
            first
                .steps
                .iter()
                .filter(|s| matches!(s, Step::Shadow { .. }))
                .count(),
            0
        );
        assert_eq!(
            first
                .steps
                .iter()
                .filter(|s| matches!(s, Step::Draw { .. }))
                .count(),
            1,
            "{:?}",
            first.steps
        );
        assert_eq!(planner.mask_atlas().len(), 1, "four cards, one coverage");
        let version = planner.mask_atlas().version();
        for scroll in [6.0, 12.0, 18.0] {
            let plan = planner.plan(&cards(scroll), 200.0, 400.0);
            assert!(plan.is_complete());
            assert_eq!(planner.mask_atlas().len(), 1, "scrolled {scroll}");
        }
        assert_eq!(
            planner.mask_atlas().version(),
            version,
            "nothing re-uploaded"
        );
    }

    /// The clip, folded into the coverage: a card list under a rounded
    /// viewport clip is still one geometry run — every shadow a masked
    /// quad, none a sealed run and a post pass — and the atlas holds the
    /// combined masks (one per distinct clip-to-shadow alignment) where the
    /// pass fetched coverage and clip separately.
    #[test]
    fn clipped_shadows_join_the_geometry_run_with_the_clip_folded_in() {
        let mut clip = Clip::NONE;
        clip.add_path(Path::rounded_rect(
            Rect::new(16.0, 16.0, 184.0, 384.0),
            24.0,
        ));
        let mut cards = Scene::default();
        for row in 0..4u16 {
            let top = 60.0 + f32::from(row) * 60.0;
            cards.push_command(Command::DrawShadow {
                rect: Rect::new(20.0, top, 180.0, top + 40.0),
                radius: 8.0,
                shadow: Shadow::new(Color::BLACK, Offset::new(0.0, 4.0), 10.0),
                transform: Transform::IDENTITY,
                clip: clip.clone(),
                covered: false,
            });
        }
        let mut planner = Planner::new();
        let plan = planner.plan(&cards, 200.0, 400.0);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        assert_eq!(
            plan.steps
                .iter()
                .filter(|s| matches!(s, Step::Shadow { .. }))
                .count(),
            0,
            "a clipped shadow is a quad, not a pass: {:?}",
            plan.steps
        );
        assert_eq!(
            plan.steps
                .iter()
                .filter(|s| matches!(s, Step::Draw { .. }))
                .count(),
            1,
            "the whole list is one geometry run"
        );
        assert!(
            plan.vertices.len() >= 16,
            "four shadow quads' worth of vertices"
        );
        // The combined masks are their own atlas entries, distinct from the
        // clip's own and from the plain coverages (which the folded path
        // never needs).
        assert!(
            planner.mask_atlas().len() >= 4,
            "one combined mask per card: {}",
            planner.mask_atlas().len()
        );
    }

    /// The fold, against the oracle with no GPU: the quad the planner emits
    /// for a clipped shadow — straight colour × the combined mask,
    /// source-over — is within the parity contract's 2/255 of
    /// `NativeRenderer`'s own frame, pixel for pixel. The mask bytes
    /// (coverage × clip, rounded to the nearest 1/255), the fetch offset
    /// and the scissor all have to be right at once for this to pass; the
    /// hardware suites then only re-witness it through a real pipeline.
    #[test]
    fn a_clipped_shadow_quad_replays_within_the_parity_contract() {
        let tint = Color::rgba(10, 20, 40, 140);
        let mut clip = Clip::NONE;
        clip.add_path(Path::rounded_rect(
            Rect::new(16.0, 16.0, 184.0, 144.0),
            20.0,
        ));
        let mut scene = Scene::default();
        scene.push_command(Command::DrawShadow {
            rect: Rect::new(40.0, 30.0, 150.0, 110.0),
            radius: 10.0,
            shadow: Shadow::new(tint, Offset::new(0.0, 6.0), 16.0),
            transform: Transform::IDENTITY,
            clip,
            covered: false,
        });
        let (w, h) = (200u32, 160u32);
        let base = Color::rgba(236, 238, 242, 255);
        let want = vieww_paint::native::NativeRenderer::new()
            .render_to_pixels(&scene, w, h, base)
            .expect("the oracle renders")
            .0;
        #[expect(clippy::cast_precision_loss, reason = "small frame")]
        let (fw, fh) = (w as f32, h as f32);
        let mut planner = Planner::new();
        let plan = planner.plan(&scene, fw, fh);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        assert!(
            plan.steps.iter().all(|s| matches!(s, Step::Draw { .. })),
            "the shadow is geometry: {:?}",
            plan.steps
        );
        let run = &plan.runs[0];
        let scissor = run.scissor.unwrap_or_else(|| Rect::new(0.0, 0.0, fw, fh));
        let vertex = plan.vertices.last().expect("the shadow quad");
        let constants = plan.draw_constants(vertex);
        assert!(constants.mask[2] > 0.5, "masked: {:?}", constants.mask);
        #[expect(clippy::cast_possible_truncation, reason = "integral offsets")]
        let (off_x, off_y) = (constants.mask[0] as i32, constants.mask[1] as i32);
        let color = decoded_rgba(tint);
        let side = planner.mask_atlas().side() as usize;
        let texels = planner.mask_atlas().texels();
        let (sx0, sy0) = (scissor.left.floor() as i32, scissor.top.floor() as i32);
        let (sx1, sy1) = (scissor.right.ceil() as i32, scissor.bottom.ceil() as i32);
        let mut worst = 0u8;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let i = (y as usize * w as usize + x as usize) * 4;
                // The base, premultiplied (a flat opaque colour).
                let mut d = [
                    f32::from(base.r) / 255.0,
                    f32::from(base.g) / 255.0,
                    f32::from(base.b) / 255.0,
                    1.0,
                ];
                let inside = x >= sx0 && x < sx1 && y >= sy0 && y < sy1;
                if inside {
                    let m = f32::from(texels[((y - off_y) as usize) * side + (x - off_x) as usize])
                        / 255.0;
                    let a = color[3] * m;
                    let src = [color[0] * a, color[1] * a, color[2] * a, a];
                    for c in 0..4 {
                        d[c] = src[c] + d[c] * (1.0 - a);
                    }
                }
                // Straightened, as the oracle writes it.
                for c in 0..3 {
                    let value = if d[3] <= f32::EPSILON {
                        0.0
                    } else {
                        d[c] / d[3]
                    };
                    let channel = (value * 255.0).round();
                    let got = want.data()[i + c];
                    let diff = (channel as i32 - i32::from(got)).unsigned_abs() as u8;
                    worst = worst.max(diff);
                }
            }
        }
        assert!(
            worst <= 2,
            "the folded shadow must replay within the parity contract: worst {worst}/255"
        );
    }

    #[test]
    fn gradients_are_per_fragment_materials_with_a_ramp_row() {
        for gradient in [
            Gradient::horizontal(),
            Gradient::radial(Offset::new(0.5, 0.5), 0.5),
            Gradient::sweep(Offset::new(0.5, 0.5), 0.0, std::f32::consts::TAU),
        ] {
            let gradient = gradient.with_stops(&[(0.0, Color::RED), (1.0, Color::BLUE)]);
            let mut scene = Scene::default();
            scene.push_command(Command::FillRect {
                rect: Rect::new(10.0, 10.0, 30.0, 20.0),
                paint: Paint::gradient(gradient),
                transform: Transform::IDENTITY,
                clip: Clip::NONE,
            });
            assert_eq!(
                plan(&scene, 100.0, 100.0)
                    .unsupported
                    .get(&Unsupported::Gradient),
                Some(&1)
            );
            let mut planner = Planner::new();
            let plan = planner.plan(&scene, 100.0, 100.0);
            assert!(plan.is_complete());
            // A rect fill is the analytic shape, so its gradient is the shape
            // material: the same per-fragment ramp maths, multiplied by the
            // SDF coverage.
            assert!(plan
                .vertices
                .iter()
                .all(|v| plan.draw_constants(v).uv_kind[2] == material::SHAPE_GRADIENT));
            // The quad is the rect inflated by one device pixel, so its
            // corners' paint coordinates sit just outside the unit square —
            // clamped in the shader, and covered to zero by the SDF.
            let corners: Vec<[f32; 2]> = plan.vertices.iter().map(|v| v.local).collect();
            assert!(
                corners
                    .iter()
                    .all(|c| c[0] < 0.0 || c[0] > 1.0 || (0.0..=1.0).contains(&c[0])),
                "{corners:?}"
            );
            assert!(
                corners.iter().any(|c| c[0] < 0.0) && corners.iter().any(|c| c[0] > 1.0),
                "the inflation reaches outside the paint bounds: {corners:?}"
            );
            assert_eq!(planner.ramp_atlas().len(), 1);

            // The same gradient on a rotated rect cannot be an analytic
            // shape. It is one quad over its inked bounding box, coverage
            // from the CPU rasterizer in the mask atlas, painted with the
            // per-fragment GRADIENT material and one ramp row. The quad's
            // corners lie outside the rotated rect, so their paint
            // coordinates leave the unit square — coverage is zero there.
            let mut scene = Scene::default();
            scene.push_command(Command::FillRect {
                rect: Rect::new(10.0, 10.0, 30.0, 20.0),
                paint: Paint::gradient(gradient),
                transform: Transform {
                    a: INV_SQRT_2,
                    b: INV_SQRT_2,
                    c: -INV_SQRT_2,
                    d: INV_SQRT_2,
                    tx: 50.0,
                    ty: 0.0,
                },
                clip: Clip::NONE,
            });
            let mut planner = Planner::new();
            let plan = planner.plan(&scene, 100.0, 100.0);
            assert!(plan.is_complete());
            assert_eq!(plan.vertices.len(), 4, "one quad");
            assert!(plan.vertices.iter().all(|v| {
                let c = plan.draw_constants(v);
                c.uv_kind[2] == material::GRADIENT && c.mask[2] > 0.5
            }));
            assert_eq!(planner.mask_atlas().len(), 1);
            let locals: Vec<[f32; 2]> = plan.vertices.iter().map(|v| v.local).collect();
            assert!(
                locals
                    .iter()
                    .any(|c| !(0.0..=1.0).contains(&c[0]) || !(0.0..=1.0).contains(&c[1])),
                "{locals:?}"
            );
        }
    }

    #[test]
    fn a_rotated_image_is_a_rotated_quad_not_its_bounding_box() {
        let image = Image::from_rgba8(vec![255; 16], 2, 2);
        let mut scene = Scene::default();
        scene.push_command(Command::DrawImage {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            image,
            transform: Transform::rotate(std::f32::consts::FRAC_PI_4),
            clip: Clip::NONE,
        });
        let mut planner = Planner::new();
        let plan = planner.plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete());
        assert_eq!(plan.vertices.len(), 4);
        let first = plan.vertices[1].position;
        let expected = Transform::rotate(std::f32::consts::FRAC_PI_4).apply(Offset::new(20.0, 0.0));
        assert!((first[0] - expected.dx).abs() < 1e-4 && (first[1] - expected.dy).abs() < 1e-4);
    }

    #[test]
    fn a_minified_image_blends_two_mip_levels() {
        let image = Image::from_rgba8(vec![200; 64 * 64 * 4], 64, 64);
        let mut scene = Scene::default();
        scene.push_command(Command::DrawImage {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            image,
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let mut planner = Planner::new();
        let plan = planner.plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete());
        let v = plan.vertices[0];
        let c = plan.draw_constants(&v);
        assert!(
            c.params[2] < 64.0,
            "the lower level is a mip, not the base: {:?}",
            c.params
        );
        assert!(
            c.mask[3] > 0.0 && c.mask[3] < 1.0,
            "a fractional level blends: {}",
            c.mask[3]
        );
    }

    #[test]
    fn a_clip_that_lets_nothing_through_draws_nothing_and_is_not_a_gap() {
        let mut clip = Clip::NONE;
        clip.add_rect(Rect::new(5.0, 5.0, 5.0, 5.0));
        let mut scene = Scene::default();
        solid(&mut scene, Rect::new(0.0, 0.0, 10.0, 10.0), opaque(), clip);
        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete());
        assert_eq!(plan.triangle_count(), 0);
    }

    #[test]
    fn a_transform_moves_the_geometry_it_is_recorded_with() {
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            paint: Paint::solid(opaque()),
            transform: Transform::translate(Offset::new(30.0, 40.0)),
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);
        // The analytic quad is the rect inflated by the one-pixel AA ramp,
        // so the geometry reaches 29..41 and 39..51 under a (30, 40) move.
        assert!(plan
            .vertices
            .iter()
            .all(|v| (29.0..=41.0).contains(&v.position[0])));
        assert!(plan
            .vertices
            .iter()
            .all(|v| (39.0..=51.0).contains(&v.position[1])));
    }

    /// A shape moved by whole pixels reuses its coverage mask: the scroll
    /// case. The atlas keeps one mask, not one per position, and the quad
    /// lands at the new place.
    #[test]
    fn a_shape_moved_by_whole_pixels_reuses_its_mask() {
        let mut planner = Planner::new();
        let at = |dy: f32| {
            let mut line = Path::new();
            line.move_to(Offset::new(10.3, 10.0));
            line.line_to(Offset::new(40.0, 25.0));
            line.line_to(Offset::new(80.0, 12.0));
            let mut scene = Scene::default();
            scene.push_command(Command::StrokePath {
                path: line,
                stroke: Stroke::new(2.5),
                paint: Paint::solid(opaque()),
                transform: Transform::translate(Offset::new(0.0, dy)),
                clip: Clip::NONE,
            });
            scene
        };
        let first = planner.plan(&at(0.25), 200.0, 200.0);
        let masks = planner.mask_atlas().len();
        assert!(masks > 0, "the stroke must go through a coverage mask");
        let moved = planner.plan(&at(40.25), 200.0, 200.0);
        assert_eq!(
            planner.mask_atlas().len(),
            masks,
            "moved by whole pixels: no new mask"
        );
        let top = |p: &ScenePlan| {
            p.vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::INFINITY, f32::min)
        };
        assert!((top(&moved) - top(&first) - 40.0).abs() < 1e-3);
    }

    /// A fill moved by whole pixels reuses its mesh: the tessellation key
    /// holds the linear part and the sub-pixel phase, not the position — the
    /// mask key's own trick, for lyon's mesh. A fill that reaches the
    /// tessellation fallback is too big for a coverage mask, so this one is.
    #[test]
    fn a_fill_moved_by_whole_pixels_reuses_its_mesh() {
        let mut planner = Planner::new();
        let at = |dy: f32| {
            let mut blob = Path::new();
            blob.move_to(Offset::new(100.0, 100.0));
            blob.line_to(Offset::new(2300.0, 300.0));
            blob.line_to(Offset::new(1200.0, 1300.0));
            blob.close();
            let mut scene = Scene::default();
            scene.push_command(Command::FillPath {
                path: blob,
                paint: Paint::solid(opaque()),
                transform: Transform::translate(Offset::new(0.0, dy)),
                clip: Clip::NONE,
            });
            scene
        };
        let first = planner.plan(&at(0.25), 2400.0, 1400.0);
        let (_, misses, _, _, _) = planner.tessellation_stats();
        assert!(misses > 0, "the oversized fill must have tessellated");
        assert!(first.triangle_count() >= 1);
        let moved = planner.plan(&at(40.25), 2400.0, 1400.0);
        let (_, misses_moved, _, _, _) = planner.tessellation_stats();
        assert_eq!(
            misses_moved, misses,
            "moved by whole pixels: no new tessellation"
        );
        let top = |p: &ScenePlan| {
            p.vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::INFINITY, f32::min)
        };
        assert!((top(&moved) - top(&first) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn a_stroke_produces_geometry_around_a_line_with_no_area() {
        let mut line = Path::new();
        line.move_to(Offset::new(10.0, 10.0));
        line.line_to(Offset::new(90.0, 10.0));
        let mut scene = Scene::default();
        scene.push_command(Command::StrokePath {
            path: line,
            stroke: Stroke::new(4.0),
            paint: Paint::solid(opaque()),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = plan(&scene, 100.0, 100.0);
        assert!(plan.is_complete());
        assert!(plan.triangle_count() >= 2);
    }

    #[test]
    fn the_colour_reaching_the_draws_is_the_straight_paint_colour() {
        let mut scene = Scene::default();
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgba(255, 128, 0, 64),
            Clip::NONE,
        );
        let plan = plan(&scene, 100.0, 100.0);
        for vertex in &plan.vertices {
            // The table's entry is the colour decoded to `k/255` per channel —
            // exactly the f32s the packed UNORM8 vertex colour used to decode
            // to on every backend, which this test used to assert byte-wise.
            assert_eq!(
                plan.draw_constants(vertex).color,
                [255.0 / 255.0, 128.0 / 255.0, 0.0, 64.0 / 255.0]
            );
        }
    }

    #[test]
    fn the_vertex_layout_is_twenty_bytes() {
        // Two positions and a draw index: 20 bytes where there were 27 floats
        // (108), then 23 floats plus a packed colour (96) — the flat payload
        // lives in the constants table now, one entry per draw item instead
        // of one copy per vertex. The tripwire for a layout the four
        // backends' attribute tables describe by offset.
        assert_eq!(std::mem::size_of::<Vertex>(), 5 * 4);
        assert_eq!(std::mem::align_of::<Vertex>(), 4);
        // And the index sits where the tables say: sixteen bytes in, after
        // the position and the paint-space coordinate.
        assert_eq!(
            std::mem::offset_of!(Vertex, draw),
            2 * std::mem::size_of::<[f32; 2]>()
        );
        // The table entry is six aligned vec4s, mirrored field for field by
        // the WGSL struct — any padding here would desynchronise the shader's
        // stride from the upload's.
        assert_eq!(std::mem::size_of::<DrawConstants>(), 6 * 16);
        assert_eq!(std::mem::align_of::<DrawConstants>(), 4);
        assert_eq!(std::mem::offset_of!(DrawConstants, uv_kind), 16);
        assert_eq!(std::mem::offset_of!(DrawConstants, params), 32);
        assert_eq!(std::mem::offset_of!(DrawConstants, extra), 48);
        assert_eq!(std::mem::offset_of!(DrawConstants, shape), 64);
        assert_eq!(std::mem::offset_of!(DrawConstants, mask), 80);
    }
}
