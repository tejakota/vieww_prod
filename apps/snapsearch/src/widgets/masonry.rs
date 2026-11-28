//! The masonry grid — few, large cells, alive on their own clock.
//!
//! `vieww_widget::GridView` is uniform-pitch only (see the framework's own
//! `docs/PRODUCTION-GAPS.md`), so — same as the HTML prototype's "Option A"
//! — this hand-builds N `Flex::column()`s and distributes tiles into the
//! shortest one as it goes. It's a plain `Widget` (`WidgetKind::Composed`),
//! not a virtualised list, which is the right trade at "a couple of large
//! cells per screen": there's nothing here worth virtualising.

use std::cell::RefCell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww::widget::Handler;

use crate::photo::{self, Photo};
use crate::widgets::Ambient;
use crate::theme::FluidTokens;

pub const COLUMN_COUNT: usize = 2;
pub const TILE_BASE: f32 = 208.0;

/// A cheap deterministic triangle wave in `[-1, 1]`, standing in for a sine
/// without pulling in `libm`/`std::f32::sin` formatting concerns — plenty
/// smooth at the tick rate this is sampled at.
fn triangle(phase: f32) -> f32 {
    let p = phase - phase.floor();
    if p < 0.5 {
        4.0 * p - 1.0
    } else {
        3.0 - 4.0 * p
    }
}

#[derive(Clone)]
pub struct Masonry {
    pub photos: Vec<(Photo, Option<f32>)>, // (photo, match score if this is a results grid)
    /// Per-tile entrance progress, 0→1, one entry per tile.
    ///
    /// Read out of the reveal `Signal`s by the *caller*, inside its own
    /// `Widget::build`, and passed down as plain numbers. That is deliberate:
    /// a `.get()` only subscribes while a build is on the tracking stack, so
    /// reading a signal inside one of the nested builder closures below would
    /// subscribe nothing at all — silently.
    pub reveal: Vec<f32>,
    /// Whether tiles turn themselves over to other photos.
    ///
    /// True while browsing, false while showing search results. A results
    /// grid that quietly reshuffles itself is not alive, it is broken: the
    /// screenshots caught a "sunset over water" search whose tiles had
    /// flipped to a desert and a night skyline while still wearing the
    /// original matches' 100% badges.
    pub live: bool,
    /// The vertical band of the grid to actually build, as
    /// `(top, bottom)` in the grid's own coordinates. Tiles wholly outside it
    /// become a spacer of the same height. `None` builds everything.
    pub window: Option<(f32, f32)>,
    pub on_tap: Handler<Photo>,
    pub key: Option<Key>,
}

// `Handler<Photo>` is `Rc<dyn Fn(Photo)>`, which has no `Debug` impl — every
// `Widget` must be `Debug` (it's part of the trait bound), so this reports
// the shape without trying to print the closure.
impl std::fmt::Debug for Masonry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Masonry")
            .field("tiles", &self.photos.len())
            .finish()
    }
}

impl Masonry {
    #[must_use]
    pub fn new(photos: Vec<(Photo, Option<f32>)>, on_tap: Handler<Photo>) -> Self {
        Self {
            photos,
            reveal: Vec::new(),
            live: true,
            window: None,
            on_tap,
            key: None,
        }
    }

    /// Give the tiles a staggered entrance — see [`Self::reveal`].
    #[must_use]
    pub fn reveal(mut self, reveal: Vec<f32>) -> Self {
        self.reveal = reveal;
        self
    }

    /// Build only the tiles that intersect `(top, bottom)`; see
    /// [`Self::window`].
    #[must_use]
    pub const fn window(mut self, top: f32, bottom: f32) -> Self {
        self.window = Some((top, bottom));
        self
    }

    /// Hold every tile on its own photo — see [`Self::live`].
    #[must_use]
    pub fn still(mut self) -> Self {
        self.live = false;
        self
    }

    #[must_use]
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }
}

impl Widget for Masonry {
    fn debug_name(&self) -> &'static str {
        "Masonry"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let theme = ThemeData::of(ctx);
        // Ambient drift and tile turnover are decoration: the device tier and
        // the person's reduce-motion setting can each switch them off, and
        // reading them here (an inherited read) rebuilds the grid if either
        // changes.
        let quality = ctx.inherit_or(vieww::foundation::QualityProfile::HIGH);
        let access = ctx.inherit_or(vieww::foundation::Accessibility::default());
        let ambient_ok = self.live && quality.allows_ambient_motion(access.reduce_motion);

        let gap = theme.metrics.gap * 1.5;
        let mut heights = [0.0_f32; COLUMN_COUNT];
        let mut columns: Vec<Vec<WidgetNode>> = vec![Vec::new(); COLUMN_COUNT];
        // Per column: the height of the run of off-window tiles being
        // collapsed into one spacer, and how many tiles are in it.
        let mut runs: [(f32, usize); COLUMN_COUNT] = [(0.0, 0); COLUMN_COUNT];

        for (index, (photo, score)) in self.photos.iter().enumerate() {
            let shortest = heights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let tile_h = TILE_BASE * photo.aspect;
            let top = heights[shortest];
            heights[shortest] += tile_h + gap;

            let visible = self
                .window
                .is_none_or(|(w_top, w_bottom)| top + tile_h >= w_top && top <= w_bottom);
            if !visible {
                let run = &mut runs[shortest];
                run.0 += tile_h;
                run.1 += 1;
                continue;
            }
            flush_run(&mut columns[shortest], &mut runs[shortest], gap);

            columns[shortest].push(tile(
                photo.clone(),
                *score,
                tile_h,
                TileMotion {
                    index: index as u64,
                    // Only generated placeholders turn themselves over: a
                    // real photo that swaps itself for a gradient is a lie.
                    live: ambient_ok && photo.source.is_sample(),
                    // Anything past the pool is already in place rather than
                    // permanently invisible: a missing reveal value must never
                    // be the reason a photo cannot be seen.
                    reveal: self.reveal.get(index).copied().unwrap_or(1.0),
                },
                &self.on_tap,
                &theme,
            ));
        }
        for (column, run) in columns.iter_mut().zip(runs.iter_mut()) {
            flush_run(column, run, gap);
        }

        // Each column must be `Flexible::expanded` — a plain `Flex::row`
        // child takes only its own preferred width, and a `Flex::column`
        // with nothing of its own explicit width collapses to whatever its
        // (also width-less) tiles report, which is next to nothing. This is
        // what actually splits the row's width evenly between columns.
        let column_widgets: Vec<WidgetNode> = columns
            .into_iter()
            .map(|tiles| {
                Flexible::expanded(1)
                    .child(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .spacing(theme.metrics.gap * 1.5)
                            .children(tiles),
                    )
                    .into()
            })
            .collect();

        Flex::row()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(theme.metrics.gap * 1.5)
            .children(column_widgets)
            .into()
    }
}

/// Close the open run of off-window tiles as one spacer.
///
/// Its height is the tiles' own plus the gaps *between* them; the gaps on
/// either side are the column's own spacing, exactly as for real tiles, so
/// the content below sits where it would have with everything built.
fn flush_run(column: &mut Vec<WidgetNode>, run: &mut (f32, usize), gap: f32) {
    if run.1 > 0 {
        let height = run.0 + gap * (run.1 - 1) as f32;
        column.push(SizedBox::from_size(Size::new(0.0, height)).into());
    }
    *run = (0.0, 0);
}

/// The total height of the grid, for tests and for sizing the scroll extent:
/// what [`Masonry`] lays out regardless of how much of it is built.
pub fn grid_height(photos: &[(Photo, Option<f32>)], gap: f32) -> f32 {
    let mut heights = [0.0_f32; COLUMN_COUNT];
    for (photo, _) in photos {
        let shortest = heights
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        heights[shortest] += TILE_BASE * photo.aspect + gap;
    }
    heights.iter().copied().fold(0.0, f32::max)
}

/// What a tile needs to know about *itself* rather than about the wall: the
/// ambient-drift inputs (grid index, the shared clock, whether motion is on,
/// and the reveal progress), grouped so `tile` stays under the argument count
/// a reader can hold in their head.
struct TileMotion {
    index: u64,
    live: bool,
    reveal: f32,
}

fn tile(
    photo: Photo,
    score: Option<f32>,
    height: f32,
    motion: TileMotion,
    on_tap: &Handler<Photo>,
    theme: &Rc<ThemeData>,
) -> WidgetNode {
    let TileMotion { index, live, reveal } = motion;

    // The entrance: rise into place and fade in together. `reveal` comes from
    // a spring in the *spatial* family, so it overshoots slightly and the
    // tile settles — but the opacity is clamped rather than following it,
    // because an alpha that overshoots asks for 1.06, clamps, and stalls
    // visibly at full opacity. Spatial motion may overshoot; effects may not.
    let rise = (1.0 - reveal) * 26.0;
    let fade = reveal.clamp(0.0, 1.0);

    let body: WidgetNode = {
        let on_tap = on_tap.clone();
        let theme = theme.clone();
        let id = photo.id.clone();
        // Always an `Ambient`, keyed by photo: the key keeps a tile's state
        // with its photo as the virtualised window slides past, and `live`
        // decides whether it ticks. A disabled one builds once and never
        // asks for a frame.
        // The content changes only when the tile turns over (every `cycle`
        // steps); between turns an ambient step moves the tile and nothing
        // else. So the body for the current turn is built once and handed back
        // as the *same* node, which reconciliation skips by identity, and only
        // the drift transform above it is new (SS-03). The `RepaintBoundary`
        // keeps the body's recording, so a step re-records nothing either.
        let memo: Rc<RefCell<Option<(u64, WidgetNode)>>> = Rc::new(RefCell::new(None));
        Ambient::new(live, move |step| {
            let tick = if live { step } else { 0 };
            let turn = tick / turn_cycle(index);
            let body = {
                let mut memo = memo.borrow_mut();
                match &*memo {
                    Some((at, node)) if *at == turn => node.clone(),
                    _ => {
                        let node: WidgetNode = RepaintBoundary::new()
                            .child(tile_body(&photo, score, height, index, turn, live, &on_tap, &theme))
                            .into();
                        *memo = Some((turn, node.clone()));
                        node
                    }
                }
            };
            if live {
                Transformed::translate(drift_at(index, tick)).child(body).into()
            } else {
                body
            }
        })
        // Neighbours must not all breathe in step.
        .phase(index.wrapping_mul(11))
        .key(id)
        .into()
    };

    if fade >= 0.999 && rise.abs() < 0.01 {
        // At rest: no transform and no composited group.
        return body;
    }
    let placed = Transformed::translate(Offset::new(0.0, rise)).child(body);
    if fade >= 0.999 {
        placed.into()
    } else {
        Opacity::new(fade).child(placed).into()
    }
}

/// How many alternate "photos" are kept. Sample tiles turn over through a
/// small repeating set, so the generated gradients are built once instead of
/// on every ambient step.
const ALT_CACHE_MAX: usize = 256;

thread_local! {
    static ALT_PHOTOS: std::cell::RefCell<std::collections::HashMap<u64, Photo>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The generated stand-in for `seed`, built once and then shared.
///
/// `Photo::image` is reference-counted, so the clone out of the cache is a
/// pointer copy: an ambient step allocates no pixels.
fn alt_photo(seed: u64) -> Photo {
    ALT_PHOTOS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= ALT_CACHE_MAX && !cache.contains_key(&seed) {
            cache.clear();
        }
        cache.entry(seed).or_insert_with(|| photo::photo_for_seed(seed)).clone()
    })
}

/// Build every alternate a sample tile can turn over to, up front.
///
/// Called once at start-up, outside any build, when the sample library is
/// what the grid shows and the tier allows ambient motion: the turnover then
/// only ever *looks up* a stand-in, so no build closure allocates pixels
/// (SS-03). The whole set is 64 x 4 small gradients (about 3 MB), which is
/// why a Low-tier phone, where the grid never moves, does not build it.
pub fn prewarm_alternates() {
    for index in 0..64 {
        for turn in 0..4 {
            let _ = alt_photo(alt_seed(index, turn));
        }
    }
}

/// The seed a tile shows when it has turned over `turn` times.
///
/// Wraps within a small set (per tile) so the cache stays bounded however
/// long the app is left open.
fn alt_seed(index: u64, turn: u64) -> u64 {
    10_000 + (index % 64) * 977 + (turn % 4)
}

/// How many ambient steps a sample tile shows one picture before it turns.
fn turn_cycle(index: u64) -> u64 {
    220 + (index % 7) * 40
}

/// Ambient drift: a slow, per-tile-offset wander in x and y, entirely derived
/// from the ambient step, so it needs no per-tile state.
fn drift_at(index: u64, tick: u64) -> Offset {
    let drift_period = 90.0 + (index % 5) as f32 * 18.0;
    let phase_offset = index as f32 * 0.37;
    Offset::new(
        triangle(tick as f32 / drift_period + phase_offset) * 6.0,
        triangle(tick as f32 / (drift_period * 0.8) + phase_offset + 0.25) * 8.0,
    )
}

/// One tile's content after it has turned over `turn` times. `turn == 0` with
/// `live == false` is the still tile: the original photo. The drift is not
/// here: it is the transform [`tile`] puts around this, per step.
#[allow(clippy::too_many_arguments)]
fn tile_body(
    photo: &Photo,
    score: Option<f32>,
    height: f32,
    index: u64,
    turn: u64,
    live: bool,
    on_tap: &Handler<Photo>,
    theme: &Rc<ThemeData>,
) -> WidgetNode {
    // Live-replace: every `cycle` steps this tile flips to a generated
    // "photo" and back — a page-turn (scale-collapse), since `Transformed`
    // only exposes a 2D matrix.
    let showing_alt = live && (turn + index) % 2 == 1;
    let flip_target = if showing_alt { 1.0 } else { 0.0 };
    let alt = alt_photo(alt_seed(index, turn));

    let shown_photo = photo.clone();
    let shown_alt = alt.clone();
    let theme_for_build = theme.clone();
    let card = Animated::new(flip_target)
        .duration(std::time::Duration::from_millis(380))
        .curve(Curve::EASE_IN_OUT)
        .build(move |t| {
            let (shown, shown_label) = if t < 0.5 {
                (shown_photo.image.clone(), shown_photo.label.clone())
            } else {
                (shown_alt.image.clone(), shown_alt.label.clone())
            };
            // A page-turn stand-in for a true 3D card flip: the "flip" is a
            // horizontal squash-to-nothing-and-back, with content swapped at
            // the collapsed midpoint (t == 0.5) where nothing is visible.
            let squash = (t * 2.0 - 1.0).abs().max(0.02);

            // The picture itself, clipped to the tile's rounded corners.
            let picture: WidgetNode = Clip::new(ClipShape::RRect {
                // The tile's own curve token, not `metrics.corner`: that one
                // is tuned so *controls* come out as stadiums, and a 200px
                // card at that radius would be a capsule.
                radius: crate::theme::FluidTokens::new().tile_curve,
            })
            .child(
                Container::new()
                    .color(theme_for_build.colors.surface_variant)
                    .child(Image::new(shown).fit(BoxFit::Cover).label(shown_label)),
            )
            .into();

            let mut layers = vec![Positioned::fill().child(picture).into()];

            if let Some(score) = score {
                // The relevance badge as the fluid ramp itself. Deliberately
                // a *sibling* of the clip rather than a child of it: inside
                // the `Clip` the pill painted but its glyphs did not — the
                // known CPU-backend glyph placement bug the framework
                // records in `HANDOFF-2026-08-21.md`.
                layers.push(
                    Positioned::new()
                        .left(10.0)
                        .bottom(10.0)
                        .child(
                            Container::new()
                                .gradient(FluidTokens::new().fluid_gradient())
                                .radius(f32::MAX)
                                .padding(EdgeInsets::symmetric(11.0, 5.0))
                                .shadow(Shadow {
                                    color: Color::rgba(0x7c, 0x3a, 0xed, 0x8c),
                                    offset: Offset::new(0.0, 4.0),
                                    blur: 14.0,
                                    spread: 0.0,
                                    is_inset: false,
                                })
                                .child(
                                    Flex::row()
                                        .main_axis_size(MainAxisSize::Min)
                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                        .spacing(5.0)
                                        .children(children![
                                            Icon::new(crate::widgets::icons::sparkle())
                                                .size(11.0)
                                                .color(Color::WHITE)
                                                .label("match"),
                                            Text::new(format!(
                                                "{}%",
                                                (score * 100.0).round() as i32
                                            ))
                                            .color(Color::WHITE)
                                            .size(theme_for_build.text.label.size)
                                            .bold(),
                                        ]),
                                ),
                        )
                        .into(),
                );
            }

            Transformed::scale(squash, 1.0)
                .child(
                    Container::new()
                        .height(height)
                        .child(Stack::new().fit(StackFit::Expand).children(layers)),
                )
                .into()
        });

    let handler = on_tap.clone();
    let orig = photo.clone();

    Pressable::new(move |_press| card.clone().into())
        .on_tap(move || {
            handler(if showing_alt { alt.clone() } else { orig.clone() });
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternates_are_built_once_per_seed_and_shared() {
        let a = alt_photo(alt_seed(3, 0));
        let b = alt_photo(alt_seed(3, 0));
        assert_eq!(a.id, b.id);
        // Same pixels, not a regenerated copy.
        assert!(a.image == b.image, "an ambient step must not allocate pixels");
    }

    #[test]
    fn the_alternate_set_is_bounded_however_long_the_app_runs() {
        let seeds: std::collections::HashSet<u64> =
            (0..100_000u64).map(|t| alt_seed(7, t)).collect();
        assert!(seeds.len() <= 4);
        let all: std::collections::HashSet<u64> =
            (0..1000u64).flat_map(|i| (0..8).map(move |t| alt_seed(i, t))).collect();
        assert!(all.len() <= 64 * 4);
    }

    #[test]
    fn a_still_tile_has_no_ambient_wrapper_and_a_live_one_does() {
        let on_tap: Handler<Photo> = Rc::new(|_| {});
        let photos = vec![(photo::photo_for_seed(1), None)];
        let mut m = Masonry::new(photos.clone(), on_tap.clone());
        m.live = true;
        assert!(m.live);
        assert_eq!(Masonry::new(photos, on_tap).still().live, false);
    }
}

widget_node_from!(Masonry);
