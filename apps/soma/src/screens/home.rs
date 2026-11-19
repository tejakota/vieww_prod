//! The home screen: the body, the floating search, and the answer card
//! that rides with it.
//!
//! # The layout, and why it is a stack and not a column
//!
//! The 3D body owns the *whole* screen; the search floats over it as one
//! glass card — input on top, results or the open topic below — rather
//! than sitting in a band of its own with its own background. That fusion
//! is the point: the card is part of the scene's chrome, so it *moves* in
//! the scene's terms. The pose's `pill` spring carries the rule: when the
//! leading hit lives low in the body (kidneys, knees) the card rides high;
//! when it lives high (brain, eyes) the card drops low. The answer and
//! the question never fight for the same pixels, and every flight of the
//! camera has a matching glide of the card.
//!
//! # What rebuilds when
//!
//! Every animated frame the pose signal changes and this screen rebuilds —
//! but only the parts that read the pose actually differ: the rendered
//! image, the card's position and its heights. The search field, the
//! results and the topic card read *their own* signals, so typing
//! repaints a list, not a 3D render, and the camera flight repaints the
//! render, not the list.

use std::rc::Rc;

use vieww::element::Runtime;
use vieww::foundation::{Color, EdgeInsets, FontWeight, Offset, Shadow, Size, TextStyle, TextEditingValue};
use vieww::prelude::*;
use vieww::widget::{icons, DrawOnce};

use crate::{anatomy, clear, db, pick_body, run_search, select_topic, AppState};

// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

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
    // The internal buffer: a fraction of the logical size, width-capped.
    let bw = ((vw * anatomy::RENDER_SCALE) as u32)
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

    GestureDetector::new()
        .on_tap({
            let state = state.clone();
            move |tap| pick_body(&state, tap.local.dx, tap.local.dy)
        })
        .child(
            Stack::new()
                .children(children![
                    Image::new(image)
                        .width(vw)
                        .height(vh)
                        .fit(vieww::foundation::BoxFit::Fill)
                        .label("the body"),
                    // A symmetric cool wash — the card can now ride high or
                    // low, so both ends of the scene need somewhere to sit.
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
                ]),
        )
        .into()
}

/// The brand and the corpus line, floating at the top.
fn top_bar(state: &Rc<AppState>, vw: f32) -> WidgetNode {
    let topics = state.db.topics.len();
    Positioned::new()
        .left(20.0)
        .top(16.0)
        .width(vw - 40.0)
        .child(
            Flex::column()
                .spacing(3.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Text::new("SOMA").style(style(21.0, FontWeight::Bold, crate::INK, 5.0)),
                    Text::new(format!(
                        "health explorer · {topics} topics · semantic + graph · offline"
                    ))
                    .style(style(10.5, FontWeight::Medium, crate::MUTED, 1.6)),
                ]),
        )
        .into()
}

/// A label pinned to the selected organ's projected screen position.
fn organ_callout(
    state: &Rc<AppState>,
    pose: &anatomy::Pose,
    vw: f32,
    vh: f32,
) -> WidgetNode {
    let selected = state.selected.get();
    let topic_open = state.topic.get().is_some();
    let Some(organ) = selected.map(|i| &state.db.organs[i]) else {
        return nothing();
    };
    // The callout belongs to an open topic — while typing, the camera
    // itself is the feedback.
    let opacity = if topic_open { pose.xray } else { 0.0 };
    if opacity < 0.08 {
        return nothing();
    }
    let at = anatomy::Vec3::new(organ.anchor[0], organ.anchor[1], organ.anchor[2]);
    let Some((fx, fy, depth)) = anatomy::project(&pose.camera, vw, vh, at) else {
        return nothing();
    };
    if depth <= 0.0 || fx < -20.0 || fx > vw + 20.0 || fy < -20.0 || fy > vh + 20.0 {
        return nothing();
    }
    let x = fx;
    let y = fy;

    // The fade is baked into the colours rather than wrapped in an
    // `Opacity`: a `Positioned` must be the Stack's direct child to be
    // positioned at all, and one widget of wrapping would hide it.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let a = |v: u8| ((v as f32 * opacity) as u8).min(v);
    let chip = Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(10, 16, 26, a(220)))
                .radius(12.0)
                .border(vieww::foundation::Border::new(crate::ACCENT.with_alpha(a(140)), 1.0))
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, a(120)),
                    Offset::new(0.0, 4.0),
                    14.0,
                )),
        )
        .padding(EdgeInsets::symmetric(9.0, 7.0))
        .child(
            Flex::column()
                .spacing(2.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Flex::row()
                        .spacing(7.0)
                        .children(children![
                            Container::new()
                                .decoration(BoxDecoration::new().color(crate::ACCENT.with_alpha(a(255))).radius(3.0))
                                .width(6.0)
                                .height(6.0),
                            Text::new(organ.name.to_owned())
                                .style(style(12.0, FontWeight::Bold, crate::INK.with_alpha(a(255)), 0.4)),
                        ]),
                    // The citation travels with the marker: this is where
                    // in the body, and whose words describe it.
                    Text::new("Cleveland Clinic · my.clevelandclinic.org".to_owned())
                        .style(style(8.5, FontWeight::Medium, crate::MUTED.with_alpha(a(230)), 0.3)),
                ]),
        );

    Positioned::new()
        .left((x - 46.0).max(8.0))
        .top((y - 92.0).max(96.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    chip,
                    // A thin leader line down toward the organ.
                    Container::new()
                        .decoration(BoxDecoration::new().color(crate::ACCENT.with_alpha(a(90))))
                        .width(1.5)
                        .height(22.0),
                ]),
        )
        .into()
}

// ---------------------------------------------------------------------------
// The floating card
// ---------------------------------------------------------------------------

/// The floating search card: the pill on top, results or the open topic
/// under it, in one glass sheet that rides the `pill` spring.
///
/// At rest (`panel = 0`) it is the pill alone, low over the body's feet.
/// Alive (`panel = 1`) it grows to hold the answer, and `pill` decides
/// which end of the scene it floats toward.
fn floating_card(state: &Rc<AppState>, pose: &anatomy::Pose, vw: f32, vh: f32) -> WidgetNode {
    let query = state.query.get();
    let has_query = !query.text.trim().is_empty();
    let topic_open = state.topic.get().is_some();

    // The collapsed card is the pill alone; the expanded card holds the
    // answer. Heights chosen against a phone: ~46% for results, ~55% for
    // a topic with its illustration.
    let collapsed = 72.0_f32.min(vh * 0.12);
    let expanded = if topic_open { (vh * 0.55).min(520.0) } else { (vh * 0.44).min(430.0) };
    let card_h = collapsed + (expanded - collapsed) * pose.panel;

    // Where the card floats: `pill` 0 parks it at the bottom edge, 1
    // lifts it under the brand line, and the spring glides between.
    let low_top = (vh - card_h - 18.0).max(76.0);
    let high_top = 96.0;
    let top = low_top + (high_top - low_top) * pose.pill;

    let body: WidgetNode = if topic_open {
        topic_card(state, card_h, vw)
    } else if has_query {
        results_card(state, card_h)
    } else if card_h > collapsed + 40.0 {
        // Mid-transition only: the hint rides up with the card.
        hint_card(card_h)
    } else {
        nothing()
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
                        .spacing(9.0)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .children(children![search_pill(state), body]),
                ),
        )
        .into()
}

/// The pill: the search field, the mode badge, and the clear button.
fn search_pill(state: &Rc<AppState>) -> WidgetNode {
    let value = state.query.get();
    let non_empty = !value.text.trim().is_empty();
    let regex_on = regex_active(state);

    Flex::row()
        .spacing(10.0)
        .children(children![
            search_icon(),
            Flexible::new(1).child(
                TextField::new(value)
                    .size(15.0)
                    .color(crate::INK)
                    .placeholder("Ask the body anything — symptoms, organs, conditions…")
                    .placeholder_color(crate::MUTED)
                    .on_changed(Rc::new({
                        let state = state.clone();
                        move |next: TextEditingValue| {
                            state.query.set(next);
                            run_search(&state);
                        }
                    })),
            ),
            if non_empty {
                clear_button(state)
            } else {
                nothing()
            },
            if non_empty && regex_on {
                Text::new("regex".to_owned())
                    .style(style(9.0, FontWeight::Bold, crate::ACCENT, 1.0))
                    .into()
            } else {
                nothing()
            },
        ])
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
        Size::new(16.0, 16.0),
        DrawOnce::new(|_size| {
            use vieww::widget::DrawInstruction::*;
            vec![
                FillCircle {
                    center: Offset::new(6.6, 6.6),
                    radius: 4.6,
                    color: crate::MUTED,
                },
                FillCircle {
                    center: Offset::new(6.6, 6.6),
                    radius: 3.2,
                    color: crate::CARD,
                },
                DrawLine {
                    from: Offset::new(9.9, 9.9),
                    to: Offset::new(13.8, 13.8),
                    color: crate::MUTED,
                    width: 1.6,
                },
            ]
        }),
    )
    .into()
}

fn clear_button(state: &Rc<AppState>) -> WidgetNode {
    GestureDetector::new()
        .on_tap({
            let state = state.clone();
            move |_tap| clear(&state)
        })
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(Color::rgba(255, 255, 255, 26))
                        .radius(10.0),
                )
                .padding(EdgeInsets::all(4.0))
                .child(
                    Icon::new(icons::close())
                        .size(13.0)
                        .color(crate::MUTED),
                ),
        )
        .into()
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// The idle card: what this is, and where to start.
fn hint_card(card_h: f32) -> WidgetNode {
    let h = (card_h - 86.0).max(8.0);
    SizedBox::height(h).child(
        Flex::column()
            .spacing(7.0)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children(children![
                Text::new(
                    "Ask the body. It answers from 11,043 Cleveland Clinic \
                     topics — by meaning and through the anatomy graph, not \
                     just by word — with no network at all.",
                )
                .style(style(12.5, FontWeight::Regular, crate::MUTED, 0.2)),
                Flex::row()
                    .spacing(6.0)
                    .children(children![
                        hint_chip("chest pain"),
                        hint_chip("tired all the time"),
                        hint_chip("heart attack"),
                        hint_chip("migra"),
                    ]),
            ]),
    )
    .into()
}

fn hint_chip(label: &str) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(255, 255, 255, 16))
                .radius(9.0)
                .border(vieww::foundation::Border::new(crate::EDGE, 1.0)),
        )
        .padding(EdgeInsets::symmetric(8.0, 5.0))
        .child(Text::new(label.to_owned()).style(style(11.0, FontWeight::Medium, crate::MUTED, 0.3)))
        .into()
}

/// How a hit was found, as a small colored tag for its row.
fn found_tags(flags: u8) -> Vec<WidgetNode> {
    let mut out: Vec<WidgetNode> = Vec::new();
    let mut tag = |label: &str, color: Color| {
        out.push(
            Container::new()
                .decoration(
                    BoxDecoration::new().color(color.with_alpha(36)).radius(6.0),
                )
                .padding(EdgeInsets::symmetric(4.5, 2.0))
                .child(Text::new(label.to_owned()).style(style(8.0, FontWeight::Bold, color, 0.6)))
                .into(),
        );
    };
    if flags & crate::search::found::LITERAL != 0 {
        tag("text", crate::MUTED);
    }
    if flags & crate::search::found::SEMANTIC != 0 {
        tag("meaning", crate::ACCENT);
    }
    if flags & crate::search::found::GRAPH != 0 {
        tag("graph", crate::CORAL);
    }
    if flags & crate::search::found::EXPANDED != 0 {
        tag("expanded", Color::rgb(122, 162, 247));
    }
    out
}

/// The results list, live as you type.
fn results_card(state: &Rc<AppState>, card_h: f32) -> WidgetNode {
    let results = state.results.get();
    let fell_back = state.regex_fallback.get();

    let header = if fell_back {
        Text::new("pattern not valid · searching literally".to_owned())
            .style(style(10.0, FontWeight::Medium, crate::CORAL, 0.8))
    } else {
        Text::new(format!("{} answers · best first", results.len()))
            .style(style(10.0, FontWeight::Medium, crate::MUTED, 1.0))
    };

    let list_h = (card_h - 112.0).max(24.0);

    if results.is_empty() {
        return SizedBox::height(list_h)
            .child(
                Flex::column()
                    .spacing(6.0)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .children(children![
                        header,
                        Text::new(
                            "Nothing in the corpus matches that yet — try \
                             another spelling, or a broader word.",
                        )
                        .style(style(12.0, FontWeight::Regular, crate::MUTED, 0.2)),
                    ]),
            )
            .into();
    }

    let rows: Vec<WidgetNode> = results.iter().take(5).map(|hit| result_row(state, hit)).collect();

    SizedBox::height(list_h).child(
        Flex::column()
            .spacing(5.0)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children![
                header,
                Flexible::new(1).child(
                    Scrollable::vertical(state.panel_scroll.offset())
                        .child(Flex::column().spacing(3.0).children(rows)),
                ),
            ]),
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
        .unwrap_or("general");
    let index = hit.topic;
    let title: String = topic.title.to_owned();
    let tags = found_tags(hit.flags);

    GestureDetector::new()
        .on_tap({
            let state = state.clone();
            move |_tap| select_topic(&state, index)
        })
        .child(
            Container::new()
                .decoration(BoxDecoration::new().color(Color::rgba(255, 255, 255, 10)))
                .radius(12.0)
                .padding(EdgeInsets::symmetric(10.0, 8.0))
                .child(
                    Flex::row()
                        .spacing(10.0)
                        .children(children![
                            kind_badge(topic.kind),
                            Flexible::new(1).child(
                                Flex::column()
                                    .spacing(1.5)
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .children(children![
                                        Text::new(title)
                                            .style(style(13.0, FontWeight::Medium, crate::INK, 0.1)),
                                        {
                                            let mut meta = vec![Text::new(format!(
                                                "{} · {}",
                                                topic.kind_name(),
                                                organ
                                            ))
                                            .style(style(10.5, FontWeight::Regular, crate::MUTED, 0.5))
                                            .into()];
                                            meta.extend(tags);
                                            Flex::row().spacing(6.0).children(meta)
                                        },
                                    ]),
                            ),
                        ]),
                ),
        )
        .into()
}

fn kind_badge(kind: char) -> WidgetNode {
    let (letter, color) = match kind {
        'B' => ("B", crate::ACCENT),
        'S' => ("S", crate::CORAL),
        'D' => ("D", crate::CORAL),
        'X' => ("X", Color::rgb(122, 162, 247)),
        'T' | 'P' => ("T", Color::rgb(240, 196, 110)),
        'M' => ("M", Color::rgb(184, 122, 169)),
        _ => ("G", crate::MUTED),
    };
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(color.with_alpha(40))
                .radius(8.0),
        )
        .width(24.0)
        .height(24.0)
        .child(Center::new().child(
            Text::new(letter.to_owned()).style(style(10.5, FontWeight::Bold, color, 0.5)),
        ))
        .into()
}

/// The open topic: illustration, title, kind, the article, its relations,
/// and where every word of it came from.
fn topic_card(state: &Rc<AppState>, card_h: f32, vw: f32) -> WidgetNode {
    let Some(index) = state.topic.get() else {
        return nothing();
    };
    let Some(topic) = state.db.topics.get(index) else {
        return nothing();
    };

    // The organ's illustration, when the corpus carries one: the zoom-in
    // view the answer leads with.
    let organ_key = topic.organ.map(|i| state.db.organs[i].key);
    let illustration = organ_key.and_then(crate::organs::illustration);

    // A region tap with no article: still show what was targeted.
    if state.db.body(index).is_none() && topic.description.is_empty() {
        let organ_line = topic
            .organ
            .map(|i| state.db.organs[i].name)
            .unwrap_or("the body");
        return topic_shell(
            state,
            card_h,
            vw,
            illustration,
            topic.title.to_owned(),
            topic.kind,
            organ_line,
            format!(
                "Tap the body to explore further, or search for a condition \
                 that affects the {organ_line}."
            ),
            index,
        );
    }

    let organ_line = topic
        .organ
        .map(|i| state.db.organs[i].name)
        .unwrap_or("general");
    let mut body_text = state
        .db
        .body(index)
        .map(str::to_owned)
        .unwrap_or_else(|| topic.description.to_owned());
    // An article's first heading usually restates the title ("Heart" ->
    // "What is the heart?"); showing both is a stutter, so drop it when
    // it does.
    {
        let title_lower = topic.title.to_lowercase();
        let mentions = body_text
            .lines()
            .next()
            .and_then(|first| first.strip_prefix("## "))
            .is_some_and(|h| {
                let h = h.to_lowercase();
                h.contains(&title_lower) || title_lower.contains(h.trim_end_matches('?'))
            });
        if mentions {
            body_text = body_text
                .split_once('\n')
                .map(|(_, rest)| rest.trim_start())
                .unwrap_or(&body_text)
                .to_owned();
        }
    }

    topic_shell(
        state,
        card_h,
        vw,
        illustration,
        topic.title.to_owned(),
        topic.kind,
        organ_line,
        body_text,
        index,
    )
}

#[allow(clippy::too_many_arguments)]
fn topic_shell(
    state: &Rc<AppState>,
    card_h: f32,
    vw: f32,
    illustration: Option<Rc<vieww::foundation::Image>>,
    title: String,
    kind: char,
    organ_line: &str,
    body_text: String,
    index: usize,
) -> WidgetNode {
    let art_h = if illustration.is_some() { 148.0 } else { 0.0 };
    let list_h = (card_h - 108.0 - art_h).max(40.0);
    let related = related_topics(state, index);
    let credit = state
        .db
        .topics
        .get(index)
        .and_then(|t| t.organ)
        .map(|i| state.db.organs[i].key)
        .and_then(crate::organs::credit);

    // The figure with its caption; the caption carries the citation for
    // the picture itself.
    let art_block: WidgetNode = match illustration {
        Some(img) => Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgb(12, 17, 28))
                    .radius(14.0)
                    .border(vieww::foundation::Border::new(crate::EDGE, 1.0)),
            )
            .height(art_h)
            .child(
                Clip::rounded(13.0).child(
                    Stack::new().children(children![
                        Image::new((*img).clone())
                            .width(vw - 52.0)
                            .height(art_h - 18.0)
                            .fit(vieww::foundation::BoxFit::Contain)
                            .label("organ illustration"),
                        Positioned::new()
                            .left(10.0)
                            .bottom(4.0)
                            .child(match credit {
                                Some((c_title, _url)) => Text::new(format!(
                                    "Illustration — “{c_title}”, Cleveland Clinic"
                                ))
                                .style(style(8.5, FontWeight::Medium, crate::MUTED, 0.3))
                                .into(),
                                None => nothing(),
                            }),
                    ]),
                ),
            )
            .into(),
        None => nothing(),
    };

    SizedBox::height(list_h).child(
        Stack::new()
            .children(children![
        Scrollable::vertical(state.panel_scroll.offset()).child(
            Flex::column()
                .spacing(10.0)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(children![
                    // Header: kind, organ, title.
                    Flex::column()
                        .spacing(5.0)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(children![
                            Flex::row()
                                .spacing(8.0)
                                .children(children![
                                    kind_badge(kind),
                                    Text::new(db::kind_name(kind).to_owned())
                                        .style(style(10.5, FontWeight::Bold, crate::MUTED, 1.4)),
                                    Text::new("·".to_owned())
                                        .style(style(10.5, FontWeight::Regular, crate::MUTED, 0.0)),
                                    Text::new(organ_line.to_owned())
                                        .style(style(10.5, FontWeight::Bold, crate::ACCENT, 0.8)),
                                ]),
                            Text::new(title).style(style(17.0, FontWeight::Bold, crate::INK, -0.2)),
                        ]),
                    // The organ's picture, cited.
                    art_block,
                    // The article itself.
                    Markdown::new(body_text),
                    // Related, when the organ has more to say.
                    if related.is_empty() {
                        nothing()
                    } else {
                        Flex::column()
                            .spacing(6.0)
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .children(children![
                                Text::new("NEARBY IN THE BODY".to_owned())
                                    .style(style(9.5, FontWeight::Bold, crate::MUTED, 1.6)),
                                Flex::row().spacing(6.0).children(related),
                            ])
                            .into()
                    },
                    // Provenance.
                    source_line(state, index),
                    SizedBox::height(4.0),
                ]),
        ),
        // The fade says "there is more below" without stealing the touch.
        Positioned::new()
            .left(0.0)
            .right(0.0)
            .bottom(0.0)
            .child(
                IgnorePointer::new().child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new().gradient(
                                vieww::foundation::Gradient::vertical().with_stops(&[
                                    (0.0, Color::rgba(16, 22, 35, 0)),
                                    (1.0, Color::rgba(16, 22, 35, 170)),
                                ]),
                            ),
                        )
                        .height(18.0),
                ),
            ),
            ]),
    )
    .into()
}

/// Related topics: same organ, the most useful kinds first.
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
        .take(5)
        .map(|(i, _score, title)| {
            let label: String = title.chars().take(20).collect();
            let long = title.chars().count() > 20;
            GestureDetector::new()
                .on_tap({
                    let state = state.clone();
                    move |_tap| select_topic(&state, i)
                })
                .child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .color(crate::ACCENT.with_alpha(26))
                                .radius(9.0)
                                .border(vieww::foundation::Border::new(
                                    crate::ACCENT.with_alpha(76),
                                    1.0,
                                )),
                        )
                        .padding(EdgeInsets::symmetric(8.0, 5.0))
                        .child(
                            Text::new(if long {
                                format!("{label}…")
                            } else {
                                label
                            })
                            .style(style(10.5, FontWeight::Medium, crate::ACCENT, 0.2)),
                        ),
                )
                .into()
        })
        .collect()
}

/// The provenance block: whose words, from which page.
fn source_line(state: &Rc<AppState>, index: usize) -> WidgetNode {
    let topic = state.db.topics.get(index);
    let url = topic.map(|t| t.url).unwrap_or("");
    let title = topic.map(|t| t.title).unwrap_or("");
    let short: String = url.trim_start_matches("https://").chars().take(58).collect();
    Flex::column()
        .spacing(2.0)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(children![
            Text::new("SOURCE".to_owned()).style(style(9.0, FontWeight::Bold, crate::MUTED, 1.8)),
            Text::new(format!("Cleveland Clinic — “{title}”"))
                .style(style(10.5, FontWeight::Medium, crate::INK.with_alpha(220), 0.2)),
            Text::new(short)
                .style(style(9.5, FontWeight::Regular, crate::MUTED.with_alpha(210), 0.2)),
        ])
        .into()
}
