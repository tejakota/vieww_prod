//! The home screen: the body, the floating search, and the answer card
//! that rides with it.
//!
//! # The layout, and why it is a stack and not a column
//!
//! The 3D body owns the *whole* screen; the search floats over it as one
//! glass card — input on top, results or the open answer below — rather
//! than sitting in a band of its own with its own background. That fusion
//! is the point: the card is part of the scene's chrome, so it *moves* in
//! the scene's terms. The pose's `pill` spring carries the rule: when the
//! answer lives low in the body (kidneys, knees) the card rides high and
//! the camera frames the organ low; when it lives high (brain, eyes) the
//! card drops low and the organ is framed high. The answer and the question
//! never fight for the same pixels.
//!
//! # The answer is text; the picture is the body
//!
//! The answer card carries no images. Each organ's illustration has been
//! grown into the organ's 3D element inside the body (`anatomy.rs`), and the
//! camera flies to that element — so the picture the answer is about is the
//! one the person can turn, zoom and pan, not a flat figure in the card.
//!
//! # Touch
//!
//! On the body: tap picks a region; drag orbits; pinch zooms; a pinch's
//! moving midpoint pans; the mouse wheel zooms. The round buttons top-right
//! do the same in steps, and the target button brings the framing back.
//!
//! # Type
//!
//! Nothing is set below 11 px, and every tappable thing has a 44 px target
//! (DESIGN_SPEC.md).

use std::cell::Cell;
use std::rc::Rc;

use vieww::element::Runtime;
use vieww::foundation::{
    Color, EdgeInsets, FontWeight, Offset, Shadow, Size, TextEditingValue, TextStyle,
};
use vieww::prelude::*;
use vieww::widget::{icons, DrawOnce};

use crate::{
    anatomy, clear, db, go_back, pick_body, run_search, safety, search, search_for, select_topic,
    AppState,
};

// ---------------------------------------------------------------------------
// Tokens (DESIGN_SPEC.md §1)
// ---------------------------------------------------------------------------

/// Eyebrows and tags.
const MICRO: f32 = 11.0;
/// Meta lines and citations.
const CAPTION: f32 = 12.0;
/// Article text.
const BODY: f32 = 14.0;
/// A result's title, the input.
const ROW: f32 = 15.0;
/// The open answer's title.
const TITLE: f32 = 20.0;
/// The smallest a tappable thing may be.
const HIT: f32 = 44.0;

fn style(size: f32, weight: FontWeight, color: Color, spacing: f32) -> TextStyle {
    TextStyle::new(size)
        .weight(weight)
        .color(color)
        .letter_spacing(spacing)
}

/// A no-content widget, for branches that have nothing to show.
#[derive(Debug)]
struct Nothing;

impl Widget for Nothing {
    fn debug_name(&self) -> &'static str {
        "Nothing"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        SizedBox::height(0.0).into()
    }
}

fn nothing() -> WidgetNode {
    Nothing.into()
}

vieww::widget_node_from!(Nothing);

// ---------------------------------------------------------------------------
// The screen
// ---------------------------------------------------------------------------

/// The home screen, stateful: the app's real root.
#[must_use]
pub fn root(state: &Rc<AppState>) -> WidgetNode {
    BodyScreen {
        state: state.clone(),
    }
    .into()
}

/// The screen contract the studio's render looks for: a fresh state, the
/// resting body.
#[must_use]
pub fn screen() -> impl Widget {
    let runtime = Runtime::new();
    let state = crate::build_state(&runtime);
    BodyScreen { state }
}

struct BodyScreen {
    state: Rc<AppState>,
}

impl std::fmt::Debug for BodyScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BodyScreen").finish_non_exhaustive()
    }
}

vieww::widget_node_from!(BodyScreen);

impl Widget for BodyScreen {
    fn debug_name(&self) -> &'static str {
        "BodyScreen"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let state = self.state.clone();
        let pose = state.pose.get();

        LayoutBuilder::new(move |constraints| {
            let big = constraints.biggest();
            let (vw, vh) = (big.width, big.height);
            if vw > 1.0 && vh > 1.0 && state.viewport.get() != (vw, vh) {
                state.viewport.set((vw, vh));
            }

            Stack::new()
                .children(children![
                    // The 3D body owns the full screen — the card floats
                    // over it, it is never handed a band of its own.
                    viewport(&state, &pose, vw, vh),
                    top_bar(&state, vw),
                    camera_controls(&state, pose.pill, vw, vh),
                    floating_card(&state, &pose, vw, vh),
                ])
                .into()
        })
        .into()
    }
}

// ---------------------------------------------------------------------------
// The viewport
// ---------------------------------------------------------------------------

fn viewport(state: &Rc<AppState>, pose: &anatomy::Pose, vw: f32, vh: f32) -> WidgetNode {
    // The internal buffer: a fraction of the logical size, width-capped,
    // and a fifth smaller again in an x-ray close-up — there the see-through
    // shell covers every pixel, and the frame must still fit 60 Hz.
    let scale = anatomy::RENDER_SCALE * (1.0 - 0.2 * pose.xray);
    let bw = ((vw * scale) as u32)
        .min(anatomy::MAX_INTERNAL_WIDTH)
        .max(2);
    let bh = ((bw as f32) * vh / vw.max(1.0)) as u32;

    let image = anatomy::render_frame(
        &mut state.scene.borrow_mut(),
        &state.parts,
        &state.placements,
        pose,
        state.selected.get(),
        bw,
        bh,
        Some(&state.stats),
    );

    // A pinch reports its spread relative to its start; the rig wants the
    // change since the last update.
    let last_scale = Rc::new(Cell::new(1.0_f32));

    GestureDetector::new()
        .on_tap({
            let state = state.clone();
            move |tap| pick_body(&state, tap.local.dx, tap.local.dy)
        })
        .on_drag_update({
            let state = state.clone();
            move |d| {
                let mut rig = state.rig.borrow_mut();
                rig.orbit(d.delta.dx, d.delta.dy);
                rig.publish();
            }
        })
        .on_scale_start({
            let last = last_scale.clone();
            move |_s| last.set(1.0)
        })
        .on_scale_update({
            let state = state.clone();
            let last = last_scale.clone();
            move |s| {
                let step = s.scale / last.get().max(0.01);
                last.set(s.scale);
                let mut rig = state.rig.borrow_mut();
                rig.zoom_by(step);
                rig.pan(s.focal_delta.dx, s.focal_delta.dy);
                rig.publish();
            }
        })
        .on_scroll({
            let state = state.clone();
            move |e| {
                // A wheel notch (~50 px of content) is a 10% step.
                let mut rig = state.rig.borrow_mut();
                rig.zoom_by((1.0 + e.delta.dy * 0.002).clamp(0.5, 2.0));
                rig.publish();
            }
        })
        .child(Stack::new().children(children![
                Image::new(image)
                    .width(vw)
                    .height(vh)
                    .fit(vieww::foundation::BoxFit::Fill)
                    .label("The body. Tap a region to open it; drag to turn, pinch to zoom."),
                // A symmetric cool wash — the card can ride high or low, so
                // both ends of the scene need somewhere to sit.
                Container::new()
                    .decoration(
                        BoxDecoration::new().gradient(
                            vieww::foundation::Gradient::vertical().with_stops(&[
                                (0.0, Color::rgba(13, 20, 34, 150)),
                                (0.18, Color::rgba(10, 14, 22, 0)),
                                (0.80, Color::rgba(8, 11, 18, 0)),
                                (1.0, Color::rgba(8, 11, 18, 170)),
                            ]),
                        ),
                    )
                    .width(vw)
                    .height(vh),
                organ_callout(state, pose, vw, vh),
            ]))
        .into()
}

/// The brand and the corpus line, floating at the top.
fn top_bar(state: &Rc<AppState>, vw: f32) -> WidgetNode {
    let topics = state.db.topics.len();
    Positioned::new()
        .left(20.0)
        .top(16.0)
        .width((vw - 40.0 - HIT - 8.0).max(60.0))
        .child(
            Flex::column()
                .spacing(2.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Text::new("SOMA").style(style(22.0, FontWeight::Bold, crate::INK, 5.0)),
                    Text::new(format!("{} health topics · offline", group(topics))).style(style(
                        CAPTION,
                        FontWeight::Medium,
                        crate::MUTED,
                        0.4
                    )),
                ]),
        )
        .into()
}

/// `11043` -> `11,043`.
fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The camera's round buttons, top-right: zoom in and out, turn left and
/// right a quarter, and — once the person has moved the camera — recenter.
fn camera_controls(state: &Rc<AppState>, pill: f32, vw: f32, vh: f32) -> WidgetNode {
    let moved = state.rig.borrow().camera_is_user();
    let button =
        |child: WidgetNode, label: &'static str, act: Rc<dyn Fn(&AppState)>| -> WidgetNode {
            let state = state.clone();
            Semantics::button(label)
                .child(
                    GestureDetector::new()
                        .on_tap(move |_| {
                            act(&state);
                            let mut rig = state.rig.borrow_mut();
                            rig.poke();
                            rig.publish();
                        })
                        .child(
                            SizedBox::square(HIT).child(
                                Center::new().child(
                                    Container::new()
                                        .decoration(
                                            BoxDecoration::new()
                                                .color(crate::CARD_GLASS)
                                                .radius(18.0)
                                                .border(vieww::foundation::Border::new(
                                                    crate::EDGE,
                                                    1.0,
                                                )),
                                        )
                                        .width(36.0)
                                        .height(36.0)
                                        .child(Center::new().child(child)),
                                ),
                            ),
                        ),
                )
                .into()
        };
    let icon = |data| -> WidgetNode { Icon::new(data).size(18.0).color(crate::INK).into() };

    let mut column: Vec<WidgetNode> = vec![
        button(
            icon(icons::add()),
            "Zoom in",
            Rc::new(|s: &AppState| s.rig.borrow_mut().step_zoom(1.35)),
        ),
        button(
            icon(icons::remove()),
            "Zoom out",
            Rc::new(|s: &AppState| s.rig.borrow_mut().step_zoom(1.0 / 1.35)),
        ),
        button(
            icon(icons::chevron_left()),
            "Turn left",
            Rc::new(|s: &AppState| s.rig.borrow_mut().turn_by(-std::f32::consts::FRAC_PI_2)),
        ),
        button(
            icon(icons::chevron_right()),
            "Turn right",
            Rc::new(|s: &AppState| s.rig.borrow_mut().turn_by(std::f32::consts::FRAC_PI_2)),
        ),
    ];
    if moved {
        column.push(button(
            recenter_glyph(),
            "Recenter",
            Rc::new(|s: &AppState| s.rig.borrow_mut().recenter()),
        ));
    }

    // Top-right while the card rests low; bottom-right once it rides high,
    // so the card never covers them.
    let column_h = column.len() as f32 * (HIT + 2.0);
    let high = pill > 0.5;
    let top = if high { vh - column_h - 12.0 } else { 12.0 };
    // Bottom-left once the card rides high: the organ's callout sits
    // below the organ, near the middle, and must not land on them.
    let left = if high { 10.0 } else { vw - HIT - 10.0 };
    Positioned::new()
        .left(left)
        .top(top)
        .child(Flex::column().spacing(2.0).children(column))
        .into()
}

/// A target: ring and centre dot.
fn recenter_glyph() -> WidgetNode {
    CustomPaint::sized(
        Size::new(18.0, 18.0),
        DrawOnce::new(|_size| {
            use vieww::widget::DrawInstruction::*;
            vec![
                FillCircle {
                    center: Offset::new(9.0, 9.0),
                    radius: 7.5,
                    color: crate::INK,
                },
                FillCircle {
                    center: Offset::new(9.0, 9.0),
                    radius: 5.8,
                    color: crate::CARD_GLASS,
                },
                FillCircle {
                    center: Offset::new(9.0, 9.0),
                    radius: 2.6,
                    color: crate::ACCENT,
                },
            ]
        }),
    )
    .into()
}

/// A label pinned to the targeted organ's projected screen position. It
/// names the organ, the answer it is pinned for and the words that matched
/// — the citation of *this* query, not a generic source line.
fn organ_callout(state: &Rc<AppState>, pose: &anatomy::Pose, vw: f32, vh: f32) -> WidgetNode {
    let Some(organ_index) = state.selected.get() else {
        return nothing();
    };
    let organ = &state.db.organs[organ_index];
    let opacity = ((pose.xray - 0.2) / 0.5).clamp(0.0, 1.0);
    if opacity < 0.08 {
        return nothing();
    }
    let at = state.placements[organ_index].anchor;
    let Some((fx, fy, depth)) = anatomy::project(&pose.camera, vw, vh, at) else {
        return nothing();
    };
    if depth <= 0.0 || fx < -20.0 || fx > vw + 20.0 || fy < -20.0 || fy > vh + 20.0 {
        return nothing();
    }

    // What the callout cites: the open answer, else the leading result.
    let topic_index = state
        .topic
        .get()
        .or_else(|| state.results.get().first().map(|h| h.topic));
    let topic = topic_index.and_then(|i| state.db.topics.get(i));
    let terms: Vec<String> = if state.topic.get().is_some() {
        state.answer_terms.get()
    } else {
        search::tokenize(&state.query.get().text)
    };
    let why = match topic {
        Some(t) if !terms.is_empty() => {
            format!("“{}” · matched {}", short(t.title, 28), terms.join(", "))
        }
        Some(t) => format!("“{}”", short(t.title, 34)),
        None => String::new(),
    };
    let source = topic.map(|t| host(t.url)).unwrap_or_default();

    // The fade is baked into the colours rather than wrapped in an
    // `Opacity`: a `Positioned` must be the Stack's direct child to be
    // positioned at all, and one widget of wrapping would hide it.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let a = |v: u8| ((v as f32 * opacity) as u8).min(v);
    let mut lines: Vec<WidgetNode> = vec![Flex::row()
        .spacing(7.0)
        .children(children![
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(crate::ACCENT.with_alpha(a(255)))
                        .radius(4.0)
                )
                .width(8.0)
                .height(8.0),
            Text::new(organ.name.to_owned()).style(style(
                13.0,
                FontWeight::Bold,
                crate::INK.with_alpha(a(255)),
                0.3
            )),
        ])
        .into()];
    if !why.is_empty() {
        lines.push(
            Text::new(why)
                .style(style(
                    MICRO,
                    FontWeight::Medium,
                    crate::INK.with_alpha(a(220)),
                    0.1,
                ))
                .into(),
        );
    }
    if !source.is_empty() {
        lines.push(
            Text::new(source)
                .style(style(
                    MICRO,
                    FontWeight::Regular,
                    crate::MUTED.with_alpha(a(230)),
                    0.2,
                ))
                .into(),
        );
    }
    let chip = Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(10, 16, 26, a(225)))
                .radius(12.0)
                .border(vieww::foundation::Border::new(
                    crate::ACCENT.with_alpha(a(140)),
                    1.0,
                ))
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, a(120)),
                    Offset::new(0.0, 4.0),
                    14.0,
                )),
        )
        .padding(EdgeInsets::symmetric(10.0, 8.0))
        .child(
            Flex::column()
                .spacing(2.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(lines),
        );

    // The callout sits on the organ's side away from the card: below it
    // when the card rides high, above it when the card rests low. The
    // leader line always points at the organ.
    let width = (vw * 0.62).min(260.0);
    let leader: WidgetNode = Padding::new(EdgeInsets::only(23.0, 0.0, 0.0, 0.0))
        .child(
            Container::new()
                .decoration(BoxDecoration::new().color(crate::ACCENT.with_alpha(a(110))))
                .width(1.5)
                .height(24.0),
        )
        .into();
    let below = pose.pill > 0.5;
    let (top, column) = if below {
        ((fy + 6.0).min(vh - 96.0), children![leader, chip])
    } else {
        ((fy - 98.0).max(70.0), children![chip, leader])
    };
    Positioned::new()
        .left((fx - 24.0).clamp(8.0, (vw - width - 8.0).max(8.0)))
        .top(top)
        .width(width)
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(column),
        )
        .into()
}

/// The first `n` characters of `s`, with an ellipsis when cut.
fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// `https://my.example.org/a/b` -> `my.example.org`.
fn host(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_owned()
}

// ---------------------------------------------------------------------------
// The floating card
// ---------------------------------------------------------------------------

/// The floating search card: the pill on top, then suggestions, results or
/// the open answer, in one glass sheet that rides the `pill` spring.
///
/// At rest (`panel = 0`) it is the pill and a row of suggestions, low over
/// the body's feet. Alive (`panel = 1`) it grows to hold the answer, and
/// `pill` decides which end of the scene it floats toward.
fn floating_card(state: &Rc<AppState>, pose: &anatomy::Pose, vw: f32, vh: f32) -> WidgetNode {
    let query = state.query.get();
    let has_query = !query.text.trim().is_empty();
    let topic_open = state.topic.get().is_some();

    let collapsed = 112.0_f32.min(vh * 0.18);
    let expanded = if topic_open {
        (vh * 0.56).min(540.0)
    } else {
        (vh * 0.46).min(450.0)
    };
    let card_h = collapsed + (expanded - collapsed) * pose.panel;

    // Where the card floats: `pill` 0 parks it at the bottom edge, 1 lifts
    // it under the brand line, and the spring glides between.
    let low_top = (vh - card_h - 16.0).max(76.0);
    let high_top = 76.0;
    let top = low_top + (high_top - low_top) * pose.pill;

    let body: WidgetNode = if topic_open {
        answer_card(state)
    } else if has_query {
        results_card(state)
    } else {
        hint_row(state)
    };

    Positioned::new()
        .left(12.0)
        .top(top)
        .width(vw - 24.0)
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(crate::CARD_GLASS)
                        .radius(24.0)
                        .border(vieww::foundation::Border::new(crate::EDGE, 1.0))
                        .shadow(Shadow::new(
                            Color::rgba(0, 0, 0, 120),
                            Offset::new(0.0, 16.0),
                            34.0,
                        )),
                )
                .padding(EdgeInsets::symmetric(14.0, 10.0))
                .height(card_h)
                .child(
                    Flex::column()
                        .spacing(8.0)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .children(children![search_pill(state), Flexible::new(1).child(body)]),
                ),
        )
        .into()
}

/// The pill: the search field, the mode badge, and the clear button.
fn search_pill(state: &Rc<AppState>) -> WidgetNode {
    let value = state.query.get();
    let non_empty = !value.text.trim().is_empty() || state.topic.get().is_some();
    let regex_on = regex_active(state);

    SizedBox::height(48.0)
        .child(
            Flex::row()
                .spacing(10.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    search_icon(),
                    Flexible::new(1).child(
                        TextField::new(value)
                            .size(ROW)
                            .color(crate::INK)
                            .placeholder("Ask about a symptom, organ or condition")
                            .placeholder_color(crate::MUTED)
                            .on_changed(Rc::new({
                                let state = state.clone();
                                move |next: TextEditingValue| {
                                    state.query.set(next);
                                    run_search(&state);
                                }
                            })),
                    ),
                    if non_empty && regex_on {
                        badge("REGEX", crate::ACCENT)
                    } else {
                        nothing()
                    },
                    if non_empty {
                        clear_button(state)
                    } else {
                        nothing()
                    },
                ]),
        )
        .into()
}

fn regex_active(state: &Rc<AppState>) -> bool {
    let q = state.query.get();
    let text = q.text.trim();
    !text.is_empty() && crate::search::looks_like_regex(text) && !state.regex_fallback.get()
}

fn search_icon() -> WidgetNode {
    // A magnifier: a ring (outer disc in icon grey, inner disc in the
    // card's own background) and a handle line.
    CustomPaint::sized(
        Size::new(18.0, 18.0),
        DrawOnce::new(|_size| {
            use vieww::widget::DrawInstruction::*;
            vec![
                FillCircle {
                    center: Offset::new(7.4, 7.4),
                    radius: 5.2,
                    color: crate::MUTED,
                },
                FillCircle {
                    center: Offset::new(7.4, 7.4),
                    radius: 3.6,
                    color: crate::CARD_GLASS,
                },
                DrawLine {
                    from: Offset::new(11.2, 11.2),
                    to: Offset::new(15.6, 15.6),
                    color: crate::MUTED,
                    width: 1.8,
                },
            ]
        }),
    )
    .into()
}

fn clear_button(state: &Rc<AppState>) -> WidgetNode {
    Semantics::button("Clear search")
        .child(
            GestureDetector::new()
                .on_tap({
                    let state = state.clone();
                    move |_tap| clear(&state)
                })
                .child(
                    SizedBox::square(HIT).child(
                        Center::new().child(
                            Container::new()
                                .decoration(
                                    BoxDecoration::new()
                                        .color(Color::rgba(255, 255, 255, 26))
                                        .radius(14.0),
                                )
                                .width(28.0)
                                .height(28.0)
                                .child(
                                    Center::new().child(
                                        Icon::new(icons::close()).size(15.0).color(crate::INK),
                                    ),
                                ),
                        ),
                    ),
                ),
        )
        .into()
}

fn badge(label: &str, color: Color) -> WidgetNode {
    Container::new()
        .decoration(BoxDecoration::new().color(color.with_alpha(40)).radius(6.0))
        .padding(EdgeInsets::symmetric(6.0, 3.0))
        .child(Text::new(label.to_owned()).style(style(MICRO, FontWeight::Bold, color, 0.8)))
        .into()
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// The resting card's suggestions: where to start, one tap each.
fn hint_row(state: &Rc<AppState>) -> WidgetNode {
    let chips: Vec<WidgetNode> = [
        "chest pain",
        "tired all the time",
        "kidney stones",
        "migraine",
    ]
    .into_iter()
    .map(|q| {
        chip(
            state,
            q,
            crate::MUTED,
            Rc::new(move |s: &AppState| search_for(s, q)),
        )
    })
    .collect();
    Scrollable::horizontal(state.panel_scroll.offset())
        .child(Flex::row().spacing(6.0).children(chips))
        .into()
}

/// A rounded, tappable label with a 44 px target.
fn chip(state: &Rc<AppState>, label: &str, color: Color, act: Rc<dyn Fn(&AppState)>) -> WidgetNode {
    let state = state.clone();
    GestureDetector::new()
        .on_tap(move |_| act(&state))
        .child(
            SizedBox::height(HIT).child(
                Center::new().child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .color(color.with_alpha(26))
                                .radius(10.0)
                                .border(vieww::foundation::Border::new(color.with_alpha(76), 1.0)),
                        )
                        .padding(EdgeInsets::symmetric(12.0, 8.0))
                        .child(Text::new(label.to_owned()).style(style(
                            CAPTION,
                            FontWeight::Medium,
                            color,
                            0.2,
                        ))),
                ),
            ),
        )
        .into()
}

/// The emergency card a red-flag query shows above everything else.
fn alert_card(alert: safety::Alert) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(crate::CORAL.with_alpha(34))
                .radius(12.0)
                .border(vieww::foundation::Border::new(
                    crate::CORAL.with_alpha(150),
                    1.0,
                )),
        )
        .padding(EdgeInsets::symmetric(12.0, 10.0))
        .child(
            Flex::column()
                .spacing(3.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Text::new(alert.title.to_owned()).style(style(
                        13.0,
                        FontWeight::Bold,
                        crate::CORAL,
                        0.2
                    )),
                    Text::new(alert.action.to_owned()).style(style(
                        CAPTION,
                        FontWeight::Medium,
                        crate::INK,
                        0.1
                    )),
                ]),
        )
        .into()
}

fn disclaimer() -> WidgetNode {
    Text::new(safety::DISCLAIMER.to_owned())
        .style(style(MICRO, FontWeight::Regular, crate::MUTED, 0.1))
        .into()
}

/// How a hit was found, as a small colored tag for its row.
fn found_tags(flags: u8) -> Vec<WidgetNode> {
    let mut out: Vec<WidgetNode> = Vec::new();
    if flags & crate::search::found::LITERAL != 0 {
        out.push(badge("TEXT", crate::MUTED));
    }
    if flags & crate::search::found::SEMANTIC != 0 {
        out.push(badge("MEANING", crate::ACCENT));
    }
    if flags & crate::search::found::GRAPH != 0 {
        out.push(badge("NEARBY", crate::CORAL));
    }
    if flags & crate::search::found::EXPANDED != 0 {
        out.push(badge("RELATED", Color::rgb(122, 162, 247)));
    }
    out
}

/// The results list, live as you type — every hit, scrollable.
fn results_card(state: &Rc<AppState>) -> WidgetNode {
    let results = state.results.get();
    let fell_back = state.regex_fallback.get();
    let alert = safety::red_flag(&state.query.get().text);

    let header = if fell_back {
        Text::new("Pattern not valid, searching the words instead".to_owned()).style(style(
            CAPTION,
            FontWeight::Medium,
            crate::CORAL,
            0.2,
        ))
    } else {
        Text::new(format!("{} answers, best first", results.len())).style(style(
            CAPTION,
            FontWeight::Medium,
            crate::MUTED,
            0.3,
        ))
    };

    let mut rows: Vec<WidgetNode> = Vec::new();
    if let Some(a) = alert {
        rows.push(alert_card(a));
    }
    rows.push(header.into());
    if results.is_empty() {
        rows.push(
            Text::new(
                "Nothing matches that yet. Try another spelling, a broader word, or tap the body.",
            )
            .style(style(BODY, FontWeight::Regular, crate::MUTED, 0.1))
            .into(),
        );
    } else {
        rows.extend(results.iter().map(|hit| result_row(state, hit)));
    }
    rows.push(disclaimer());

    Scrollable::vertical(state.panel_scroll.offset())
        .child(
            Flex::column()
                .spacing(4.0)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(rows),
        )
        .into()
}

fn result_row(state: &Rc<AppState>, hit: &crate::search::Hit) -> WidgetNode {
    let Some(topic) = state.db.topics.get(hit.topic) else {
        return nothing();
    };
    let organ = topic
        .organ
        .map(|i| state.db.organs[i].name)
        .unwrap_or("General");
    let index = hit.topic;
    // Kind and organ on one line, how it was found on the next: three tags
    // beside the meta line overflowed a phone's row.
    let tags = found_tags(hit.flags);

    Semantics::button(format!(
        "{}, {} in {}",
        topic.title,
        topic.kind_name(),
        organ
    ))
    .child(
        GestureDetector::new()
            .on_tap({
                let state = state.clone();
                move |_tap| select_topic(&state, index)
            })
            .child(
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .color(Color::rgba(255, 255, 255, 10))
                            .radius(12.0),
                    )
                    .padding(EdgeInsets::symmetric(10.0, 9.0))
                    .child(
                        Flex::row().spacing(10.0).children(children![
                            kind_badge(topic.kind),
                            Flexible::new(1).child(
                                Flex::column()
                                    .spacing(3.0)
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .children(children![
                                        Text::new(topic.title.to_owned()).style(style(
                                            ROW,
                                            FontWeight::Medium,
                                            crate::INK,
                                            0.0
                                        )),
                                        Text::new(format!("{} · {}", topic.kind_name(), organ))
                                            .style(style(
                                                CAPTION,
                                                FontWeight::Regular,
                                                crate::MUTED,
                                                0.2
                                            )),
                                        Flex::row().spacing(5.0).children(tags),
                                    ]),
                            ),
                        ]),
                    ),
            ),
    )
    .into()
}

fn kind_color(kind: char) -> Color {
    match kind {
        'B' => crate::ACCENT,
        'S' | 'D' => crate::CORAL,
        'X' => Color::rgb(122, 162, 247),
        'T' | 'P' => Color::rgb(240, 196, 110),
        'M' => Color::rgb(184, 122, 169),
        _ => crate::MUTED,
    }
}

fn kind_badge(kind: char) -> WidgetNode {
    // One distinct letter per kind (procedures used to show as "T").
    let letter = match kind {
        'B' | 'S' | 'D' | 'X' | 'T' | 'P' | 'M' => kind.to_string(),
        _ => "G".to_owned(),
    };
    let color = kind_color(kind);
    Container::new()
        .decoration(BoxDecoration::new().color(color.with_alpha(40)).radius(8.0))
        .width(28.0)
        .height(28.0)
        .child(Center::new().child(Text::new(letter).style(style(
            13.0,
            FontWeight::Bold,
            color,
            0.3,
        ))))
        .into()
}

/// The open answer, as text: where it is in the body, the passage that
/// answers the words asked, the full article, what is nearby, and whose
/// words these are. No images — the organ itself is in the body above.
fn answer_card(state: &Rc<AppState>) -> WidgetNode {
    let Some(index) = state.topic.get() else {
        return nothing();
    };
    let Some(topic) = state.db.topics.get(index) else {
        return nothing();
    };
    let organ_line = topic
        .organ
        .map(|i| state.db.organs[i].name)
        .unwrap_or("General");
    let terms = state.answer_terms.get();
    let alert =
        safety::red_flag(&terms.join(" ")).or_else(|| safety::red_flag(&state.query.get().text));

    let body = state.db.body(index);
    let mut article = body.map_or_else(|| topic.description.to_owned(), str::to_owned);
    // An article's first heading usually restates the title ("Heart" ->
    // "What is the heart?"); showing both is a stutter, so drop it.
    {
        let title_lower = topic.title.to_lowercase();
        let restates = article
            .lines()
            .next()
            .and_then(|first| first.strip_prefix("## "))
            .is_some_and(|h| {
                let h = h.to_lowercase();
                h.contains(&title_lower) || title_lower.contains(h.trim_end_matches('?'))
            });
        if restates {
            article = article
                .split_once('\n')
                .map(|(_, rest)| rest.trim_start())
                .unwrap_or(&article)
                .to_owned();
        }
    }
    if article.trim().is_empty() {
        article = format!(
            "There is no article for this yet. Turn the body to explore the {}, or search for a condition that affects it.",
            organ_line.to_lowercase()
        );
    }
    // The matched line, unless it is the article's own opening line —
    // then the article already leads with it.
    let opening = article
        .lines()
        .map(str::trim)
        .find(|l| l.len() >= 30 && !l.starts_with('#'))
        .unwrap_or("");
    let passage = matched_passage(&article, &terms).filter(|p| {
        let plain: String = p.chars().filter(|c| *c != '*').collect();
        !opening.starts_with(plain.trim_end_matches('…').trim())
    });
    let article = flatten_headings(&article);

    let mut blocks: Vec<WidgetNode> = Vec::new();
    if let Some(a) = alert {
        blocks.push(alert_card(a));
    }
    // Header: back, kind, organ, then the title.
    blocks.push(
        Flex::row()
            .spacing(8.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children![
                back_button(state),
                kind_badge(topic.kind),
                Flexible::new(1).child(
                    Text::new(format!("{} · {}", db::kind_name(topic.kind), organ_line))
                        .style(style(CAPTION, FontWeight::Bold, crate::ACCENT, 0.6)),
                ),
            ])
            .into(),
    );
    blocks.push(
        Text::new(topic.title.to_owned())
            .style(style(TITLE, FontWeight::Bold, crate::INK, -0.2))
            .into(),
    );
    if let Some(p) = passage {
        blocks.push(
            Flex::row()
                .spacing(10.0)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(children![
                    Container::new()
                        .decoration(BoxDecoration::new().color(crate::ACCENT).radius(2.0))
                        .width(3.0),
                    Flexible::new(1).child(Markdown::new(p)),
                ])
                .into(),
        );
    }
    blocks.push(Markdown::new(article).into());

    let related = related_topics(state, index);
    if !related.is_empty() {
        blocks.push(
            Flex::column()
                .spacing(2.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Text::new("NEARBY IN THE BODY".to_owned()).style(style(
                        MICRO,
                        FontWeight::Bold,
                        crate::MUTED,
                        1.4
                    )),
                    Flex::row().spacing(6.0).children(related),
                ])
                .into(),
        );
    }
    blocks.push(source_line(topic));
    blocks.push(disclaimer());
    blocks.push(SizedBox::height(8.0).into());

    Stack::new()
        .children(children![
            Scrollable::vertical(state.answer_scroll.offset()).child(
                Flex::column()
                    .spacing(10.0)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(blocks),
            ),
            // The fade says "there is more below" without stealing the touch.
            Positioned::new().left(0.0).right(0.0).bottom(0.0).child(
                IgnorePointer::new().child(
                    Container::new()
                        .decoration(BoxDecoration::new().gradient(
                            vieww::foundation::Gradient::vertical().with_stops(&[
                                (0.0, Color::rgba(16, 22, 35, 0)),
                                (1.0, Color::rgba(16, 22, 35, 200)),
                            ]),
                        ))
                        .height(22.0),
                )
            ),
        ])
        .into()
}

fn back_button(state: &Rc<AppState>) -> WidgetNode {
    Semantics::button("Back")
        .child(
            GestureDetector::new()
                .on_tap({
                    let state = state.clone();
                    move |_| go_back(&state)
                })
                .child(
                    SizedBox::square(HIT).child(
                        Center::new().child(
                            Container::new()
                                .decoration(
                                    BoxDecoration::new()
                                        .color(Color::rgba(255, 255, 255, 18))
                                        .radius(14.0),
                                )
                                .width(28.0)
                                .height(28.0)
                                .child(
                                    Center::new().child(
                                        Icon::new(icons::chevron_left())
                                            .size(18.0)
                                            .color(crate::INK),
                                    ),
                                ),
                        ),
                    ),
                ),
        )
        .into()
}

/// The line of `article` that carries the most query terms, with those
/// words in bold — the sentence that answers the question asked, shown
/// before the article it comes from. The corpus breaks paragraphs with
/// single newlines, so a line is a paragraph; headings and table rows are
/// skipped, and a long paragraph is cut at a sentence end near 320
/// characters. `None` without terms or a hit.
#[must_use]
pub fn matched_passage(article: &str, terms: &[String]) -> Option<String> {
    let terms: Vec<&str> = terms
        .iter()
        .map(String::as_str)
        .filter(|t| t.len() >= 3)
        .collect();
    if terms.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    for para in article.lines() {
        let p = para.trim();
        if p.len() < 30 || p.starts_with('#') || p.starts_with('|') {
            continue;
        }
        let lower = p.to_lowercase();
        let score = terms.iter().filter(|t| lower.contains(*t)).count();
        if score > 0 && best.is_none_or(|(s, _)| score > s) {
            best = Some((score, p));
        }
    }
    let (_, para) = best?;
    let para = cut_at_sentence(para, 320);
    // Bold each word that begins with a query term (stems: "ston" marks
    // "stones"). Word by word, so the punctuation stays where it was.
    let mut out = String::with_capacity(para.len() + 16);
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            let lw = word.to_lowercase();
            if terms.iter().any(|t| lw.starts_with(t)) {
                out.push_str("**");
                out.push_str(word);
                out.push_str("**");
            } else {
                out.push_str(word);
            }
            word.clear();
        }
    };
    for c in para.trim_start_matches(['-', '*', ' ']).chars() {
        if c.is_alphanumeric() || c == '\'' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            if c != '*' {
                out.push(c);
            }
        }
    }
    flush(&mut word, &mut out);
    Some(out)
}

/// `text` up to the last sentence end at or before `max` bytes (whole if
/// shorter, or if no sentence ends in time).
fn cut_at_sentence(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = None;
    for (i, c) in text.char_indices() {
        if i > max {
            break;
        }
        if matches!(c, '.' | '!' | '?') {
            end = Some(i + c.len_utf8());
        }
    }
    end.map_or(text, |e| &text[..e])
}

/// Markdown headings, set as bold lines: inside a card a few hundred
/// pixels tall, a 24 px heading reads as a page break, not a signpost.
#[must_use]
pub fn flatten_headings(article: &str) -> String {
    article
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with('#') {
                let text = t.trim_start_matches('#').trim();
                if text.is_empty() {
                    String::new()
                } else {
                    format!("**{text}**")
                }
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Related topics: same organ, the most useful kinds first. Tapping one
/// opens it, and the camera flies to its element.
fn related_topics(state: &Rc<AppState>, index: usize) -> Vec<WidgetNode> {
    let Some(topic) = state.db.topics.get(index) else {
        return Vec::new();
    };
    let Some(organ) = topic.organ else {
        return Vec::new();
    };
    let mut candidates: Vec<(usize, f32, &str)> = state
        .db
        .topics
        .iter()
        .enumerate()
        .filter(|(i, t)| *i != index && t.organ == Some(organ))
        .map(|(i, t)| {
            let boost = match t.kind {
                'B' => 0.5,
                'S' => 0.4,
                'D' => 0.3,
                'X' | 'T' | 'P' => 0.15,
                _ => 0.0,
            };
            let len = t.title.chars().count() as f32;
            (i, boost + (48.0 - len.min(48.0)) * 0.005, t.title)
        })
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    candidates
        .into_iter()
        .take(4)
        .map(|(i, _score, title)| {
            chip(
                state,
                &short(title, 22),
                crate::ACCENT,
                Rc::new(move |s: &AppState| select_topic(s, i)),
            )
        })
        .collect()
}

/// The provenance block: whose words, from which page.
fn source_line(topic: &db::Topic) -> WidgetNode {
    Flex::column()
        .spacing(2.0)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(children![
            Text::new("SOURCE".to_owned()).style(style(MICRO, FontWeight::Bold, crate::MUTED, 1.4)),
            Text::new(format!("“{}”", topic.title)).style(style(
                CAPTION,
                FontWeight::Medium,
                crate::INK.with_alpha(220),
                0.1
            )),
            Text::new(short(topic.url.trim_start_matches("https://"), 60)).style(style(
                CAPTION,
                FontWeight::Regular,
                crate::MUTED,
                0.1
            )),
        ])
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passage_is_the_paragraph_that_answers() {
        let article = "## Overview\nThe kidneys filter blood in two bean-shaped organs.\nKidney stones are hard deposits that form in the kidneys.\nSee a doctor.";
        let terms = vec!["kidney".to_owned(), "ston".to_owned()];
        let p = matched_passage(article, &terms).unwrap();
        assert!(p.starts_with("**Kidney** **stones**"), "{p}");
        assert!(p.contains("**kidneys**"));
        assert_eq!(matched_passage(article, &[]), None);
        assert_eq!(matched_passage(article, &["zzz".to_owned()]), None);
    }

    #[test]
    fn headings_become_bold_lines() {
        assert_eq!(
            flatten_headings("## What is it?\nText\n#\n"),
            "**What is it?**\nText\n"
        );
    }

    #[test]
    fn small_helpers() {
        assert_eq!(group(11043), "11,043");
        assert_eq!(group(999), "999");
        assert_eq!(short("abcdef", 4), "abc…");
        assert_eq!(host("https://my.example.org/a/b"), "my.example.org");
    }
}
