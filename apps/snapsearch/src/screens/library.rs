//! The library / search screen.
//!
//! A close port of the HTML prototype's `screens/library`. Where the two
//! differ, the difference is noted at the site rather than left to be
//! discovered by comparing screenshots — the earlier pass drifted on
//! several details at once (stock palette instead of the prototype's, an
//! emoji instead of the magnifier, no grab handle, no search hint, a
//! bottom-anchored progress card instead of a centred one), which is
//! exactly the kind of drift that is invisible from the code alone.

use std::rc::Rc;

use vieww::prelude::*;
use vieww::widget::Handler;

use crate::photo::Photo;
use crate::state::{AppState, LibraryView, Match, SearchMode, SpaceStatus};
use crate::theme::FluidTokens;
use crate::widgets::Ambient;
use crate::widgets::icons as app_icons;
use crate::widgets::{Masonry, SpaceMap};
use crate::library::Origin;
use crate::screens::DetailView;

/// How far the sheet travels when it slides away.
const SHEET_TRAVEL: f32 = 460.0;

/// The backdrop blur, as a Gaussian sigma — roughly CSS `blur(24px)`.
const BACKDROP_SIGMA: f32 = 8.0;

pub struct LibraryScreen {
    pub state: AppState,
}

impl std::fmt::Debug for LibraryScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryScreen").finish()
    }
}

impl Widget for LibraryScreen {
    fn debug_name(&self) -> &'static str {
        "LibraryScreen"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let theme = ThemeData::of(ctx);
        let tokens = FluidTokens::new();
        let state = self.state.clone();

        let library = state.photos.get();
        let results = state.results.get();
        let sheet_open = state.sheet_open.get();
        let searching = state.searching.get();
        let mode = state.mode.get();
        let text_query = state.text_query.get();
        let ref_photo = state.ref_photo.get();
        let toast = state.toast.get();
        // Both read here, inside `Widget::build`, which is the only place a
        // `.get()` subscribes.
        let header_t = state.header.progress().get();
        // Known before the content is built, because the frosting wraps it.
        let origin = state.origin.get();
        let loading = state.loading.get();
        let issues = state.load_issues.get();
        let layout_pending = state.layout_pending.get();
        let detail = state.detail.get();
        let detail_full = state.detail_full.get();
        // The detail view is modal too, so it frosts the library behind it.
        let dim = sheet_open || searching || detail.is_some();
        let reveal: Vec<f32> = state.reveal.iter().map(Signal::get).collect();

        // Badges show relevance *relative to the best hit*, not the raw
        // cosine. Raw cosine would be dishonest as a percentage: CLIP's
        // image/text similarities live in a narrow band and a perfect match
        // scores about 0.3, which a "30% match" badge would misreport as a
        // poor one. Scaling against the top hit is the ordinary relevance
        // convention and reads correctly under either encoder.
        let tiles: Vec<(Photo, Option<f32>)> = match &results {
            Some(matches) => {
                let best = matches.first().map(|(_, s)| *s).unwrap_or(1.0).max(f32::EPSILON);
                matches
                    .iter()
                    .map(|(p, s)| (p.clone(), Some((s / best).clamp(0.0, 1.0))))
                    .collect()
            }
            None => library.iter().cloned().map(|p| (p, None)).collect(),
        };

        let view = state.view.get();
        let has_tiles = !tiles.is_empty();

        let tap_state = state.clone();
        let on_tap: Handler<Photo> = Rc::new(move |photo: Photo| {
            tap_state.open_detail(photo);
        });

        // ---- the scrolling library underneath everything ---------------
        let grid: WidgetNode = if tiles.is_empty() {
            empty_state(&theme, &tokens)
        } else if view == LibraryView::Space {
            space_view(&state, &theme, &library, results.as_deref(), on_tap)
        } else {
            Scrollable::vertical(state.scroll.offset())
                .on_drag(state.scroll.on_drag())
                .on_drag_end(state.scroll.on_drag_end())
                .on_extents(state.scroll.on_extents())
                .child(
                    Padding::new(EdgeInsets::only(
                        theme.metrics.gap * 1.5,
                        0.0,
                        theme.metrics.gap * 1.5,
                        // Room for the FAB to float over, rather than the
                        // last row being permanently half-covered by it.
                        tokens.fab_size + theme.metrics.gap * 5.0,
                    ))
                    .child(frosted(dim, &theme, {
                        // Only the tiles near the viewport are built; the band changes a few
                        // times per screen of scrolling, not every frame.
                        let band = state.scroll_band.get();
                        let (top, bottom) = crate::state::window_for(band, state.viewport());
                        let grid = Masonry::new(tiles, on_tap).window(top, bottom);
                        // Results hold still and arrive staggered; the
                        // library is the thing that drifts.
                        let grid = if results.is_some() {
                            grid.still().reveal(reveal)
                        } else {
                            grid
                        };
                        WidgetNode::from(grid)
                    })),
                )
                .into()
        };

        let content = Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children![
                frosted(
                    dim,
                    &theme,
                    header(&state, &theme, results.as_deref(), header_t, &origin, loading, issues),
                ),
                // The Space view's one line of instruction — the grid needs
                // none, its gesture is universal; a map does.
                if view == LibraryView::Space && has_tiles {
                    space_caption(&theme)
                } else {
                    SizedBox::shrink().into()
                },
                // `Scrollable` has no height of its own; without an
                // expanded `Flexible` it collapses to nothing.
                Flexible::expanded(1).child(grid),
            ]);

        // ---- scrim ------------------------------------------------------
        // Flat and heavily tinted, deliberately: the prototype's mapping
        // records that `backdrop-filter: blur()` has no vieww equivalent,
        // so this is the agreed substitute rather than an approximation of
        // a blur that was never available.
        let close_for_barrier = state.clone();
        let barrier = Animated::new(if dim { 1.0 } else { 0.0 })
            .duration(theme.motion.duration_medium)
            .build(move |t| {
                if t <= 0.001 {
                    return SizedBox::shrink().into();
                }
                let close = close_for_barrier.clone();
                Opacity::new(t)
                    .child(
                        ModalBarrier::new()
                            // Lighter than it was: this used to carry the
                            // whole job of separating the sheet from the
                            // library. Now the blur does most of it and the
                            // tint only has to buy contrast.
                            .color(tokens.scrim_over_blur)
                            .on_dismiss(move || close.close_sheet()),
                    )
                    .into()
            });

        // ---- the search sheet -------------------------------------------
        // Read here, in `build`, so dragging the grip re-places the sheet.
        let sheet_drag = state.sheet_drag.get();
        let sheet = Animated::new(if sheet_open { 1.0 } else { 0.0 })
            .duration(theme.motion.duration_long)
            .curve(theme.motion.curve_emphasized)
            .build({
                let state = state.clone();
                let theme = theme.clone();
                move |t| {
                    Positioned::new()
                        .left(0.0)
                        .right(0.0)
                        .bottom(-SHEET_TRAVEL * (1.0 - t) - sheet_drag)
                        .child(search_sheet(
                            &state,
                            &theme,
                            &tokens,
                            mode,
                            &text_query,
                            ref_photo.clone(),
                        ))
                        .into()
                }
            });

        // ---- the fluid FAB ----------------------------------------------
        // Fades and shrinks away while the sheet is up. Without this it
        // sits on top of the sheet's own Search button — visible in the
        // first screenshots, and the sort of thing only a picture shows.
        // Retracts whenever the scrim is up — sheet open *or* mid-search.
        // Keyed off the same `dim` the barrier uses so the two can never
        // disagree about whether the screen is currently modal.
        // The halo breathes only when the device tier and the person both
        // allow ambient motion, and never behind a modal.
        let quality = ctx.inherit_or(vieww::foundation::QualityProfile::HIGH);
        let access = ctx.inherit_or(vieww::foundation::Accessibility::default());
        let halo_breathes =
            quality.allows_ambient_motion(access.reduce_motion) && !dim;
        let fab = Animated::new(if dim { 0.0 } else { 1.0 })
            .duration(theme.motion.duration_medium)
            .curve(theme.motion.curve_emphasized)
            .build({
                let state = state.clone();
                let theme = theme.clone();
                let badge = results.as_ref().map(|r| r.len());
                // The halo ticks itself (`Ambient`), so the screen no longer
                // rebuilds every 140 ms to hand it a phase.
                move |t| {
                    if t <= 0.001 {
                        return SizedBox::shrink().into();
                    }
                    Positioned::new()
                        .right(theme.metrics.gap * 2.0)
                        .bottom(theme.metrics.gap * 3.0)
                        .child(
                            Opacity::new(t).child(
                                Transformed::scale(0.85 + 0.15 * t, 0.85 + 0.15 * t)
                                    .child(fab_cluster(&state, &theme, &tokens, badge, halo_breathes)),
                            ),
                        )
                        .into()
                }
            });

        // ---- the searching dialog ---------------------------------------
        let progress_card = Animated::new(if searching { 1.0 } else { 0.0 })
            .duration(theme.motion.duration_medium)
            .build({
                let theme = theme.clone();
                let tokens = FluidTokens::new();
                let embedder = state.embedder.clone();
                let state = state.clone();
                move |t| {
                    if t <= 0.001 {
                        return SizedBox::shrink().into();
                    }
                    // Centre it. The earlier pass set only left/right on a
                    // `Positioned`, which pins the horizontal axis and
                    // leaves the vertical to the stack's alignment — the
                    // card ended up wherever that happened to put it.
                    Positioned::fill()
                        .child(
                            Align::new(Alignment::CENTER).child(
                                Opacity::new(t).child(Padding::new(EdgeInsets::all(32.0)).child(
                                    progress_dialog(&state, &theme, &tokens, embedder.name()),
                                )),
                            ),
                        )
                        .into()
                }
            });

        // ---- toast ------------------------------------------------------
        let toast_bar = Animated::new(if toast.is_some() { 1.0 } else { 0.0 })
            .duration(theme.motion.duration_medium)
            .build({
                let theme = theme.clone();
                let toast = toast.clone();
                move |t| {
                    let Some(message) = toast.clone() else {
                        return SizedBox::shrink().into();
                    };
                    if t <= 0.001 {
                        return SizedBox::shrink().into();
                    }
                    Positioned::new()
                        .left(0.0)
                        .right(0.0)
                        // Clear of the FAB, which owns the bottom-right.
                        .bottom(theme.metrics.gap * 3.0 + 80.0)
                        .child(
                            Align::new(Alignment::CENTER)
                                .child(Opacity::new(t).child(toast_pill(&theme, message))),
                        )
                        .into()
                }
            });

        // Frames, on demand. The screen used to be rebuilt every 140 ms by a
        // shared clock, which kept the frame loop alive as a side effect; now
        // nothing runs while the library is idle, so work that has to be
        // polled (workers finishing, the search timing out, the toast
        // expiring) asks for frames here, and only while it is pending.
        let pacemaker = Ambient::new(
            searching
                || toast.is_some()
                || loading > 0
                || layout_pending
                || (detail.is_some() && detail_full.is_none()),
            |_| SizedBox::shrink().into(),
        );

        Container::new()
            .color(tokens.background)
            .child(Stack::new().fit(StackFit::Expand).children(children![
                pacemaker,
                Positioned::fill().child(SafeArea::new().child(content)),
                barrier,
                sheet,
                fab,
                progress_card,
                toast_bar,
                // Above everything, including the sheet — it is the most
                // modal thing on the screen.
                Animated::new(if detail.is_some() { 1.0 } else { 0.0 })
                    .duration(theme.motion.duration_medium)
                    .curve(theme.motion.curve_emphasized)
                    .build({
                        let state = state.clone();
                        move |t| match (&detail, t) {
                            (Some(photo), t) if t > 0.001 => DetailView {
                                state: state.clone(),
                                photo: photo.clone(),
                                full: detail_full.clone(),
                                progress: t,
                            }
                            .into(),
                            _ => SizedBox::shrink().into(),
                        }
                    }),
            ]))
            .into()
    }
}

// ===================================================================== bits

/// Blur a subtree while a modal is up — the app's "frosted glass".
///
/// # Why this is applied per-region rather than once around the screen
///
/// `Filtered` filters the group's OWN pixels. It is `filter`, not
/// `backdrop-filter`, which vieww still does not have — so what should look
/// blurred goes INSIDE the filter and the sheet goes over it in the `Stack`.
/// That much is the documented workaround.
///
/// What is **not** documented, and what a screenshot caught: a `Filtered`
/// does not compose with a repaint boundary inside it. A `Scrollable`'s
/// `RenderViewport::is_repaint_boundary()` is `true`, and a boundary records
/// into a layer the filter's offscreen pass never reads. Wrapping the whole
/// screen — header plus scrolling grid — blurred the header and made every
/// photo **vanish**. `src/bin/filter_probe.rs` reduces it to stock widgets:
///
/// | subtree inside the filter | result |
/// |---|---|
/// | plain content | filtered correctly |
/// | a `Scrollable` alone | filter silently dropped; content intact, no blur |
/// | plain content *and* a `Scrollable` | filter applies to the plain part, the scrollable's content is lost |
///
/// So the filter goes on each side of the boundary rather than across it.
/// Two filtered layers instead of one, which is still a nameable number —
/// each costs an offscreen buffer and a kernel pass per frame, and both
/// exist only while a modal is up.
///
/// σ, not a CSS pixel radius: a blur reaches about 3σ, so [`BACKDROP_SIGMA`]
/// is roughly `filter: blur(24px)`.
fn frosted(dim: bool, theme: &Rc<ThemeData>, child: impl Into<WidgetNode>) -> WidgetNode {
    let child = child.into();
    Animated::new(if dim { 1.0 } else { 0.0 })
        .duration(theme.motion.duration_medium)
        .curve(theme.motion.curve_standard)
        .build(move |t| {
            let sigma = t * BACKDROP_SIGMA;
            if sigma <= 0.05 {
                // No filter at all when nothing is blurred, rather than a
                // σ-0 one: an identity filter still buys the offscreen.
                return child.clone();
            }
            Filtered::blur(sigma)
                // Desaturating as it recedes. Chained colour calls compose
                // into ONE matrix, so this is still a single pass.
                .saturation(1.0 - 0.3 * t)
                .child(child.clone())
                .into()
        })
        .into()
}

/// `collapse` is 0 at the top of the library and 1 once it has scrolled
/// [`crate::state::HEADER_RANGE`] — a `ScrollTimeline`, which is one clamped
/// division read by everything that responds to it, rather than N separate
/// scroll listeners each with their own idea of where the scroll is.
/// What the header says about photos that are not on screen, so a library
/// never quietly looks smaller than it is. `(skipped by the scan limit,
/// failed to load)`.
pub fn issue_note((skipped, failed): (usize, usize)) -> Option<String> {
    match (skipped, failed) {
        (0, 0) => None,
        (s, 0) => Some(format!("{s} OVER THE LIMIT")),
        (0, f) => Some(format!("{f} COULD NOT BE READ")),
        (s, f) => Some(format!("{s} OVER THE LIMIT, {f} UNREADABLE")),
    }
}

fn header(
    state: &AppState,
    theme: &Rc<ThemeData>,
    results: Option<&Vec<Match>>,
    collapse: f32,
    origin: &Origin,
    loading: usize,
    issues: (usize, usize),
) -> WidgetNode {
    let headline = match results {
        Some(matches) if matches.is_empty() => "No matches".to_string(),
        Some(matches) => format!("{} match{}", matches.len(), if matches.len() == 1 { "" } else { "es" }),
        None => format!("{} photos", state.photos.peek().len()),
    };

    // The eyebrow fades out over the first half of the collapse and the
    // headline shrinks toward the title size — the header gets out of the
    // way of the photos without ever disappearing.
    let eyebrow_alpha = (1.0 - collapse * 2.0).clamp(0.0, 1.0);
    let headline_size = theme.text.headline.size
        + (theme.text.title.size - theme.text.headline.size) * collapse;

    let mut stacked = vec![Text::new(headline)
        .color(theme.colors.on_surface)
        .size(headline_size)
        .bold()
        .into()];
    if eyebrow_alpha > 0.001 {
        // Says where these pictures came from, and never lets a generated
        // set pass for a photo library.
        let eyebrow = if loading > 0 {
            format!("INDEXING · {loading} LEFT")
        } else {
            match origin {
                Origin::Directory(dir) => {
                    let name = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_uppercase())
                        .unwrap_or_else(|| "LIBRARY".to_string());
                    match issue_note(issues) {
                        Some(note) => format!("{name} · {note}"),
                        None => name,
                    }
                }
                Origin::Samples => "SAMPLE SET · NO PHOTOS FOUND".to_string(),
            }
        };
        stacked.insert(
            0,
            Opacity::new(eyebrow_alpha)
                .child(
                    Text::new(eyebrow)
                        .color(if loading > 0 {
                            theme.colors.primary
                        } else {
                            theme.colors.on_surface_variant
                        })
                        .size(theme.text.label.size),
                )
                .into(),
        );
    }

    // The headline yields to whatever the row's right side needs — the view
    // toggle and, with results, the Clear button — rather than the row
    // overflowing the screen's width when all three are present.
    let mut row = vec![Flexible::expanded(1)
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(2.0)
                .children(stacked),
        )
        .into()];

    // Grid or Space: the library, seen two ways. The active side is filled;
    // the other stays text. Two small buttons rather than a segmented
    // control — the header is already busy, and a toggle that reads as two
    // words beats one that reads as a widget.
    let view = state.view.peek();
    let set_grid = state.clone();
    let set_space = state.clone();
    row.push(
        Flex::row()
            .spacing(4.0)
            .push(
                Button::new("Grid")
                    .style(if view == LibraryView::Grid {
                        ButtonStyle::Filled
                    } else {
                        ButtonStyle::Text
                    })
                    .on_pressed(move || set_grid.set_view(LibraryView::Grid)),
            )
            .push(
                Button::new("Space")
                    .style(if view == LibraryView::Space {
                        ButtonStyle::Filled
                    } else {
                        ButtonStyle::Text
                    })
                    .on_pressed(move || set_space.set_view(LibraryView::Space)),
            )
            .into(),
    );

    if results.is_some() {
        let clear = state.clone();
        row.push(
            Button::new("Clear")
                .style(ButtonStyle::Text)
                .on_pressed(move || clear.clear_search())
                .into(),
        );
    }

    Padding::new(EdgeInsets::only(
        theme.metrics.gap * 2.5,
        theme.metrics.gap * 2.0 - theme.metrics.gap * collapse,
        theme.metrics.gap * 2.0,
        theme.metrics.gap * 1.5 - theme.metrics.gap * collapse,
    ))
    .child(
        Flex::row()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(row),
    )
    .into()
}

/// The Space view: the library laid out by embedding similarity.
///
/// Photos whose vector has not landed yet are simply absent — the map shows
/// what the index knows. Match scores, when a search has run, are normalised
/// against the best hit exactly as the grid's badges are, so a dot's ring and
/// its tile's badge say the same number.
fn space_view(
    state: &AppState,
    theme: &Rc<ThemeData>,
    library: &Rc<Vec<Photo>>,
    results: Option<&Vec<Match>>,
    on_tap: Handler<Photo>,
) -> WidgetNode {
    // Subscribes this build to the layout landing.
    let _ = state.layout_epoch.get();
    // The layout is computed off the frame thread and cached; until it lands
    // the screen shows a skeleton rather than blocking on an O(n²) pass.
    let (layout, photos) = match state.space_layout() {
        SpaceStatus::Ready { layout, photos } => (layout, (*photos).clone()),
        SpaceStatus::Computing => return space_skeleton(theme),
    };
    let _ = library;

    // Scores, parallel to the placed photos: photo id -> normalised score.
    let scores: Vec<Option<f32>> = match results {
        Some(matches) => {
            let best = matches.first().map(|(_, s)| *s).unwrap_or(1.0).max(f32::EPSILON);
            let by_id: std::collections::HashMap<&str, f32> = matches
                .iter()
                .map(|(p, s)| (p.id.as_str(), (s / best).clamp(0.0, 1.0)))
                .collect();
            photos
                .iter()
                .map(|p| by_id.get(p.id.as_str()).copied())
                .collect()
        }
        None => photos.iter().map(|_| None).collect(),
    };

    Padding::new(EdgeInsets::only(
        theme.metrics.gap * 1.5,
        0.0,
        theme.metrics.gap * 1.5,
        theme.metrics.gap * 1.5,
    ))
    .child(
        Clip::rounded(theme.metrics.corner * 2.0).child(SpaceMap {
            photos,
            layout,
            scores,
            has_matches: results.is_some_and(|m| !m.is_empty()),
            on_tap,
        }),
    )
    .into()
}

/// What the Space view shows while its layout is being computed: the map's
/// plate with a quiet pulse, so the tab never looks frozen or empty.
fn space_skeleton(theme: &Rc<ThemeData>) -> WidgetNode {
    let (surface, text, size) =
        (theme.colors.surface_variant, theme.colors.on_surface_variant, theme.text.body.size);
    Padding::new(EdgeInsets::only(
        theme.metrics.gap * 1.5,
        0.0,
        theme.metrics.gap * 1.5,
        theme.metrics.gap * 1.5,
    ))
    .child(
        Clip::rounded(theme.metrics.corner * 2.0).child(
            Container::new().color(Color::rgb(14, 12, 22)).alignment(Alignment::CENTER).child(
                Ambient::new(true, move |step| {
                    // A slow breath on the label; the plate itself is still.
                    let phase = (step % 14) as f32 / 14.0;
                    let tri = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
                    Opacity::new(0.45 + 0.4 * tri)
                        .child(
                            Container::new()
                                .color(surface)
                                .radius(f32::MAX)
                                .padding(EdgeInsets::symmetric(18.0, 10.0))
                                .child(
                                    Text::new("Laying out your library…").color(text).size(size),
                                ),
                        )
                        .into()
                }),
            ),
        ),
    )
    .into()
}

/// The Space view's one line of instruction.
fn space_caption(theme: &Rc<ThemeData>) -> WidgetNode {
    Padding::new(EdgeInsets::symmetric(theme.metrics.gap * 2.5, 0.0))
        .child(
            Text::new("SPACE · placed by embedding similarity — near is alike; matches ring in viridis")
                .color(theme.colors.on_surface_variant)
                .size(theme.text.label.size),
        )
        .into()
}

/// The FAB, plus the "Search your photos…" hint pill beside it.
///
/// `tick` is the ambient clock the masonry already drifts on. The halo behind
/// the button breathes on the same clock — a slow radial glow swelling and
/// settling — so the one control the whole flow starts from is the one thing
/// on the screen that is visibly *alive*, before anything is pressed. Same
/// triangle wave, same period class as the tiles' drift: one ambient rhythm,
/// not two.
/// The largest the halo's padding ever gets.
const HALO_MAX: f32 = 20.0;

/// The halo at ambient step `step`: `(padding, alpha)`.
///
/// A 0→1→0 triangle over 50 steps (~7 s at the 140 ms step), mapped to scale
/// and alpha. Never absent — the FAB is the flow's entry point and a pulse
/// that fully disappears reads as a flicker rather than a breath.
fn halo_at(step: u64) -> (f32, f32) {
    let phase = (step % 50) as f32 / 50.0;
    let tri = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
    let breath = 0.35 + 0.65 * tri;
    (10.0 + 10.0 * breath, (0.30 + 0.28 * breath).clamp(0.0, 1.0))
}

fn fab_cluster(
    state: &AppState,
    theme: &Rc<ThemeData>,
    tokens: &FluidTokens,
    badge: Option<usize>,
    breathes: bool,
) -> WidgetNode {
    let open = state.clone();
    let size = tokens.fab_size;

    let hint = Container::new()
        .color(tokens.surface_raised)
        .radius(f32::MAX) // a true stadium
        .padding(EdgeInsets::symmetric(theme.metrics.gap * 2.0, theme.metrics.gap * 1.5))
        .shadow(Shadow {
            color: Color::rgba(0, 0, 0, 0x66),
            offset: Offset::new(0.0, 4.0),
            blur: 16.0,
            spread: 0.0,
            is_inset: false,
        })
        .child(
            Text::new("Search your photos")
                .color(theme.colors.on_surface_variant)
                .size(theme.text.body.size),
        );

    // Bottom of the stack: the ambient halo, a soft violet bloom behind the
    // button, sized and faded by the breath. The gradient's last stop is the
    // transparent spelling of the SAME violet, not a flat alpha over a
    // different hue — a tinted fade that changes hue on the way out reads as
    // a smudge ring.
    let mut fab_stack: Vec<WidgetNode> = vec![
        // A fixed slot as large as the halo ever gets; the halo itself
        // breathes inside it on the element's own clock, so nothing outside
        // this subtree is rebuilt to animate it.
        Positioned::new()
            .left(-HALO_MAX)
            .top(-HALO_MAX)
            .child(
                SizedBox::from_size(Size::new(size + HALO_MAX * 2.0, size + HALO_MAX * 2.0))
                    .child(Ambient::new(breathes, move |step| {
                        let (halo_pad, halo_alpha) = halo_at(step);
                        Align::new(Alignment::CENTER)
                            .child(
                                Opacity::new(halo_alpha).child(
                                    Container::new()
                                        .size(size + halo_pad * 2.0, size + halo_pad * 2.0)
                                        .radius(f32::MAX)
                                        .gradient(
                                            Gradient::radial(Offset::new(0.5, 0.5), 0.5)
                                                .with_stops(&[
                                                    (0.0, Color::rgba(0x7c, 0x3a, 0xed, 0xff)),
                                                    (0.55, Color::rgba(0x7c, 0x3a, 0xed, 0x66)),
                                                    (1.0, Color::rgba(0x7c, 0x3a, 0xed, 0x00)),
                                                ]),
                                        ),
                                ),
                            )
                            .into()
                    })),
            )
            .into(),
        Container::new()
            .size(size, size)
            .radius(tokens.fab_curve)
            .gradient(tokens.fluid_gradient())
            .shadow(Shadow {
                color: Color::rgba(0x7c, 0x3a, 0xed, 0x8c),
                offset: Offset::new(0.0, 12.0),
                blur: 30.0,
                spread: 0.0,
                is_inset: false,
            })
            .alignment(Alignment::CENTER)
            .child(
                Icon::new(app_icons::search())
                    .size(26.0)
                    .color(Color::WHITE)
                    .label("Search photos"),
            )
            .into(),
    ];

    if let Some(n) = badge {
        fab_stack.push(
            Positioned::new()
                .right(-2.0)
                .top(-2.0)
                .child(
                    Container::new()
                        .color(tokens.coral)
                        .radius(f32::MAX)
                        .padding(EdgeInsets::symmetric(7.0, 3.0))
                        .child(
                            Text::new(n.to_string())
                                .color(Color::WHITE)
                                .size(theme.text.label.size)
                                .bold(),
                        ),
                )
                .into(),
        );
    }

    let button = Pressable::new(move |press| {
        let scale = 1.0 - press * 0.08;
        Transformed::scale(scale, scale)
            .child(
                Stack::new()
                    .alignment(Alignment::TOP_LEFT)
                    .children(fab_stack.clone()),
            )
            .into()
    })
    .on_tap(move || open.open_sheet());

    Flex::row()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(theme.metrics.gap * 1.5)
        .children(children![hint, button])
        .into()
}

/// The sheet's grab handle, which is also a real handle: dragging it down
/// moves the sheet with the finger and releasing past [`SHEET_DISMISS_PX`]
/// (or with a downward fling) dismisses it; a short drag springs back. A tall
/// transparent hit area surrounds the 4 px bar, because a 4 px target cannot
/// be hit by a thumb.
fn sheet_grip(state: &AppState, theme: &Rc<ThemeData>) -> WidgetNode {
    let (update, end) = (state.clone(), state.clone());
    GestureDetector::new()
        .on_drag_update(move |d: vieww::foundation::DragDetails| {
            let next = sheet_drag_after(update.sheet_drag.peek(), d.delta.dy);
            update.sheet_drag.set(next);
        })
        .on_drag_end(move |d: vieww::foundation::DragDetails| {
            if should_dismiss_sheet(end.sheet_drag.peek(), d.velocity.dy) {
                end.close_sheet();
            } else {
                end.sheet_drag.set(0.0);
            }
        })
        .child(
            Container::new().height(28.0).alignment(Alignment::CENTER).child(
                Container::new().size(40.0, 4.0).radius(f32::MAX).color(theme.colors.outline),
            ),
        )
        .into()
}

/// How far the sheet must be dragged to dismiss it.
pub const SHEET_DISMISS_PX: f32 = 90.0;

/// A downward fling faster than this dismisses regardless of distance.
pub const SHEET_FLING_PX_PER_S: f32 = 900.0;

/// The drag offset after a movement of `delta`: down only, never above rest.
pub fn sheet_drag_after(current: f32, delta: f32) -> f32 {
    (current + delta).clamp(0.0, SHEET_TRAVEL)
}

/// Whether releasing at `drag` px with `velocity` px/s closes the sheet.
pub fn should_dismiss_sheet(drag: f32, velocity: f32) -> bool {
    drag >= SHEET_DISMISS_PX || velocity >= SHEET_FLING_PX_PER_S
}

fn search_sheet(
    state: &AppState,
    theme: &Rc<ThemeData>,
    tokens: &FluidTokens,
    mode: SearchMode,
    text_query: &str,
    ref_photo: Option<Photo>,
) -> WidgetNode {
    let close_state = state.clone();
    let cancel_state = state.clone();
    let run_state = state.clone();
    let mode_state = state.clone();
    let text_state = state.clone();
    let submit_state = state.clone();
    let can_search = state.can_search();

    let body: WidgetNode = match mode {
        SearchMode::Text => Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(theme.metrics.gap)
            .children(children![
                // The field needs its own filled, rounded box — `TextField`
                // paints text and a caret, not a surface, so on its own it
                // reads as loose text floating in the sheet.
                Container::new()
                    .color(theme.colors.surface_variant)
                    .radius(theme.metrics.corner)
                    .padding(EdgeInsets::symmetric(
                        theme.metrics.gap * 2.0,
                        theme.metrics.gap * 1.5
                    ))
                    .child(
                        TextField::text(text_query)
                            .placeholder("sunset over water, a walk in the forest…")
                            .placeholder_color(theme.colors.on_surface_variant)
                            .color(theme.colors.on_surface)
                            .cursor(theme.colors.primary, 2.0)
                            .single_line()
                            .on_changed(Rc::new(move |value: TextEditingValue| {
                                text_state.text_query.set(value.text);
                            }))
                            .on_submit(Rc::new(move |_: String| submit_state.run_search())),
                    ),
                encoder_note(state, theme),
            ])
            .into(),

        SearchMode::Photo => Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(theme.metrics.gap)
            .children(children![
                Text::new(match &ref_photo {
                    Some(p) => format!("Finding photos that look like “{}”", p.label),
                    None => "Pick a photo to match against.".to_string(),
                })
                .color(theme.colors.on_surface_variant)
                .size(theme.text.body.size),
                reference_picker(state, theme, tokens, &ref_photo),
                encoder_note(state, theme),
            ])
            .into(),
    };

    let mut search_button = Button::new("Search");
    if can_search {
        search_button = search_button.on_pressed(move || run_state.run_search());
    }

    Container::new()
        .color(theme.colors.surface)
        .radius(tokens.sheet_curve)
        .shadow(Shadow {
            color: Color::rgba(0, 0, 0, 0x73),
            offset: Offset::new(0.0, -10.0),
            blur: 36.0,
            spread: 0.0,
            is_inset: false,
        })
        .padding(EdgeInsets::only(
            theme.metrics.gap * 2.5,
            theme.metrics.gap * 1.5,
            theme.metrics.gap * 2.5,
            theme.metrics.gap * 4.0,
        ))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(theme.metrics.gap * 1.5)
                .children(children![
                    // The grab handle the prototype has at the top of the
                    // sheet — the affordance that says this panel is a
                    // sheet and can be dismissed.
                    sheet_grip(state, theme),
                    Flex::row()
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Text::new("Find a photo")
                                .color(theme.colors.on_surface)
                                .size(theme.text.title.size)
                                .bold(),
                            Pressable::new({
                                let color = theme.colors.on_surface_variant;
                                move |press| {
                                    Container::new()
                                        .size(40.0, 40.0)
                                        .radius(f32::MAX)
                                        .color(Color::rgba(255, 255, 255, (press * 26.0) as u8))
                                        .alignment(Alignment::CENTER)
                                        .child(
                                            Icon::new(icons::close())
                                                .size(18.0)
                                                .color(color)
                                                .label("Close")
                                        )
                                        .into()
                                }
                            })
                            .on_tap(move || close_state.close_sheet()),
                        ]),
                    SegmentedControl::new(
                        ["Describe it", "Use a photo"],
                        match mode {
                            SearchMode::Text => 0,
                            SearchMode::Photo => 1,
                        }
                    )
                    .on_selected(Rc::new(move |i: usize| {
                        mode_state.set_mode(if i == 0 {
                            SearchMode::Text
                        } else {
                            SearchMode::Photo
                        });
                    })),
                    body,
                    Flex::row()
                        .main_axis_alignment(MainAxisAlignment::End)
                        .spacing(theme.metrics.gap)
                        .children(children![
                            Button::new("Cancel")
                                .style(ButtonStyle::Text)
                                .on_pressed(move || cancel_state.close_sheet()),
                            search_button,
                        ]),
                ]),
        )
        .into()
}

/// Which encoder is live, said plainly.
///
/// The two backends answer very differently — one understands subjects, the
/// other only colour — so which one is running is something the person
/// searching should be able to see, not a build-time detail buried in a
/// feature flag.
fn encoder_note(state: &AppState, theme: &Rc<ThemeData>) -> WidgetNode {
    let embedder = &state.embedder;
    let note = if embedder.is_semantic() {
        format!("Matching on meaning · {}", embedder.name())
    } else {
        format!("Matching on colour and composition · {}", embedder.name())
    };
    Text::new(note)
        .color(theme.colors.on_surface_variant)
        .size(theme.text.label.size)
        .into()
}

/// How many reference thumbnails per row, and how big.
///
/// A fixed grid rather than a horizontal `Scrollable`: a scrollable has no
/// height of its own, so nested in the sheet's column it collapsed to
/// nothing and the picker rendered as an invisible strip under its own
/// caption — the screenshots showed "Pick a photo to match against" with
/// no photos beneath it. Ten fixed choices need no scrolling anyway.
const PICKER_COLUMNS: usize = 5;
const PICKER_ROWS: usize = 2;
const PICKER_THUMB: f32 = 58.0;

fn reference_picker(
    state: &AppState,
    theme: &Rc<ThemeData>,
    tokens: &FluidTokens,
    selected: &Option<Photo>,
) -> WidgetNode {
    // There is no system photo picker to open — `vieww` has no such
    // service — so the reference is chosen from the library itself.
    let library = state.photos.peek();
    let thumbs: Vec<WidgetNode> = library
        .iter()
        .take(PICKER_COLUMNS * PICKER_ROWS)
        .map(|photo| {
            let is_selected = selected.as_ref().is_some_and(|p| p.id == photo.id);
            let pick = state.clone();
            let for_render = photo.clone();
            let for_tap = photo.clone();
            let primary = theme.colors.primary;
            // Copied, not borrowed: `Pressable`'s builder is `'static`, so
            // a `&FluidTokens` captured here would have to outlive the call.
            let curve = tokens.tile_curve * 0.7;
            Pressable::new(move |press| {
                let ring = if is_selected { 3.0 } else { 0.0 };
                Container::new()
                    .size(PICKER_THUMB, PICKER_THUMB)
                    .radius(curve)
                    .color(if is_selected {
                        primary
                    } else {
                        Color::rgba(255, 255, 255, (press * 40.0) as u8)
                    })
                    .padding(EdgeInsets::all(ring))
                    .child(
                        Clip::new(ClipShape::RRect {
                            radius: curve - ring,
                        })
                        .child(Image::new(for_render.image.clone()).fit(BoxFit::Cover)),
                    )
                    .into()
            })
            .on_tap(move || pick.ref_photo.set(Some(for_tap.clone())))
            .into()
        })
        .collect();

    let rows: Vec<WidgetNode> = thumbs
        .chunks(PICKER_COLUMNS)
        .map(|row| {
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .children(row.to_vec())
                .into()
        })
        .collect();

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(theme.metrics.gap)
        .children(rows)
        .into()
}

fn progress_dialog(
    state: &AppState,
    theme: &Rc<ThemeData>,
    tokens: &FluidTokens,
    encoder: &str,
) -> WidgetNode {
    // Only this card follows the search's progress: it ticks itself and reads
    // the value with `peek`, so the library behind it is not rebuilt.
    let _ = tokens;
    let (state, theme, tokens, encoder) =
        (state.clone(), theme.clone(), FluidTokens::new(), encoder.to_string());
    Ambient::new(true, move |_| {
        progress_card_body(&theme, &tokens, state.progress.peek().clamp(0.0, 1.0), &encoder)
    })
    .into()
}

fn progress_card_body(
    theme: &Rc<ThemeData>,
    tokens: &FluidTokens,
    progress: f32,
    encoder: &str,
) -> WidgetNode {

    Container::new()
        .color(theme.colors.surface)
        .radius(theme.metrics.corner * 1.2)
        .padding(EdgeInsets::all(theme.metrics.gap * 3.0))
        .shadow(Shadow {
            color: Color::rgba(0, 0, 0, 0x80),
            offset: Offset::new(0.0, 12.0),
            blur: 40.0,
            spread: 0.0,
            is_inset: false,
        })
        .child(
            Flex::column()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(theme.metrics.gap * 1.5)
                .children(children![
                    // The sparkle in its own gradient disc — the same fluid
                    // ramp the FAB carries, on the one dialog that IS the
                    // search. It says "the model is working" in the app's
                    // own visual language before a word is read.
                    Container::new()
                        .size(56.0, 56.0)
                        .radius(f32::MAX)
                        .gradient(tokens.fluid_gradient())
                        .shadow(Shadow {
                            color: Color::rgba(0x7c, 0x3a, 0xed, 0x73),
                            offset: Offset::new(0.0, 8.0),
                            blur: 22.0,
                            spread: 0.0,
                            is_inset: false,
                        })
                        .alignment(Alignment::CENTER)
                        .child(
                            Icon::new(app_icons::sparkle())
                                .size(26.0)
                                .color(Color::WHITE)
                                .label("Searching"),
                        ),
                    Text::new("Searching your library")
                        .color(theme.colors.on_surface)
                        .size(theme.text.title.size)
                        .bold(),
                    Text::new(encoder.to_string())
                        .color(theme.colors.on_surface_variant)
                        .size(theme.text.label.size),
                    SizedBox::from_size(Size::new(220.0, 6.0)).child(fluid_bar(tokens, progress)),
                    Text::new(format!("{}%", (progress * 100.0).round() as i32))
                        .color(theme.colors.on_surface_variant)
                        .size(theme.text.body.size),
                ]),
        )
        .into()
}

/// The progress bar as the fluid ramp, rather than the theme's flat primary.
///
/// `LinearProgress` paints its track and fill from `ColorScheme` roles and
/// has no gradient spelling; this is the same shape (rounded track, fill
/// sized by a `LayoutBuilder`) with the app's own ramp on the fill — the
/// one bar in the app that moves, carrying the one gradient the app owns.
fn fluid_bar(tokens: &FluidTokens, progress: f32) -> WidgetNode {
    // The ramp as a value, captured by the `'static` layout builder.
    let gradient = tokens.fluid_gradient();
    Container::new()
        .height(6.0)
        .radius(f32::MAX)
        .color(theme_track())
        .child(LayoutBuilder::new(move |constraints| {
            let width = if constraints.has_bounded_width() {
                constraints.max_width * progress
            } else {
                0.0
            };
            Align::new(Alignment::CENTER_LEFT)
                .child(
                    Container::new()
                        .width(width.max(0.0))
                        .height(6.0)
                        .radius(f32::MAX)
                        .gradient(gradient),
                )
                .into()
        }))
        .into()
}

/// The bar's track, one step off the surface so the fill reads as moving
/// *over* something rather than appearing out of the card.
fn theme_track() -> Color {
    Color::rgb(0x2e, 0x29, 0x42)
}

fn toast_pill(theme: &Rc<ThemeData>, message: String) -> WidgetNode {
    // `main_axis_size(Min)` on the wrapping row is what keeps this a pill
    // hugging its text rather than a full-width bar — the prototype's
    // `width: fit-content`.
    Flex::row()
        .main_axis_size(MainAxisSize::Min)
        .children(children![Container::new()
            .color(theme.colors.surface_variant)
            .radius(f32::MAX)
            .padding(EdgeInsets::symmetric(
                theme.metrics.gap * 2.5,
                theme.metrics.gap * 1.5
            ))
            .shadow(Shadow {
                color: Color::rgba(0, 0, 0, 0x66),
                offset: Offset::new(0.0, 6.0),
                blur: 20.0,
                spread: 0.0,
                is_inset: false,
            })
            .child(
                // A long query echoed back in "Nothing close to …" once made
                // this pill 420 of a 390-point screen. The cap lets the text
                // wrap to a second line instead — a stadium becomes a
                // lozenge, which is still a pill.
                Constrained::new(Constraints::new(0.0, 310.0, 0.0, f32::INFINITY)).child(
                    Text::new(message)
                        .color(theme.colors.on_surface)
                        .size(theme.text.body.size)
                        .align(TextAlign::Center),
                ),
            )])
        .into()
}

fn empty_state(theme: &Rc<ThemeData>, tokens: &FluidTokens) -> WidgetNode {
    // The illustration: three photo frames fanned behind a floating magnifier
    // disc, over a wide violet bloom. Built from the same tokens as everything
    // else — the frames carry the fluid ramp at low alpha, the disc is the
    // surface-raised glass, and the whole thing sits on the app's background —
    // so it reads as this app's empty state rather than a generic one.

    // A photo frame: a rounded plate with a soft tint and a hairline, rotated
    // about its own centre. `Transformed::rotate` turns about the child's
    // top-left, which would swing the whole plate off its slot —
    // `Transform::rotate_around` is the centre-pinned spelling, and it
    // composes the two translations itself.
    let frame = |rotate_deg: f32, tint: Color, w: f32, h: f32| {
        Transformed::new(Transform::rotate_around(
            Offset::new(w * 0.5, h * 0.5),
            rotate_deg.to_radians(),
        ))
        .child(
            Container::new()
                .size(w, h)
                .radius(18.0)
                .color(tint)
                .border(Border::new(Color::rgba(0xff, 0xff, 0xff, 0x2e), 1.0))
                .shadow(Shadow {
                    color: Color::rgba(0, 0, 0, 0x59),
                    offset: Offset::new(0.0, 10.0),
                    blur: 26.0,
                    spread: 0.0,
                    is_inset: false,
                }),
        )
    };

    let illustration = SizedBox::from_size(Size::new(220.0, 150.0)).child(
        Stack::new().fit(StackFit::Expand).children(children![
            // The bloom behind everything.
            Positioned::fill().child(
                Container::new().gradient(
                    Gradient::radial(Offset::new(0.5, 0.5), 0.5).with_stops(&[
                        (0.0, Color::rgba(0x7c, 0x3a, 0xed, 0x5e)),
                        (1.0, Color::rgba(0x7c, 0x3a, 0xed, 0x00)),
                    ]),
                ),
            ),
            // Back frame, tilted left.
            Positioned::new().left(8.0).top(6.0).child(
                frame(-8.0, Color::rgba(0x7c, 0x3a, 0xed, 0x66), 108.0, 132.0),
            ),
            // Back frame, tilted right.
            Positioned::new().right(8.0).top(6.0).child(
                frame(7.0, Color::rgba(0xff, 0x6b, 0x81, 0x5c), 108.0, 132.0),
            ),
            // Front frame, square on, carrying the amber end of the ramp.
            Positioned::new()
                .left(56.0)
                .top(16.0)
                .child(frame(0.0, Color::rgba(0xff, 0xb5, 0x45, 0x59), 108.0, 132.0)),
            // The magnifier disc, floating over the fan — the app's one glyph,
            // on the surface the FAB's hint pill uses.
            Positioned::new()
                .left(84.0)
                .top(44.0)
                .child(
                    Container::new()
                        .size(52.0, 52.0)
                        .radius(f32::MAX)
                        .color(tokens.surface_raised)
                        .border(Border::new(Color::rgba(0xff, 0xff, 0xff, 0x40), 1.5))
                        .shadow(Shadow {
                            color: Color::rgba(0, 0, 0, 0x66),
                            offset: Offset::new(0.0, 10.0),
                            blur: 24.0,
                            spread: 0.0,
                            is_inset: false,
                        })
                        .alignment(Alignment::CENTER)
                        .child(
                            Icon::new(app_icons::search())
                                .size(24.0)
                                .color(theme.colors.on_surface)
                                .label("Search photos"),
                        ),
                ),
        ]),
    );

    Align::new(Alignment::CENTER)
        .child(
            Padding::new(EdgeInsets::all(theme.metrics.gap * 4.0)).child(
                Flex::column()
                    .main_axis_size(MainAxisSize::Min)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(theme.metrics.gap * 1.5)
                    .children(children![
                        illustration,
                        Text::new("Nothing close to that")
                            .color(theme.colors.on_surface)
                            .size(theme.text.title.size)
                            .bold(),
                        Text::new("Try describing it differently, or match against a photo.")
                            .color(theme.colors.on_surface_variant)
                            .align(TextAlign::Center),
                    ]),
            ),
        )
        .into()
}

widget_node_from!(LibraryScreen);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_follows_the_finger_down_and_not_up() {
        assert_eq!(sheet_drag_after(0.0, -30.0), 0.0, "cannot be pulled above rest");
        assert_eq!(sheet_drag_after(40.0, 25.0), 65.0);
        assert_eq!(sheet_drag_after(450.0, 100.0), SHEET_TRAVEL);
    }

    #[test]
    fn a_long_drag_or_a_fast_fling_dismisses_and_a_short_one_does_not() {
        assert!(should_dismiss_sheet(SHEET_DISMISS_PX, 0.0));
        assert!(should_dismiss_sheet(10.0, SHEET_FLING_PX_PER_S));
        assert!(!should_dismiss_sheet(30.0, 200.0));
        assert!(!should_dismiss_sheet(30.0, -2000.0), "an upward fling never dismisses");
    }

    #[test]
    fn the_header_says_what_is_missing_from_the_library() {
        assert_eq!(issue_note((0, 0)), None);
        assert_eq!(issue_note((12, 0)).unwrap(), "12 OVER THE LIMIT");
        assert_eq!(issue_note((0, 3)).unwrap(), "3 COULD NOT BE READ");
        assert!(issue_note((5, 2)).unwrap().contains("UNREADABLE"));
    }
}
