//! The backend-agnostic execution of a [`ScenePlan`]: the traversal that
//! turns a plan into an ordered list of render operations, extracted from
//! `vulkan::scene`'s `execute` so every backend runs the *same* frame.
//!
//! # Why this module exists
//!
//! `vulkan::scene::SceneRenderer` holds two things: Vulkan resource
//! management, and a plan→passes traversal — *which* render passes a frame
//! needs, in what order, with which scissored regions, blend pipelines and
//! push-constant payloads. Only the first part is Vulkan's. The second is
//! pure logic over [`ScenePlan`] and `post.wgsl`'s mode table, and it is the
//! part a Metal or D3D12 port would otherwise have to copy line for line —
//! at which point the three backends execute three gradually diverging
//! definitions of the same frame, and "the GPU renderers agree with the CPU
//! oracle" quietly stops meaning one thing.
//!
//! So the traversal lives here, once, and [`frame_passes`] is the one
//! function that produces it. `vulkan::scene` interprets the result into
//! Vulkan commands; `metal::scene` and `d3d12::scene` interpret the same
//! stream into Metal and D3D12 ones. The interpreter contract is
//! [`SceneOp`]'s docs; the resource contract (target formats, atlas
//! formats, the WGSL sources and their binding conventions) is the
//! [`crate::vulkan::scene`] module doc's, shared verbatim by the ports.
//!
//! # What is here and what is not
//!
//! Everything the three backends must agree on is here: the clip algebra a
//! damaged frame may redraw ([`frame_passes`]' `damage` handling, including
//! the retained-frame guard and the skip of whole untouched layers), the
//! damage-grown band its filter chains run over (`filter_passes`,
//! `chain_margins`), the
//! `post.wgsl` push-constant layout ([`PostPush`]) and mode table
//! ([`mode`]), the 28 blend modes' numbering ([`blend_index`]), the
//! layer/backdrop-filter/shadow pass construction, and the per-op
//! [`PassKind`] a backend's timestamp marks are charged to.
//!
//! Everything backend-owned is not: target allocation and lifetimes, the
//! readback (and its premultiply-straight conversion), present, overlap and
//! the swapchain blit, per-frame timing beyond the kind each op belongs to.
//! A backend marks [`PassKind::Blit`] itself when the op stream ends and it
//! copies the finished frame out — Vulkan's timestamp for it, the ports'
//! CPU-side equivalent.
//!
//! # Verification
//!
//! The traversal is pure logic, so its own tests run anywhere — no GPU, no
//! backend feature — and pin the shapes the parity suites exercise on real
//! hardware: nested layers, damaged frames, skipped layers, backdrop
//! filters, blend snapshots, masks and shadows. The Vulkan parity suites
//! (`tests/vulkan_scene.rs`, `tests/vulkan_compositor.rs`) remain the
//! pixel-level arbiter, on hardware; they run against this traversal since
//! `vulkan::scene::execute` became an interpreter over its output.

use vieww_foundation::{BlendMode, Color, Rect};
use vieww_gpu::{FilterOp, MaskRef, PixelRect, ScenePlan, Step};

/// Which image a pass reads or writes — one frame, one scratch twin per
/// nesting level, and the 1x1 transparent stand-in bound wherever a pass
/// reads nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetId {
    /// `L[n]`: the frame at 0, the nested layer at `n`.
    Level(usize),
    /// `S[n]`: the ping-pong partner of `L[n]`.
    Scratch(usize),
    /// The 1x1 transparent target.
    Dummy,
}

/// The kind of work an op is — the same kinds [`crate::vulkan::GpuPassTimes`]
/// buckets a frame's GPU timestamps into, so every backend's profiler
/// charges the same stream the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    /// Geometry into a target.
    Draw,
    /// Layer begins, masks, composites.
    Layers,
    /// Blurs and colour matrices.
    Filters,
    /// Shadow tints.
    Shadows,
    /// The frame's copy out (marked by the backend, not an op kind).
    Blit,
}

/// Push constants for `post.wgsl`: eight `vec4<f32>`, 128 bytes — the
/// minimum push-constant size every Vulkan implementation guarantees, and
/// (not coincidentally) exactly the `set_fragment_bytes`/root-constant
/// budget the ports bind it through.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
#[repr(C)]
pub struct PostPush {
    /// `x0, y0, x1, y1` of the region `src` is readable in.
    pub region: [f32; 4],
    /// mode, radius, blend mode, alpha.
    pub args: [f32; 4],
    /// Mode-specific parameters: matrix rows, mask offsets, shadow colour.
    pub p: [[f32; 4]; 6],
}

/// `post.wgsl`'s mode table.
pub mod mode {
    pub const COPY: f32 = 0.0;
    pub const BOX_H: f32 = 1.0;
    pub const BOX_V: f32 = 2.0;
    pub const MATRIX: f32 = 3.0;
    pub const MASK: f32 = 4.0;
    pub const BLEND: f32 = 5.0;
    pub const OVER: f32 = 6.0;
    pub const SHADOW: f32 = 7.0;
}

/// `post.wgsl`'s blend-mode numbering: `BlendMode`'s declaration order.
#[must_use]
pub const fn blend_index(mode: BlendMode) -> f32 {
    use BlendMode::*;
    match mode {
        Normal => 0.0,
        Clear => 1.0,
        Src => 2.0,
        Dst => 3.0,
        DstOver => 4.0,
        SrcIn => 5.0,
        DstIn => 6.0,
        SrcOut => 7.0,
        DstOut => 8.0,
        SrcAtop => 9.0,
        DstAtop => 10.0,
        Xor => 11.0,
        Plus => 12.0,
        Multiply => 13.0,
        Screen => 14.0,
        Overlay => 15.0,
        Darken => 16.0,
        Lighten => 17.0,
        ColorDodge => 18.0,
        ColorBurn => 19.0,
        HardLight => 20.0,
        SoftLight => 21.0,
        Difference => 22.0,
        Exclusion => 23.0,
        Hue => 24.0,
        Saturation => 25.0,
        Color => 26.0,
        Luminosity => 27.0,
    }
}

/// One post pass, described backend-neutrally. The backend binds `src` and
/// `aux` as `post.wgsl`'s first two textures (the mask atlas as the third),
/// sets `push` as the immediates block, and draws one full-screen triangle
/// under each scissor of the op's `scissors`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostPass {
    /// Which target the pass writes.
    pub into: TargetId,
    /// Where it writes, before frame-clip intersection.
    pub scissor: PixelRect,
    /// Where `src` is readable; transparent outside.
    pub read: PixelRect,
    /// What it reads.
    pub src: TargetId,
    /// The destination snapshot a blend reads, or [`TargetId::Dummy`].
    pub aux: TargetId,
    /// Whether the pass draws with the premultiplied-over blend pipeline.
    pub over: bool,
    /// The immediates block.
    pub push: PostPush,
}

impl PostPass {
    /// A pass of `mode` writing `rect` of `into`, reading `src`.
    #[must_use]
    pub const fn new(into: TargetId, rect: PixelRect, src: TargetId, mode: f32) -> Self {
        Self {
            into,
            scissor: rect,
            read: rect,
            src,
            aux: TargetId::Dummy,
            over: false,
            push: PostPush {
                region: [0.0; 4],
                args: [mode, 0.0, 0.0, 0.0],
                p: [[0.0; 4]; 6],
            },
        }
    }
}

/// One operation of a frame, in order. A backend keeps one "open target"
/// across the stream — see [`SceneOp::Begin`].
#[allow(
    clippy::large_enum_variant,
    reason = "PostPass carries the whole 128-byte immediates block by value: \
              a frame is tens of ops, built once and consumed once"
)]
#[derive(Clone, Debug, PartialEq)]
pub enum SceneOp {
    /// End the pass open on any other target and begin one on `target`.
    ///
    /// `clear` is the premultiplied colour the target starts as: `Some`
    /// always (re)starts the pass cleared; `None` keeps what the target
    /// holds, beginning a pass only when none is open on it yet. That pair
    /// is `Recorder::begin`/`ensure_open`'s contract, stated once here so
    /// every interpreter implements the same thing.
    ///
    /// `kind` is the [`PassKind`] this begin is charged to — [`PassKind::Layers`]
    /// for a nested layer's, `None` for the frame's own.
    Begin {
        target: TargetId,
        clear: Option<[f32; 4]>,
        kind: Option<PassKind>,
    },
    /// Clear `rects` of the *currently open* target to `color` — a damaged
    /// frame's own regions of `L[0]`, whose rest is kept.
    ClearRegions {
        rects: Vec<PixelRect>,
        color: [f32; 4],
    },
    /// One indexed draw of `plan.indices[first_index ..][.. index_count]`
    /// through the scene pipeline, into `target` (opened on demand — a pass
    /// already open on it is reused), under `scissor`.
    DrawRun {
        target: TargetId,
        first_index: u32,
        index_count: u32,
        scissor: PixelRect,
    },
    /// One post pass, drawn once per scissor. Never emitted with empty
    /// `scissors` — a pass that touches nothing does not even open its
    /// target, exactly as `vulkan::scene`'s `pass` returns early.
    Post {
        pass: PostPass,
        scissors: Vec<PixelRect>,
        /// The kind of work this pass is — a backend's timestamp marks.
        kind: PassKind,
    },
}

/// What a traversal decided a frame redraws, and what the frame target
/// holds once it finishes — the backend stores `retained` only if the frame
/// completes; on error nothing is kept, exactly as `vulkan::scene` clears
/// its `retained` the moment recording begins.
#[derive(Debug, Clone)]
pub struct FrameOps {
    /// The frame, in order.
    pub ops: Vec<SceneOp>,
    /// How much of the frame target the ops redraw.
    pub redrawn: Redrawn,
    /// What `L[0]` holds after the frame: this size, drawn over this clear.
    pub retained: Retained,
    /// The rectangles a damaged frame redraws, when it kept the previous
    /// frame — the backend's own `frame_clip`, should it need it outside the
    /// ops (Vulkan's probe logic does).
    pub frame_clip: Option<Vec<PixelRect>>,
}

/// How much of the frame target a frame redrew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redrawn {
    /// Every pixel: no damage was given, or what the target held could not
    /// be kept (a new size or clear colour, a backdrop filter reaching
    /// outside the damage).
    Full,
    /// Only the damage: this many pixels (each rectangle's area, summed).
    Damage {
        /// Pixels redrawn.
        pixels: u64,
    },
}

impl Redrawn {
    /// Pixels redrawn, given the frame's own size for [`Redrawn::Full`].
    #[must_use]
    pub const fn pixels(self, width: u32, height: u32) -> u64 {
        match self {
            Self::Full => width as u64 * height as u64,
            Self::Damage { pixels } => pixels,
        }
    }
}

/// What a frame target holds between frames: the frame's own finished
/// pixels, at `size`, drawn over `clear`. A damaged frame keeps them only
/// while both still describe the frame being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retained {
    /// Frame size the target holds.
    pub size: (u32, u32),
    /// The clear colour it was drawn over.
    pub clear: Color,
}

/// More damage rectangles than this and the closest ones are merged until
/// this many are left ([`reduce_clip`]): every rectangle repeats each
/// top-level draw call under its own scissor, and past a handful the
/// repeated vertex work costs more than the pixels saved.
const MAX_CLIP_RECTS: usize = 8;

/// Bring a disjoint `clip` down to at most `max` rectangles by merging the
/// pair whose union adds the fewest pixels, again and again, absorbing
/// whatever each union comes to overlap so the result stays disjoint.
///
/// This used to be "more than `max`, so the bounding box of all of them".
/// On a fill-bound phone that is the expensive answer to a cheap question: a
/// live feed's frame is a scrolled-in strip, a rim and three ticking rows'
/// price cells — ten small rectangles spread over the list — and their
/// bounding box is the whole list, so one rectangle too many turned a frame
/// that changed a tenth of the screen into one that redrew all of it.
fn reduce_clip(clip: &mut Vec<PixelRect>, max: usize) {
    let area = |r: PixelRect| i64::from(r.width()) * i64::from(r.height());
    let union = |a: PixelRect, b: PixelRect| PixelRect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    };
    while clip.len() > max.max(1) {
        let mut best: Option<(usize, usize, i64)> = None;
        for i in 0..clip.len() {
            for j in i + 1..clip.len() {
                let waste = area(union(clip[i], clip[j])) - area(clip[i]) - area(clip[j]);
                if best.is_none_or(|(_, _, w)| waste < w) {
                    best = Some((i, j, waste));
                }
            }
        }
        let Some((i, j, _)) = best else { break };
        let b = clip.swap_remove(j);
        let a = clip.swap_remove(i);
        let mut merged = union(a, b);
        // A union can reach rectangles neither half touched; they join it,
        // which only ever shortens the list.
        while let Some(k) = clip.iter().position(|r| !r.intersect(merged).is_empty()) {
            merged = union(merged, clip.swap_remove(k));
        }
        clip.push(merged);
    }
}

/// Turn `plan` into the ordered operations of one frame at `width x height`
/// over `clear`.
///
/// `damage` — in this frame's device pixels — is what differs from the
/// previous frame; the traversal keeps the previous frame's untouched
/// pixels when `retained` says the target still holds that frame (same
/// size, same clear), and redraws everything otherwise. `None` redraws
/// everything, as a first frame does.
#[must_use]
pub fn frame_passes(
    plan: &ScenePlan,
    width: u32,
    height: u32,
    clear: Color,
    damage: Option<&[PixelRect]>,
    retained: Option<Retained>,
) -> FrameOps {
    let width = width.max(1);
    let height = height.max(1);
    let a = f32::from(clear.a) / 255.0;
    let premultiplied_clear = [
        f32::from(clear.r) / 255.0 * a,
        f32::from(clear.g) / 255.0 * a,
        f32::from(clear.b) / 255.0 * a,
        a,
    ];

    let clip = damage.and_then(|d| usable_clip(plan, d, width, height, clear, retained));
    let skip = clip
        .as_deref()
        .map(|c| skippable_layers(plan, c))
        .unwrap_or_default();
    let redrawn = clip.as_ref().map_or(Redrawn::Full, |c| Redrawn::Damage {
        pixels: c
            .iter()
            .map(|r| u64::from(r.width()) * u64::from(r.height()))
            .sum(),
    });

    let frame = PixelRect {
        x0: 0,
        y0: 0,
        x1: width as i32,
        y1: height as i32,
    };

    let mut ops: Vec<SceneOp> = Vec::new();
    match &clip {
        None => ops.push(SceneOp::Begin {
            target: TargetId::Level(0),
            clear: Some(premultiplied_clear),
            kind: None,
        }),
        Some(clip) => {
            ops.push(SceneOp::Begin {
                target: TargetId::Level(0),
                clear: None,
                kind: None,
            });
            if !clip.is_empty() {
                ops.push(SceneOp::ClearRegions {
                    rects: clip.clone(),
                    color: premultiplied_clear,
                });
            }
        }
    }

    // The op-emitting closures need mutable state shared with the traversal
    // (`depth`), so this is one function with helpers, not a struct.
    let mut depth = 0usize;
    let mut index = 0usize;
    // How a damaged frame may bound each of its filter chains, per step —
    // computed only when there is one. An undamaged frame runs every chain
    // over its whole rectangle and reads none of this, which is also what
    // keeps its op stream byte for byte what it always was.
    let margins = clip.is_some().then(|| chain_margins(plan));
    while index < plan.steps.len() {
        let at = index;
        let step = plan.steps[at];
        if let Some(&pop) = skip.get(&at) {
            index = pop + 1;
            continue;
        }
        index += 1;
        match step {
            Step::Draw {
                first_run,
                run_count,
            } => {
                if plan.vertices.is_empty() || plan.indices.is_empty() {
                    continue;
                }
                let clip = if depth == 0 { clip.as_deref() } else { None };
                for run in &plan.runs[first_run..first_run + run_count] {
                    let scissor = run.scissor.map_or(frame, |r| scissor_of(r, width, height));
                    let regions = clipped(scissor, clip);
                    // Under damage, each region draws only the triangles that
                    // reach it. Drawing the whole run under every region's
                    // scissor made a live update that touched eight rows
                    // process every triangle on screen eight times — ~30 ms
                    // of GPU on the Redmi whatever the damage was.
                    let chunks = (clip.is_some() && run.index_count > CULL_CHUNK)
                        .then(|| chunk_bounds(plan, run.first_index, run.index_count));
                    for s in regions {
                        match &chunks {
                            Some(chunks) => {
                                for (first_index, index_count) in spans_touching(chunks, s) {
                                    ops.push(SceneOp::DrawRun {
                                        target: TargetId::Level(depth),
                                        first_index,
                                        index_count,
                                        scissor: s,
                                    });
                                }
                            }
                            None => ops.push(SceneOp::DrawRun {
                                target: TargetId::Level(depth),
                                first_index: run.first_index,
                                index_count: run.index_count,
                                scissor: s,
                            }),
                        }
                    }
                }
            }
            Step::PushLayer { rect, backdrop } => {
                let parent = depth;
                depth += 1;
                ops.push(SceneOp::Begin {
                    target: TargetId::Level(depth),
                    clear: Some([0.0; 4]),
                    kind: Some(PassKind::Layers),
                });
                if let (Some(filter), false) = (backdrop, rect.is_empty()) {
                    post(
                        &mut ops,
                        &clip,
                        frame,
                        PostPass::new(
                            TargetId::Level(depth),
                            rect,
                            TargetId::Level(parent),
                            mode::COPY,
                        ),
                        PassKind::Layers,
                    );
                    filter_passes(
                        &mut ops,
                        &clip,
                        frame,
                        depth,
                        rect,
                        filter,
                        margins.as_ref().map(|m| m[at]),
                        PassKind::Filters,
                    );
                }
            }
            Step::PopLayer {
                rect,
                composite,
                filter,
                mask,
                blend,
                alpha,
            } => {
                let child = depth;
                depth = depth.saturating_sub(1);
                let mut source = TargetId::Level(child);
                if !rect.is_empty() {
                    if let Some(filter) = filter {
                        filter_passes(
                            &mut ops,
                            &clip,
                            frame,
                            child,
                            rect,
                            filter,
                            margins.as_ref().map(|m| m[at]),
                            PassKind::Filters,
                        );
                    }
                    if let Some(MaskRef { offset }) = mask {
                        let mut pass =
                            PostPass::new(TargetId::Scratch(child), rect, source, mode::MASK);
                        pass.push.p[0] = [offset[0], offset[1], 1.0, 0.0];
                        post(&mut ops, &clip, frame, pass, PassKind::Layers);
                        source = TargetId::Scratch(child);
                    }
                }
                let Some(region) = composite else {
                    continue;
                };
                if blend == BlendMode::Normal {
                    let mut pass =
                        PostPass::new(TargetId::Level(depth), region, source, mode::OVER);
                    pass.read = rect;
                    pass.over = true;
                    pass.push.args[3] = alpha;
                    post(&mut ops, &clip, frame, pass, PassKind::Layers);
                } else {
                    post(
                        &mut ops,
                        &clip,
                        frame,
                        PostPass::new(
                            TargetId::Scratch(depth),
                            region,
                            TargetId::Level(depth),
                            mode::COPY,
                        ),
                        PassKind::Layers,
                    );
                    let mut pass =
                        PostPass::new(TargetId::Level(depth), region, source, mode::BLEND);
                    pass.read = rect;
                    pass.aux = TargetId::Scratch(depth);
                    pass.push.args[2] = blend_index(blend);
                    pass.push.args[3] = alpha;
                    post(&mut ops, &clip, frame, pass, PassKind::Layers);
                }
            }
            Step::Shadow {
                patch,
                scissor,
                coverage,
                color,
            } => {
                if patch.is_empty() || scissor.is_empty() {
                    continue;
                }
                // A clipped shadow arrives with the clip folded into its
                // coverage (`plan_shadow`'s multiply), so this pass has one
                // mask where it used to have two — and the quad the planner
                // emits by default has exactly the same one.
                let mut tint = PostPass::new(
                    TargetId::Level(depth),
                    scissor,
                    TargetId::Dummy,
                    mode::SHADOW,
                );
                tint.read = patch;
                tint.over = true;
                tint.push.p[0] = [coverage.offset[0], coverage.offset[1], 1.0, 0.0];
                tint.push.p[2] = color;
                post(&mut ops, &clip, frame, tint, PassKind::Shadows);
            }
        }
    }

    FrameOps {
        ops,
        redrawn,
        retained: Retained {
            size: (width, height),
            clear,
        },
        frame_clip: clip,
    }
}

/// Emit `pass` under its frame-clipped scissors, or not at all when it
/// touches nothing. The clip a pass is held to is the damage clip whenever
/// it writes the frame's own level or scratch — a top-level blend's
/// snapshot reads the frame pixel for pixel, so both must stay inside it.
#[allow(clippy::too_many_arguments)]
fn post(
    ops: &mut Vec<SceneOp>,
    frame_clip: &Option<Vec<PixelRect>>,
    frame: PixelRect,
    mut pass: PostPass,
    kind: PassKind,
) {
    debug_assert!(
        pass.into != pass.src && pass.into != pass.aux,
        "a pass may not read its own target"
    );
    let clip = if matches!(pass.into, TargetId::Level(0) | TargetId::Scratch(0)) {
        frame_clip.as_deref()
    } else {
        None
    };
    let scissors = clipped(pass.scissor.intersect(frame), clip);
    if scissors.is_empty() {
        return;
    }
    pass.push.region = [
        pass.read.x0 as f32,
        pass.read.y0 as f32,
        pass.read.x1 as f32,
        pass.read.y1 as f32,
    ];
    ops.push(SceneOp::Post {
        pass,
        scissors,
        kind,
    });
}

/// A filter over `rect` of `L[level]`, result left in `L[level]`: three
/// horizontal+vertical box passes ping-ponging through `S[level]`, then the
/// colour matrix through the scratch, with `edge = 1` — the nearest-edge
/// clamp `effects::blur` uses, CSS's `duplicate` edge mode for `filter` and
/// `backdrop-filter`.
///
/// `margin` — [`chain_margins`] computes it — says how a damaged frame may
/// bound the chain. `None` (no damage), or a chain whose output a backdrop
/// filter snapshots: the chain runs over its whole `rect`, exactly as an
/// undamaged frame's does. Any other chain runs over each damage rectangle
/// grown by its own reach plus the enclosing chains' ([`damage_band`]),
/// and the pixels come out the same as a full redraw's — by construction,
/// not by tolerance:
///
/// * The chain's *input* is pixel for pixel a full redraw's. A layer raster
///   is cleared and redrawn in whole every damaged frame — draws inside a
///   layer are deliberately unclipped — and a backdrop's seed copy still
///   runs over the whole `rect`, reading a parent that is either redrawn in
///   whole (`L[k >= 1]`) or the frame itself: a top-level backdrop whose
///   rect the damage does not contain is refused outright (`usable_clip`'s
///   guard), and one whose rect it does contain copies only pixels the
///   frame redrew this pass. And when the parent's content holds another
///   chain's output, that chain is `snapshotted`, running its whole rect,
///   so what the seed copies is a full redraw's values there too.
/// * The chain's *own* boundary is the margin. A box pass of radius `r`
///   reads its partner at most `r` away; three rounds of them read at most
///   `3r` away in each direction, and a sample outside `rect` is clamped to
///   `rect`'s edge — the same pixel a full redraw's pass reads there. So the
///   chain's value at a pixel is the full-redraw value exactly when every
///   pixel of `rect` within its own reach is inside the band, and the band
///   is the damage grown by that reach plus the enclosing chains': what the
///   frame can still read of the chain's output — directly under the
///   damage, and through the enclosing chains' filters, wherever their total
///   reach can carry it — lies within `outer` of the damage, which the band
///   covers with the chain's whole reach to spare. No composite, mask or
///   blend reads sideways, so what the band leaves raw outside itself —
///   further than its own reach plus `outer` from every damage rectangle —
///   cannot reach `L[0]` at all.
///
/// The honest caveat: this is the blit route's own retained-frame model. It
/// holds while what `L[0]` holds outside the damage is the previous frame's
/// *finished* picture of this scene — which is what a caller's damage
/// promises and `Retained` checks — and the moment that is not true the
/// caller passes no damage, the traversal clips nothing, and the chain runs
/// over its whole rectangle again. Inside that model the equivalence is
/// exact, not a tolerance: a damaged frame's bytes cannot differ from a
/// full redraw's — outside the damaged band because the promise says the
/// two frames' scenes agree there, inside it because every pixel the frame
/// redraws is computed, per the two bullets above, from values that are a
/// full redraw's own.
#[allow(clippy::too_many_arguments)]
fn filter_passes(
    ops: &mut Vec<SceneOp>,
    frame_clip: &Option<Vec<PixelRect>>,
    frame: PixelRect,
    level: usize,
    rect: PixelRect,
    filter: FilterOp,
    margin: Option<ChainMargin>,
    kind: PassKind,
) {
    // The margin the band must cover: this chain's own blur plus the
    // enclosing chains' — the accumulated reach, the same rule
    // `effects::blur`'s `band_plan` states for one chain (`reach = 3 *
    // radius`, `grow_ink(3 * radius)`).
    let own = filter.box_radius.map_or(0, blur_reach);
    let band = match (frame_clip.as_deref(), margin) {
        // No damage, or a chain a backdrop filter snapshots (see
        // `chain_margins`): the chain runs as an undamaged frame's does,
        // over its whole rectangle.
        (Some(clip), Some(m)) if !m.snapshotted => {
            damage_band(clip, rect, m.outer.saturating_add(own))
        }
        _ => vec![rect],
    };
    if band.is_empty() {
        return;
    }
    if let Some(radius) = filter.box_radius {
        #[allow(clippy::cast_precision_loss)]
        let r = radius as f32;
        let edge = 1.0;
        for _ in 0..3 {
            // Every horizontal pass of a round runs before the first
            // vertical one. A vertical pass reads up to `r` past its band,
            // which growing may have brought into a neighbouring band, and
            // that neighbour's horizontal pass must already have run for
            // the read to see this round's value rather than a stale one.
            for run in &band {
                let mut h = PostPass::new(
                    TargetId::Scratch(level),
                    *run,
                    TargetId::Level(level),
                    mode::BOX_H,
                );
                h.read = rect;
                h.push.args[1] = r;
                h.push.args[2] = edge;
                post(ops, frame_clip, frame, h, kind);
            }
            for run in &band {
                let mut v = PostPass::new(
                    TargetId::Level(level),
                    *run,
                    TargetId::Scratch(level),
                    mode::BOX_V,
                );
                v.read = rect;
                v.push.args[1] = r;
                v.push.args[2] = edge;
                post(ops, frame_clip, frame, v, kind);
            }
        }
    }
    if let Some(m) = filter.color_matrix {
        for run in &band {
            let mut pass = PostPass::new(
                TargetId::Scratch(level),
                *run,
                TargetId::Level(level),
                mode::MATRIX,
            );
            pass.read = rect;
            pass.push.p[0] = [m[0], m[1], m[2], m[3]];
            pass.push.p[1] = [m[5], m[6], m[7], m[8]];
            pass.push.p[2] = [m[10], m[11], m[12], m[13]];
            pass.push.p[3] = [m[15], m[16], m[17], m[18]];
            pass.push.p[4] = [m[4], m[9], m[14], m[19]];
            post(ops, frame_clip, frame, pass, kind);
        }
        for run in &band {
            let mut copy = PostPass::new(
                TargetId::Level(level),
                *run,
                TargetId::Scratch(level),
                mode::COPY,
            );
            copy.read = rect;
            post(ops, frame_clip, frame, copy, kind);
        }
    }
}

/// The reach of a three-pass box blur of radius `r`: `3 * r` pixels in
/// every direction, the reach `effects::blur`'s `band_plan` grows a band by
/// — every sample feeding a pixel further out passed through one of the
/// three windows, so none of them can have contributed.
#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped to a quarter of the coordinate range first, so the cast is lossless"
)]
fn blur_reach(radius: u32) -> i32 {
    // Clamped, never wrapped: an absurd radius still means "everything the
    // rect touches", not a negative margin.
    let reach = 3 * i64::from(radius);
    if reach > i64::from(i32::MAX / 4) {
        i32::MAX / 4
    } else {
        reach as i32
    }
}

/// The rectangles a filter chain runs over on a damaged frame: each damage
/// rectangle grown by `reach` and kept inside the filter's `rect`, with
/// rectangles that growing brought into contact merged into their union so
/// no pixel is drawn twice. The union is the damage dilated by `reach`
/// (dilation distributes over union), so it is a superset of the exact
/// window a chain needs — a bound may round outward, never inward.
fn damage_band(clip: &[PixelRect], rect: PixelRect, reach: i32) -> Vec<PixelRect> {
    let mut band: Vec<PixelRect> = clip
        .iter()
        .map(|c| c.inflate(reach).intersect(rect))
        .filter(|band| !band.is_empty())
        .collect();
    merge_overlapping(&mut band);
    band
}

/// Merge rectangles that overlap — or that merging made overlap — into
/// their union, until none do. Growing damage rectangles can bring ones
/// that were apart into contact; overlapping scissors would draw the same
/// pixels of a pass twice for no gain.
fn merge_overlapping(rects: &mut Vec<PixelRect>) {
    let mut again = true;
    while again {
        again = false;
        'pair: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                if rects[i].intersect(rects[j]).is_empty() {
                    continue;
                }
                let other = rects.swap_remove(j);
                rects[i] = rects[i].union(other);
                again = true;
                break 'pair;
            }
        }
    }
}

/// How a damaged frame may bound one filter chain: how far the damage must
/// grow for it, and whether it may be bounded at all.
#[derive(Clone, Copy)]
struct ChainMargin {
    /// The accumulated blur reach of the chains *enclosing* this chain's
    /// layer — the total reach between its result and the frame.
    outer: i32,
    /// Whether a backdrop filter snapshots this chain's output: such a
    /// chain runs its whole rect, never a band.
    snapshotted: bool,
}

/// One filter chain in the plan, for [`chain_margins`]'s walks: which step
/// runs it, which step pushed its layer, and of which kind it is.
struct ChainStep {
    /// The step that runs the chain: the pop for a pop filter, the push for
    /// a backdrop.
    at: usize,
    /// The step that pushed the chain's layer.
    pushed: usize,
    /// A pop filter (`Step::PopLayer`'s `filter`), not a backdrop.
    pop: bool,
}

/// The [`ChainMargin`] of every filter chain, by its step: the band algebra
/// a damaged frame runs under. Computed only when there is one — an
/// undamaged frame runs every chain over its whole rectangle.
///
/// `outer` is the sum of the open layers' pop-filter reaches over the
/// chain's step. Composites, masks and blends read pixel for pixel, so
/// between a filtered pixel and `L[0]` only an *enclosing* layer's own
/// filter can carry a difference sideways — and a layer's pop filter, not
/// its backdrop, is the one that sits over what is nested inside it: a
/// backdrop runs at the push, before any of the layer's own content, and
/// the planner puts a filter on one side of a layer or the other, never
/// both. A pop chain's own filter never counts at its own step — it runs
/// *before* the layer composites, so past its result only an *enclosing*
/// filter or nothing lies.
///
/// `snapshotted` marks the chains a *backdrop* filter snapshots, which may
/// not be bounded. A bounded chain's output is correct exactly on the
/// damage grown by `outer` — the region its layer's composite and the
/// enclosing chains' kernels can read. But a backdrop's seed copy is a
/// *snapshot*: its own kernel then reads the parent's raster up to its own
/// reach further out, and a bounded chain's raw edge there would ride the
/// backdrop's blur back into the damage. Rather than grow every
/// contributor's band by every later reader's reach, the chain simply runs
/// unbounded — its output is then correct everywhere, whatever reads it.
/// The mark spreads, because an unbounded chain reads its input everywhere:
/// everything nested inside a pop chain's layer is unbounded too, and so is
/// everything a backdrop snapshots — which is what marked it.
fn chain_margins(plan: &ScenePlan) -> Vec<ChainMargin> {
    let steps = plan.steps.len();
    let mut out = vec![
        ChainMargin {
            outer: 0,
            snapshotted: false
        };
        steps
    ];
    // First walk: the chains, in step order, each layer's own pop-filter
    // reach — which sits on the layer's `PopLayer` step, after all of its
    // nesting, the one fact that makes these pre-computed walks rather than
    // state the traversal could keep — and the initial marking. A backdrop
    // chain snapshots the parent's raster as of its push: every chain
    // composited into it since the parent opened, plus the parent's own seed
    // chain, sitting in its raster since the parent's push. A top-level one
    // snapshots the frame, whose mark is the beginning.
    let mut chains: Vec<ChainStep> = Vec::new();
    let mut snapshotted: Vec<bool> = Vec::new();
    let mut own = vec![0i32; steps];
    // Per open layer: its push step, where the chains composited into its
    // raster begin, and its seed chain, if it is a backdrop layer.
    let mut open: Vec<(usize, usize, Option<usize>)> = Vec::new();
    for (at, step) in plan.steps.iter().enumerate() {
        match *step {
            Step::PushLayer { backdrop, .. } => {
                let seed = if backdrop.is_some() {
                    let (mark, parent_seed) = match open.last() {
                        Some(&(_, mark, seed)) => (mark, seed),
                        None => (0, None),
                    };
                    for flag in &mut snapshotted[mark..] {
                        *flag = true;
                    }
                    if let Some(slot) = parent_seed {
                        snapshotted[slot] = true;
                    }
                    let slot = chains.len();
                    chains.push(ChainStep {
                        at,
                        pushed: at,
                        pop: false,
                    });
                    snapshotted.push(false);
                    Some(slot)
                } else {
                    None
                };
                open.push((at, chains.len(), seed));
            }
            Step::PopLayer { filter, .. } => {
                // A backdrop layer's pop carries no filter — the planner
                // puts a filter on one side of a layer or the other — and
                // the margins below depend on that: a layer with both would
                // smear its own backdrop chain's output by a reach no
                // margin accounts for.
                if let Some((pushed, _, seed)) = open.pop() {
                    debug_assert!(
                        filter.is_none() || seed.is_none(),
                        "a layer with both a backdrop and a pop filter breaks \
                         the band margins"
                    );
                    if let Some(filter) = filter {
                        chains.push(ChainStep {
                            at,
                            pushed,
                            pop: true,
                        });
                        snapshotted.push(false);
                        own[pushed] = filter.box_radius.map_or(0, blur_reach);
                    }
                }
            }
            _ => {}
        }
    }
    // The mark spreads: an unbounded chain reads its input everywhere, and
    // for a pop chain that input is its layer's raster — everything nested
    // inside it, the chains whose steps lie in the layer's `[pushed, at)`.
    // Spreading can mark pop chains that have not spread yet, so round and
    // round until nothing changes.
    let mut spread = vec![false; chains.len()];
    loop {
        let mut again = false;
        for slot in 0..chains.len() {
            if !snapshotted[slot] || spread[slot] || !chains[slot].pop {
                continue;
            }
            spread[slot] = true;
            let (push_step, pop_step) = (chains[slot].pushed, chains[slot].at);
            for other in 0..chains.len() {
                if push_step < chains[other].at && chains[other].at < pop_step {
                    snapshotted[other] = true;
                }
            }
            again = true;
        }
        if !again {
            break;
        }
    }
    // The enclosing reach, per chain: the sum of the open layers' own
    // reaches over the chain's step — before the push for a backdrop (its
    // layer adds nothing to what is inside it), after the pop for a pop
    // filter (its own filter is the chain). Saturating, and capped at a
    // quarter of the coordinate range: past any frame's width the band is
    // everything the rect touches either way, and the cap keeps
    // `PixelRect::inflate`'s arithmetic inside `i32` for every coordinate a
    // frame can have.
    let sum = |open: &[i32]| {
        open.iter()
            .fold(0i32, |a, b| a.saturating_add(*b))
            .min(i32::MAX / 4)
    };
    let mut open: Vec<i32> = Vec::new();
    for (at, step) in plan.steps.iter().enumerate() {
        match *step {
            Step::PushLayer { backdrop, .. } => {
                if backdrop.is_some() {
                    out[at].outer = sum(&open);
                }
                open.push(own[at]);
            }
            Step::PopLayer { filter, .. } => {
                open.pop();
                if filter.is_some() {
                    out[at].outer = sum(&open);
                }
            }
            _ => {}
        }
    }
    for (slot, chain) in chains.iter().enumerate() {
        out[chain.at].snapshotted = snapshotted[slot];
    }
    out
}

/// Convert a frame of premultiplied RGBA16F texels into the straight-alpha
/// RGBA8 bytes every backend's readback hands back — `half_to_f32`,
/// unpremultiply, `v * 255 + 0.5`, alpha unchanged.
///
/// The readback conversion is shared for the same reason the traversal is:
/// the target format is part of the contract, so "what the CPU receives for
/// a finished frame" is one definition, not one per backend.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn straight_rgba8_from_rgba16f(pixels: &[u16]) -> Vec<u8> {
    let mut out = vec![0u8; pixels.len()];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |v: f32| (v * 255.0 + 0.5) as u8;
    for (pixel, slot) in pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0)
    {
        let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]].map(half_to_f32);
        let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
        slot.copy_from_slice(&[byte(r * inv), byte(g * inv), byte(b * inv), byte(a)]);
    }
    out
}

/// An IEEE 754 binary16 as an `f32` — the readback conversion's core, and
/// (through it) the only half-float arithmetic any backend needs. Verbatim
/// the arithmetic `vulkan::scene`'s readback always used, so the shared
/// conversion changes nobody's bytes.
#[must_use]
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = f32::from(h & 0x3ff);
    match exponent {
        0 => sign * mantissa * 2f32.powi(-24),
        31 if mantissa == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    }
}

/// An `f32` as IEEE 754 binary16, round-to-nearest-even — the upload
/// counterpart of [`half_to_f32`], for the linear-light textures the 3D
/// pass copies onto the GPU as RGBA16F (so the oracle's `f32` texels and
/// the GPU's `f16` ones are the same numbers to both renderers' sampling,
/// within half-float precision).
#[must_use]
pub fn f32_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = i32::try_from((bits >> 23) & 0xff).unwrap_or(0);
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        // Inf or NaN: keep it being one.
        return sign | 0x7c00 | u16::from(mantissa != 0);
    }
    // Re-bias from f32's 127 to f16's 15, with the f16's 10-bit mantissa.
    let rebased = exponent - 127 + 15;
    if rebased >= 0x1f {
        // Overflow: infinity.
        return sign | 0x7c00;
    }
    if rebased <= 0 {
        // Subnormal (or zero): shift the mantissa right, rounding to
        // nearest even through the guard bits.
        let shift = u32::try_from(14 - rebased).unwrap_or(32);
        if shift >= 32 {
            return sign;
        }
        let kept = mantissa | 0x80_0000; // the implicit one back in
        let half = kept >> shift;
        let rest = kept & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        let rounded = half + u32::from(rest > halfway || (rest == halfway && half & 1 == 1));
        return sign | (rounded as u16);
    }
    let kept = mantissa >> 13;
    let rest = mantissa & 0x1fff;
    let halfway = 0x1000;
    let mut out = ((rebased as u32) << 10) | kept;
    out += u32::from(rest > halfway || (rest == halfway && kept & 1 == 1));
    sign | (out as u16)
}

#[cfg(test)]
mod half_tests {
    use super::*;

    #[test]
    fn halves_round_trip_through_the_values_that_matter() {
        for value in [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            0.25,
            0.003_130_8, // the sRGB knee
            1.0 / 255.0,
            254.0 / 255.0,
            0.999_9,
            3.0,   // albedo + emissive can exceed 1
            100.0, // ...by a lot
            f32::INFINITY,
        ] {
            let back = half_to_f32(f32_to_half(value));
            assert!(
                (value.is_infinite() && back.is_infinite())
                    || (back - value).abs() <= value.abs() * 0.001,
                "{value} -> {back}"
            );
        }
        assert!(half_to_f32(f32_to_half(f32::NAN)).is_nan());
    }
}

/// The damage a frame may actually redraw: `damage` clamped to the frame,
/// or `None` (redraw everything) when the target does not hold the previous
/// frame at this size and clear, or a top-level backdrop filter reaches
/// outside the damage.
fn usable_clip(
    plan: &ScenePlan,
    damage: &[PixelRect],
    width: u32,
    height: u32,
    clear: Color,
    retained: Option<Retained>,
) -> Option<Vec<PixelRect>> {
    let wanted = Retained {
        size: (width, height),
        clear,
    };
    if retained != Some(wanted) {
        return None;
    }
    #[expect(clippy::cast_possible_wrap, reason = "frame extents")]
    let frame = PixelRect {
        x0: 0,
        y0: 0,
        x1: width as i32,
        y1: height as i32,
    };
    // Disjoint, because every rectangle repeats each top-level draw under
    // its own scissor: where two overlapped, a translucent draw — a group
    // opacity, a shadow's tint, a blend — would be applied twice.
    let mut clip = disjoint(
        damage
            .iter()
            .map(|rect| rect.intersect(frame))
            .filter(|rect| !rect.is_empty()),
    );
    reduce_clip(&mut clip, MAX_CLIP_RECTS);
    let mut depth = 0usize;
    for step in &plan.steps {
        match *step {
            Step::PushLayer { rect, backdrop } => {
                if depth == 0
                    && backdrop.is_some()
                    && touches(&clip, rect)
                    && !clip.iter().any(|c| contains(*c, rect))
                {
                    return None;
                }
                depth += 1;
            }
            Step::PopLayer { .. } => depth = depth.saturating_sub(1),
            Step::Draw { .. } | Step::Shadow { .. } => {}
        }
    }
    Some(clip)
}

/// For each top-level `PushLayer` whose composite misses `clip`, the index
/// of its `PopLayer`: a damaged frame skips the whole layer.
fn skippable_layers(
    plan: &ScenePlan,
    clip: &[PixelRect],
) -> std::collections::HashMap<usize, usize> {
    let mut skip = std::collections::HashMap::new();
    let mut open: Vec<usize> = Vec::new();
    for (index, step) in plan.steps.iter().enumerate() {
        match *step {
            Step::PushLayer { .. } => open.push(index),
            Step::PopLayer { composite, .. } => {
                if let Some(push) = open.pop() {
                    if open.is_empty() && !composite.is_some_and(|c| touches(clip, c)) {
                        skip.insert(push, index);
                    }
                }
            }
            Step::Draw { .. } | Step::Shadow { .. } => {}
        }
    }
    skip
}

/// Indices per culling chunk: 32 triangles. Small enough that a chunk's
/// bounds stay near one row's content, large enough that the bounds pass is
/// cheap next to the draw it saves.
const CULL_CHUNK: u32 = 96;

/// A run's indices in chunks of [`CULL_CHUNK`], each with the device-pixel
/// bounds of its triangles' vertices — the pixels any of them can cover.
fn chunk_bounds(
    plan: &ScenePlan,
    first_index: u32,
    index_count: u32,
) -> Vec<(u32, u32, PixelRect)> {
    let mut out = Vec::with_capacity((index_count / CULL_CHUNK + 1) as usize);
    let end = first_index + index_count;
    let mut start = first_index;
    while start < end {
        let count = CULL_CHUNK.min(end - start);
        let (mut x0, mut y0, mut x1, mut y1) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        for &i in &plan.indices[start as usize..(start + count) as usize] {
            if let Some(v) = plan.vertices.get(i as usize) {
                x0 = x0.min(v.position[0]);
                y0 = y0.min(v.position[1]);
                x1 = x1.max(v.position[0]);
                y1 = y1.max(v.position[1]);
            }
        }
        let bounds = if x0 <= x1 && y0 <= y1 {
            // Out a pixel either way: rounding is the rasterizer's, and a
            // chunk dropped wrongly is a hole, a chunk kept wrongly is only
            // a little work.
            PixelRect::round_out(Rect::new(x0 - 1.0, y0 - 1.0, x1 + 1.0, y1 + 1.0))
        } else {
            PixelRect::EMPTY
        };
        out.push((start, count, bounds));
        start += count;
    }
    out
}

/// The index spans of `chunks` whose bounds reach `region`, adjacent chunks
/// merged, in paint order.
fn spans_touching(chunks: &[(u32, u32, PixelRect)], region: PixelRect) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    for &(start, count, bounds) in chunks {
        if bounds.intersect(region).is_empty() {
            continue;
        }
        match spans.last_mut() {
            Some((first, len)) if *first + *len == start => *len += count,
            _ => spans.push((start, count)),
        }
    }
    spans
}

/// A run's `Rect` scissor, clamped to the frame — the floor/ceil/`max(0)`
/// rounding `NativeRenderer` sizes its own buffers with.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "device pixel coordinates: small, non-negative after clamping"
)]
fn scissor_of(rect: Rect, width: u32, height: u32) -> PixelRect {
    let left = (rect.left.floor().max(0.0) as u32).min(width);
    let top = (rect.top.floor().max(0.0) as u32).min(height);
    let right = (rect.right.ceil().max(0.0) as u32).min(width);
    let bottom = (rect.bottom.ceil().max(0.0) as u32).min(height);
    PixelRect {
        x0: left as i32,
        y0: top as i32,
        x1: right as i32,
        y1: bottom as i32,
    }
}

/// `scissor` itself, or its intersection with each of `clip`'s rectangles;
/// empty results dropped either way.
fn clipped(scissor: PixelRect, clip: Option<&[PixelRect]>) -> Vec<PixelRect> {
    let Some(clip) = clip else {
        return if scissor.is_empty() {
            Vec::new()
        } else {
            vec![scissor]
        };
    };
    clip.iter()
        .map(|c| scissor.intersect(*c))
        .filter(|s| !s.is_empty())
        .collect()
}

/// `rects`' union as rectangles that do not overlap: each one minus every
/// rectangle already taken, as up to four bands around the overlap.
pub fn disjoint(rects: impl IntoIterator<Item = PixelRect>) -> Vec<PixelRect> {
    let mut out: Vec<PixelRect> = Vec::new();
    for rect in rects {
        let mut pieces = vec![rect];
        for taken in &out {
            pieces = pieces
                .into_iter()
                .flat_map(|piece| subtract(piece, *taken))
                .collect();
        }
        out.extend(pieces);
    }
    out
}

/// `a` minus `b`: what is left of `a` as up to four non-overlapping bands
/// (above, below, then left and right of the overlap).
fn subtract(a: PixelRect, b: PixelRect) -> Vec<PixelRect> {
    let overlap = a.intersect(b);
    if overlap.is_empty() {
        return vec![a];
    }
    let bands = [
        PixelRect {
            y1: overlap.y0,
            ..a
        },
        PixelRect {
            y0: overlap.y1,
            ..a
        },
        PixelRect {
            y0: overlap.y0,
            y1: overlap.y1,
            x1: overlap.x0,
            ..a
        },
        PixelRect {
            y0: overlap.y0,
            y1: overlap.y1,
            x0: overlap.x1,
            ..a
        },
    ];
    bands.into_iter().filter(|band| !band.is_empty()).collect()
}

/// Whether `rect` overlaps any of `clip`'s rectangles.
fn touches(clip: &[PixelRect], rect: PixelRect) -> bool {
    clip.iter().any(|c| !c.intersect(rect).is_empty())
}

/// Whether `outer` contains all of `inner`.
const fn contains(outer: PixelRect, inner: PixelRect) -> bool {
    outer.x0 <= inner.x0 && outer.y0 <= inner.y0 && outer.x1 >= inner.x1 && outer.y1 >= inner.y1
}

#[cfg(test)]
mod tests {
    //! The traversal is pure logic — every test here runs with no GPU and no
    //! backend feature, which is the point: the shapes the hardware parity
    //! suites exercise are pinned where every platform can see them fail.

    use std::collections::BTreeMap;

    use super::*;
    use vieww_gpu::DrawRun;

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> PixelRect {
        PixelRect { x0, y0, x1, y1 }
    }

    /// A plan of `steps` with one run of `indices`, so a `Draw` step has
    /// something to draw.
    fn plan(steps: Vec<Step>, target_depth: usize) -> ScenePlan {
        ScenePlan {
            vertices: vec![vieww_gpu::Vertex {
                position: [0.0, 0.0],
                color: [255, 0, 0, 255],
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

    const W: u32 = 100;
    const H: u32 = 100;
    const CLEAR: Color = Color::BLACK;

    fn begins(ops: &[SceneOp]) -> Vec<TargetId> {
        ops.iter()
            .filter_map(|op| match op {
                SceneOp::Begin { target, .. } => Some(*target),
                _ => None,
            })
            .collect()
    }

    /// Under damage, a region draws only the triangles that reach it: two
    /// chunks of quads far apart, damage over the second, and the draw
    /// covers the second chunk's indices alone — not the whole run.
    #[test]
    fn a_damaged_frame_draws_only_the_triangles_its_damage_reaches() {
        let v = |x: f32, y: f32| vieww_gpu::Vertex {
            position: [x, y],
            ..plan(vec![], 1).vertices[0]
        };
        let mut p = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            1,
        );
        // 64 triangles near the top, 64 near the bottom.
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        for i in 0..128u32 {
            let y = if i < 64 { 5.0 } else { 90.0 };
            let base = vertices.len() as u32;
            vertices.extend([v(10.0, y), v(20.0, y), v(10.0, y + 4.0)]);
            indices.extend([base, base + 1, base + 2]);
        }
        p.vertices = vertices;
        p.indices = indices;
        p.runs[0].index_count = 384;
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let frame = frame_passes(&p, W, H, CLEAR, Some(&[rect(0, 85, 100, 100)]), retained);
        let draws: Vec<(u32, u32)> = frame
            .ops
            .iter()
            .filter_map(|op| match op {
                SceneOp::DrawRun {
                    first_index,
                    index_count,
                    ..
                } => Some((*first_index, *index_count)),
                _ => None,
            })
            .collect();
        assert_eq!(draws, vec![(192, 192)], "{:?}", frame.ops);
    }

    #[test]
    fn a_flat_frame_is_one_cleared_begin_and_its_draws() {
        let plan = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        assert_eq!(begins(&frame.ops), vec![TargetId::Level(0)]);
        assert_eq!(
            frame.ops[1],
            SceneOp::DrawRun {
                target: TargetId::Level(0),
                first_index: 0,
                index_count: 3,
                scissor: rect(0, 0, 100, 100)
            }
        );
        assert_eq!(frame.redrawn, Redrawn::Full);
        assert_eq!(
            frame.retained,
            Retained {
                size: (W, H),
                clear: CLEAR
            }
        );
    }

    #[test]
    fn a_nested_layer_begins_cleared_and_composites_over() {
        let layer = rect(10, 10, 50, 50);
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
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 0.5,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        assert_eq!(
            begins(&frame.ops),
            vec![TargetId::Level(0), TargetId::Level(1)]
        );
        let SceneOp::Post { pass, .. } = &frame.ops[3] else {
            panic!("the composite is a post pass: {:?}", frame.ops);
        };
        assert_eq!(pass.into, TargetId::Level(0));
        assert_eq!(pass.src, TargetId::Level(1));
        assert_eq!(pass.push.args[0], mode::OVER);
        assert_eq!(pass.push.args[3], 0.5);
        assert!(pass.over);
        // `read` is the layer's rect — transparent outside it.
        assert_eq!(pass.push.region, [10.0, 10.0, 50.0, 50.0]);
    }

    #[test]
    fn a_blend_mode_snapshots_the_destination_first() {
        let layer = rect(0, 0, 100, 100);
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
                    filter: None,
                    mask: None,
                    blend: BlendMode::Multiply,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        let posts: Vec<&PostPass> = frame
            .ops
            .iter()
            .filter_map(|op| match op {
                SceneOp::Post { pass, .. } => Some(pass),
                _ => None,
            })
            .collect();
        assert_eq!(posts.len(), 2, "snapshot then blend");
        assert_eq!(posts[0].into, TargetId::Scratch(0));
        assert_eq!(posts[0].src, TargetId::Level(0));
        assert_eq!(posts[0].push.args[0], mode::COPY);
        assert_eq!(posts[1].into, TargetId::Level(0));
        assert_eq!(posts[1].src, TargetId::Level(1));
        assert_eq!(posts[1].aux, TargetId::Scratch(0));
        assert_eq!(posts[1].push.args[0], mode::BLEND);
        assert_eq!(posts[1].push.args[2], blend_index(BlendMode::Multiply));
    }

    #[test]
    fn a_blur_filter_ping_pongs_three_times_then_returns_home() {
        let layer = rect(0, 0, 100, 100);
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
                    alpha: 1.0,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        let blurs: Vec<&PostPass> = frame
            .ops
            .iter()
            .filter_map(|op| match op {
                SceneOp::Post { pass, .. } if pass.push.args[0] != mode::OVER => Some(pass),
                _ => None,
            })
            .collect();
        assert_eq!(blurs.len(), 6, "three horizontal and three vertical");
        for (i, pass) in blurs.iter().enumerate() {
            let horizontal = i % 2 == 0;
            assert_eq!(
                pass.push.args[0],
                if horizontal { mode::BOX_H } else { mode::BOX_V }
            );
            assert_eq!(pass.push.args[1], 2.0);
            assert_eq!(pass.push.args[2], 1.0, "the duplicate edge clamp");
            assert_eq!(
                pass.into,
                if horizontal {
                    TargetId::Scratch(1)
                } else {
                    TargetId::Level(1)
                }
            );
            // No damage: the chain still covers the whole rect, reads the
            // whole rect.
            assert_eq!(pass.scissor, layer);
            assert_eq!(pass.read, layer);
        }
    }

    #[test]
    fn a_backdrop_filter_seeds_the_layer_with_the_parent() {
        let layer = rect(20, 20, 80, 80);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: layer,
                    backdrop: Some(FilterOp {
                        box_radius: Some(1),
                        color_matrix: None,
                    }),
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: layer,
                    composite: Some(layer),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        let SceneOp::Post { pass, .. } = &frame.ops[2] else {
            panic!(
                "the seed copy precedes the layer's own content: {:?}",
                frame.ops
            );
        };
        assert_eq!(pass.into, TargetId::Level(1));
        assert_eq!(pass.src, TargetId::Level(0));
        assert_eq!(pass.push.args[0], mode::COPY);
    }

    #[test]
    fn a_shadow_is_one_tint_pass_over_arrival_blurred_coverage() {
        let scissor = rect(0, 0, 40, 40);
        let patch = rect(4, 4, 36, 36);
        let plan = plan(
            vec![Step::Shadow {
                patch,
                scissor,
                coverage: MaskRef { offset: [7.0, 9.0] },
                color: [0.1, 0.2, 0.3, 0.5],
            }],
            0,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        let SceneOp::Post { pass, scissors, .. } = &frame.ops[1] else {
            panic!("a shadow is one post pass");
        };
        assert_eq!(pass.into, TargetId::Level(0));
        assert_eq!(pass.push.args[0], mode::SHADOW);
        assert!(pass.over);
        assert_eq!(pass.push.p[0], [7.0, 9.0, 1.0, 0.0]);
        assert_eq!(pass.push.p[2], [0.1, 0.2, 0.3, 0.5]);
        assert_eq!(scissors, &vec![scissor]);
        assert_eq!(pass.push.region, [4.0, 4.0, 36.0, 36.0]);
    }

    #[test]
    fn a_mask_multiplies_the_layer_through_the_scratch() {
        let layer = rect(0, 0, 60, 60);
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
                    filter: None,
                    mask: Some(MaskRef { offset: [3.0, 5.0] }),
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        let SceneOp::Post { pass, .. } = &frame.ops[3] else {
            panic!("the mask is a post pass before the composite");
        };
        assert_eq!(pass.push.args[0], mode::MASK);
        assert_eq!(pass.into, TargetId::Scratch(1));
        assert_eq!(pass.push.p[0], [3.0, 5.0, 1.0, 0.0]);
        // The composite then reads the masked scratch, not the layer.
        let SceneOp::Post { pass, .. } = &frame.ops[4] else {
            panic!("the composite follows");
        };
        assert_eq!(pass.src, TargetId::Scratch(1));
    }

    #[test]
    fn damage_keeps_the_previous_frame_and_clears_only_itself() {
        let plan = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(10, 20, 30, 40);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        assert_eq!(frame.redrawn, Redrawn::Damage { pixels: 20 * 20 });
        assert_eq!(
            frame.ops[0],
            SceneOp::Begin {
                target: TargetId::Level(0),
                clear: None,
                kind: None,
            }
        );
        assert_eq!(
            frame.ops[1],
            SceneOp::ClearRegions {
                rects: vec![damage],
                color: [0.0, 0.0, 0.0, 1.0]
            }
        );
        // The draw is clipped to the damage.
        assert_eq!(
            frame.ops[2],
            SceneOp::DrawRun {
                target: TargetId::Level(0),
                first_index: 0,
                index_count: 3,
                scissor: damage
            }
        );
    }

    #[test]
    fn damage_on_a_retained_frame_of_another_shape_redraws_everything() {
        let plan = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        // A different clear colour: the previous frame cannot be kept.
        let retained = Some(Retained {
            size: (W, H),
            clear: Color::WHITE,
        });
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[rect(0, 0, 1, 1)]), retained);
        assert_eq!(frame.redrawn, Redrawn::Full);
        assert!(matches!(
            frame.ops[0],
            SceneOp::Begin { clear: Some(_), .. }
        ));
        assert!(!frame
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::ClearRegions { .. })));
    }

    #[test]
    fn a_top_level_layer_outside_the_damage_is_skipped_whole() {
        let inside = rect(0, 0, 20, 20);
        let outside = rect(50, 50, 60, 60);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: inside,
                    backdrop: None,
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: inside,
                    composite: Some(inside),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
                Step::PushLayer {
                    rect: outside,
                    backdrop: None,
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: outside,
                    composite: Some(outside),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[inside]), retained);
        // One layer begin: the untouched layer contributed nothing.
        assert_eq!(
            begins(&frame.ops),
            vec![TargetId::Level(0), TargetId::Level(1)],
            "the second layer is skipped whole"
        );
        // And nothing that writes the frame does so outside the damage:
        // depth-0 draws and composites into `L[0]` stay inside it. Draws
        // *inside* a kept layer are deliberately unclipped — the layer's
        // raster is reusable whole; it is the composite that is held to the
        // damage.
        for op in &frame.ops {
            let frame_level = match op {
                SceneOp::DrawRun {
                    target, scissor, ..
                } if *target == TargetId::Level(0) => Some(*scissor),
                SceneOp::Post { pass, scissors, .. }
                    if matches!(pass.into, TargetId::Level(0) | TargetId::Scratch(0)) =>
                {
                    scissors.first().copied()
                }
                _ => None,
            };
            if let Some(scissor) = frame_level {
                assert_eq!(scissor.intersect(inside), scissor);
            }
        }
    }

    #[test]
    fn a_backdrop_filter_reaching_past_the_damage_forces_a_full_frame() {
        let layer = rect(10, 10, 90, 90);
        let plan = plan(
            vec![Step::PushLayer {
                rect: layer,
                backdrop: Some(FilterOp {
                    box_radius: Some(1),
                    color_matrix: None,
                }),
            }],
            1,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(20, 20, 30, 30);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        assert_eq!(frame.redrawn, Redrawn::Full);
    }

    /// A damaged frame runs a layer's blur chain over the damage grown by
    /// the chain's reach — three box passes of radius 2 read 6 pixels away —
    /// not over the layer's whole rectangle, while the composite into
    /// `L[0]` keeps its own clip: the damage alone.
    #[test]
    fn a_damaged_frame_blurs_only_the_damage_grown_by_the_blurs_reach() {
        let layer = rect(10, 10, 90, 90);
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
                    alpha: 1.0,
                },
            ],
            1,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(30, 40, 50, 60);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        let band = damage.inflate(3 * 2).intersect(layer);
        let blurs: Vec<&PostPass> = frame
            .ops
            .iter()
            .filter_map(|op| match op {
                SceneOp::Post { pass, .. }
                    if pass.push.args[0] == mode::BOX_H || pass.push.args[0] == mode::BOX_V =>
                {
                    Some(pass)
                }
                _ => None,
            })
            .collect();
        assert_eq!(blurs.len(), 6, "the chain still ping-pongs three times");
        for pass in blurs {
            assert_eq!(
                pass.scissor, band,
                "the chain runs over the damage grown by 3r"
            );
            assert_eq!(
                pass.read, layer,
                "the edge clamp still reads the whole layer rect"
            );
        }
        let (pass, scissors) = frame
            .ops
            .iter()
            .find_map(|op| match op {
                SceneOp::Post { pass, scissors, .. } if pass.push.args[0] == mode::OVER => {
                    Some((pass, scissors))
                }
                _ => None,
            })
            .expect("the layer composites");
        assert_eq!(pass.src, TargetId::Level(1));
        assert_eq!(
            scissors,
            &vec![damage],
            "the composite is held to the damage"
        );
    }

    /// A blur inside a blur: the inner chain's band grows by the outer
    /// blur's reach as well as its own. Its raw, unblurred edge outside a
    /// per-filter band would ride the outer blur back into the damage — the
    /// accumulated reach is what the margin must cover.
    #[test]
    fn a_blur_inside_a_blur_grows_its_band_by_what_still_encloses_it() {
        let outer_rect = rect(0, 0, 100, 100);
        let inner_rect = rect(20, 20, 60, 60);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: outer_rect,
                    backdrop: None,
                },
                Step::PushLayer {
                    rect: inner_rect,
                    backdrop: None,
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: inner_rect,
                    composite: Some(inner_rect),
                    filter: Some(FilterOp {
                        box_radius: Some(1),
                        color_matrix: None,
                    }),
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
                Step::PopLayer {
                    rect: outer_rect,
                    composite: Some(outer_rect),
                    filter: Some(FilterOp {
                        box_radius: Some(5),
                        color_matrix: None,
                    }),
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            2,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(40, 40, 44, 44);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        let bands = |level: usize| -> Vec<PixelRect> {
            frame
                .ops
                .iter()
                .filter_map(|op| match op {
                    SceneOp::Post { pass, .. }
                        if (pass.push.args[0] == mode::BOX_H
                            || pass.push.args[0] == mode::BOX_V)
                            && (pass.into == TargetId::Scratch(level)
                                || pass.into == TargetId::Level(level)) =>
                    {
                        Some(pass.scissor)
                    }
                    _ => None,
                })
                .collect()
        };
        // The inner chain: radius 1 (reach 3) inside radius 5 (reach 15).
        assert_eq!(
            bands(2),
            vec![damage.inflate(3 + 15).intersect(inner_rect); 6],
            "the inner band grows by both reaches"
        );
        // The outer chain: radius 5 (reach 15), nothing enclosing it.
        assert_eq!(
            bands(1),
            vec![damage.inflate(15).intersect(outer_rect); 6],
            "the outer band grows by its own reach alone"
        );
    }

    /// A nested blurred layer the damage cannot reach runs no filter pass at
    /// all. Its parent's composite touches the damage, so the layer is not
    /// skipped whole — but nothing it filters can influence a damaged pixel,
    /// so the chain has nothing to contribute.
    #[test]
    fn a_filter_the_damage_cannot_reach_runs_not_at_all() {
        let full = rect(0, 0, 100, 100);
        let far = rect(60, 60, 90, 90);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: full,
                    backdrop: None,
                },
                Step::PushLayer {
                    rect: far,
                    backdrop: None,
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: far,
                    composite: Some(far),
                    filter: Some(FilterOp {
                        box_radius: Some(2),
                        color_matrix: None,
                    }),
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
                Step::PopLayer {
                    rect: full,
                    composite: Some(full),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            2,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[rect(0, 0, 20, 20)]), retained);
        assert!(
            !frame.ops.iter().any(|op| match op {
                SceneOp::Post { pass, .. } => {
                    pass.push.args[0] == mode::BOX_H || pass.push.args[0] == mode::BOX_V
                }
                _ => false,
            }),
            "the chain has nothing to contribute: {:?}",
            frame.ops
        );
        // The layer still composites into its parent, unclipped as ever.
        assert!(frame.ops.iter().any(|op| match op {
            SceneOp::Post { pass, .. } => {
                pass.push.args[0] == mode::OVER && pass.into == TargetId::Level(1)
            }
            _ => false,
        }));
    }

    /// A colour matrix has no reach of its own: on a damaged frame it runs
    /// over the damage inside its rect, no more.
    #[test]
    fn a_colour_matrix_runs_only_over_the_damage() {
        let layer = rect(10, 10, 90, 90);
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
                        box_radius: None,
                        color_matrix: Some([1.0; 20]),
                    }),
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(20, 20, 40, 40);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        let matrix = frame
            .ops
            .iter()
            .find_map(|op| match op {
                SceneOp::Post { pass, .. } if pass.push.args[0] == mode::MATRIX => Some(pass),
                _ => None,
            })
            .expect("the matrix pass");
        assert_eq!(matrix.scissor, damage);
        assert_eq!(matrix.read, layer, "the readable region is still the rect");
        let copy = frame
            .ops
            .iter()
            .find_map(|op| match op {
                SceneOp::Post { pass, .. }
                    if pass.push.args[0] == mode::COPY
                        && pass.into == TargetId::Level(1)
                        && pass.src == TargetId::Scratch(1) =>
                {
                    Some(pass)
                }
                _ => None,
            })
            .expect("the copy back");
        assert_eq!(copy.scissor, damage);
        assert_eq!(copy.read, layer);
    }

    /// A damaged frame bounds a backdrop filter's chain but not its seed
    /// copy: the chain reads up to its reach past the band, and the seed is
    /// what it reads there — a parent that is either redrawn in whole, or
    /// refused by the damaged frame outright.
    #[test]
    fn a_backdrop_chain_is_bounded_but_its_seed_copies_the_whole_rect() {
        let outer = rect(0, 0, 100, 100);
        let panel = rect(10, 10, 90, 90);
        let plan = plan(
            vec![
                Step::PushLayer {
                    rect: outer,
                    backdrop: None,
                },
                Step::PushLayer {
                    rect: panel,
                    backdrop: Some(FilterOp {
                        box_radius: Some(2),
                        color_matrix: None,
                    }),
                },
                Step::Draw {
                    first_run: 0,
                    run_count: 1,
                },
                Step::PopLayer {
                    rect: panel,
                    composite: Some(panel),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
                Step::PopLayer {
                    rect: outer,
                    composite: Some(outer),
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            2,
        );
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let damage = rect(30, 30, 50, 50);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[damage]), retained);
        assert_eq!(frame.redrawn, Redrawn::Damage { pixels: 400 });
        let seed = frame
            .ops
            .iter()
            .find_map(|op| match op {
                SceneOp::Post { pass, .. }
                    if pass.push.args[0] == mode::COPY
                        && pass.into == TargetId::Level(2)
                        && pass.src == TargetId::Level(1) =>
                {
                    Some(pass)
                }
                _ => None,
            })
            .expect("the backdrop seed copies the parent");
        assert_eq!(seed.scissor, panel, "the seed still copies the whole rect");
        let band = damage.inflate(3 * 2).intersect(panel);
        for op in &frame.ops {
            if let SceneOp::Post { pass, .. } = op {
                if pass.push.args[0] == mode::BOX_H || pass.push.args[0] == mode::BOX_V {
                    assert_eq!(pass.scissor, band, "the chain runs over the band");
                    assert_eq!(pass.read, panel);
                }
            }
        }
    }

    #[test]
    fn overlapping_damage_is_disjointed_before_it_is_cleared() {
        let plan = plan(vec![], 0);
        let retained = Some(Retained {
            size: (W, H),
            clear: CLEAR,
        });
        let a = rect(0, 0, 30, 30);
        let b = rect(10, 10, 40, 40);
        let frame = frame_passes(&plan, W, H, CLEAR, Some(&[a, b]), retained);
        let SceneOp::ClearRegions { rects, .. } = &frame.ops[1] else {
            panic!("the damage is cleared");
        };
        // Disjoint: the overlap is drawn once, not twice.
        for (i, x) in rects.iter().enumerate() {
            for y in &rects[i + 1..] {
                assert!(x.intersect(*y).is_empty(), "{x:?} overlaps {y:?}");
            }
        }
        let total: u64 = rects
            .iter()
            .map(|r| u64::from(r.width()) * u64::from(r.height()))
            .sum();
        let expected = 30 * 30 + 30 * 30 - 20 * 20;
        assert_eq!(total, expected);
    }

    #[test]
    fn a_layer_with_no_composite_draws_nothing_outside_itself() {
        // `composite: None` — the layer is pushed for isolation but its ink
        // never reaches the parent (the planner emits this for a layer whose
        // content is fully clipped away).
        let layer = rect(0, 0, 40, 40);
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
                    composite: None,
                    filter: None,
                    mask: None,
                    blend: BlendMode::Normal,
                    alpha: 1.0,
                },
            ],
            1,
        );
        let frame = frame_passes(&plan, W, H, CLEAR, None, None);
        assert!(
            !frame
                .ops
                .iter()
                .any(|op| matches!(op, SceneOp::Post { .. })),
            "no composite, no post passes"
        );
    }

    #[test]
    fn a_draw_step_with_no_geometry_emits_nothing() {
        let mut empty = plan(
            vec![Step::Draw {
                first_run: 0,
                run_count: 1,
            }],
            0,
        );
        empty.vertices.clear();
        empty.indices.clear();
        let frame = frame_passes(&empty, W, H, CLEAR, None, None);
        assert_eq!(frame.ops.len(), 1, "only the frame's own begin");
    }

    #[test]
    fn every_blend_mode_has_the_index_its_shader_case_expects() {
        assert_eq!(blend_index(BlendMode::Normal), 0.0);
        assert_eq!(blend_index(BlendMode::Luminosity), 27.0);
        // The full range: 28 modes, no gaps, no repeats.
        let modes = [
            BlendMode::Normal,
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::Dst,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcAtop,
            BlendMode::DstAtop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];
        let indices: Vec<f32> = modes.iter().map(|m| blend_index(*m)).collect();
        for (i, index) in indices.iter().enumerate() {
            assert_eq!(*index, i as f32);
        }
    }
}
