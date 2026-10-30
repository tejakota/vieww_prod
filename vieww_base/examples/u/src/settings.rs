//! Settings — the studio where the persona is shaped.
//!
//! Reached by the ALIVE chip on home or a right-to-left swipe; left by the
//! back button or a swipe the other way. The portrait at the top is who U is
//! becoming — the companion at small scale, the name it calls you (edited in
//! place), and the voice and style from the two cards below it, so what the
//! portrait says and what the form says cannot drift apart.
//!
//! # The layout is the reference's own
//!
//! The reference draws the studio on a 420 × 1040 canvas; the app lays the
//! form out at exactly those coordinates inside a box of exactly that size,
//! which scrolls in the 420 × 920 viewport the [`crate::stage`] provides.
//! The numbered cards sit at y = 235, 382, 522, 697 and 840; every row,
//! every glyph, every toggle is where the reference put it. What scrolls is
//! the whole canvas — the header travels with the form, exactly as drawn.
//!
//! # The form is controlled throughout
//!
//! Every control is handed its value from a signal and reports the change it
//! was asked for, so nothing here holds a second copy of anything. The
//! screen scrolls with a real `ScrollController`, so a fling survives the
//! screen being covered and returned to.

use std::cell::Cell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Axis, Color, FontWeight, Offset, Path, Size, TextStyle};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::controls;
use crate::neurons::{self, Reveal};
use crate::palette::{self, AXON, CARD, EDGE, INK, MUTED, SLIDER_MINT};
use crate::stage;
use crate::AppState;

/// The studio's own canvas — the reference's settings drawing, whole.
const CANVAS: Size = Size::new(420.0, 1040.0);

/// Where a glyph's baseline sits below its line box's top, as a fraction of
/// the font size — Inter's own ascent, the same constant home places its
/// text by.
const ASCENT: f32 = 0.969;

/// A text box top for a baseline at `baseline`.
fn top_of(baseline: f32, size: f32) -> f32 {
    baseline - ASCENT * size
}

/// A `Positioned` at the reference's own coordinates.
fn place(left: f32, top: f32) -> Positioned {
    Positioned::new().left(left).top(top)
}

#[derive(Debug)]
pub struct SettingsScreen {
    pub state: AppState,
}

#[widget]
impl SettingsScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();

        let t = state.time.get();
        let calm = state.calm.get();

        // The studio's atmosphere at the reference's own presence; calm —
        // the app's own reduced-motion switch — keeps a third of it.
        let amb = if calm { 0.35 } else { 1.0 };

        let scroll = state.scroll.clone();

        let canvas = Stack::new()
            .fit(StackFit::Expand)
            .push(Positioned::fill().child(neurons::studio(t, amb)))
            .children(children![
                header(&state, ctx),
                portrait(&state, t),
                voice_section(&state, ctx),
                style_section(&state, ctx),
                training_section(&state, t),
                feel_section(&state, ctx),
                privacy_section(&state, ctx),
                footer(),
            ]);

        // The form is taller than the viewport on every phone this targets:
        // the whole reference canvas scrolls, header and all.
        let scroller = Scrollable::vertical(scroll.offset())
            .key("u-settings-scroll")
            .on_drag(scroll.on_drag())
            .on_drag_end(scroll.on_drag_end())
            .on_extents(scroll.on_extents())
            .child(SizedBox::from_size(CANVAS).child(canvas));

        // The way back: a swipe from left to right pops the screen — the
        // mirror of the swipe that opened it. The form scrolls vertically;
        // a horizontal pull was never its gesture to take.
        let travel = Rc::new(Cell::new(0.0_f32));
        let travel_start = travel.clone();
        let travel_update = travel.clone();
        let state_for_back = state.clone();
        WidgetNode::new(
            GestureDetector::new()
                .drag_axis(Axis::Horizontal)
                .on_drag_start(move |_details| travel_start.set(0.0))
                .on_drag_update(move |details| {
                    travel_update.set(travel_update.get() + details.delta.dx);
                })
                .on_drag_end(move |details| {
                    if travel.get() >= 48.0 || details.velocity.dx >= 420.0 {
                        state_for_back.nav.pop();
                    }
                })
                .child(stage::screen(scroller)),
        )
    }
}

// ---- the header -------------------------------------------------------------


/// Back button, title, and the person this persona is for — the reference's
/// own marks: the button at (24, 28), the title's baseline at y = 48, the
/// medallion centred at (373, 49).
fn header(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let initial = state
        .display_name()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "U".to_owned());

    let state_for_back = state.clone();
    let back = Pressable::themed(ctx, move |press| {
        let _ = press;
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgb(21, 30, 48))
                    .radius(15.0)
                    .border(Border::new(Color::rgba(255, 255, 255, 23), 1.0)),
            )
            .size(42.0, 42.0)
            .alignment(Alignment::CENTER)
            .child(back_chevron())
            .into()
    })
    .on_tap(move || {
        state_for_back.nav.pop();
    });

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, 28.0).child(back))
            .push(place(82.0, top_of(48.0, 20.0)).child(Text::new("Settings").style(
                TextStyle::new(20.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(-0.6)
                    .color(Color::rgb(240, 242, 250)),
            )))
            .push(place(82.0, top_of(63.0, 9.5)).child(Text::new("PERSONA STUDIO").style(
                TextStyle::new(9.5).letter_spacing(0.8).color(Color::rgb(127, 140, 164)),
            )))
            .push(place(333.0, 41.8).child(
                SizedBox::width(80.0).child(
                    Text::new(initial).style(
                        TextStyle::new(13.0)
                            .weight(FontWeight::Medium)
                            .color(Color::rgb(222, 214, 255)),
                    )
                    .align(TextAlign::Center),
                ),
            ))
            .push(place(354.0, 30.0).child(avatar_disc())),
    )
    .into()
}

/// The back chevron — the reference's own stroke, `#c4b6f5` at 1.8.
fn back_chevron() -> Painting {
    Painting::sized(
        Size::square(24.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(196, 182, 245);
            let mut path = Path::new();
            path.move_to(Offset::new(c.dx + 6.0, c.dy - 9.0));
            path.line_to(Offset::new(c.dx - 2.0, c.dy));
            path.line_to(Offset::new(c.dx + 6.0, c.dy + 9.0));
            book.stroke_styled(path, color, 1.8, StrokeStyle::rounded());
        }),
    )
}

/// The medallion — `#27213e` under an `#aa92f7` rim at 35%, centred on
/// (373, 49).
fn avatar_disc() -> Painting {
    Painting::sized(
        Size::square(38.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.circle(c, 19.0, Color::rgb(39, 33, 62));
            book.ring(c, 19.0, 1.0, Color::rgb(170, 146, 247).with_alpha(89));
        }),
    )
}

// ---- the portrait -----------------------------------------------------------

/// The identity hero: the companion at 0.28 scale, the name it calls you —
/// edited in place, no separate field to visit — the voice and manner under
/// both, and the pencil that marks the name as editable. The reference's own
/// card: 372 × 112 at (24, 92), the mark centred at in-card (67, 55).
fn portrait(state: &AppState, t: f32) -> WidgetNode {
    let name_write = state.clone();
    let tone = state.tone.get();
    let style = state.style.get();
    let voice = ["Direct", "Warm", "Analytical"][tone.min(2)];
    let manner = ["Intuitive", "Deliberate"][style.min(1)];

    place(24.0, 92.0).child(
        SizedBox::from_size(Size::new(372.0, 112.0)).child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(palette::CARD_HERO)
                        .radius(23.0)
                        .border(Border::new(Color::rgba(255, 255, 255, 23), 1.0)),
                )
                .child(
                    Stack::new()
                        .fit(StackFit::Expand)
                        .push(Positioned::fill().child(hero_wash()))
                        .push(place(25.0, 13.0).child(neurons::companion(
                            84.0,
                            t,
                            0.0,
                            0.0,
                            Reveal::complete(),
                        )))
                        .push(place(127.0, top_of(39.0, 9.0)).child(Text::new("U CALLS YOU").style(
                            TextStyle::new(9.0)
                                .weight(FontWeight::Medium)
                                .letter_spacing(1.7)
                                .color(Color::rgb(129, 144, 168)),
                        )))
                        .push(
                            place(127.0, top_of(66.0, 22.0)).width(190.0).child(
                                TextField::new(state.name.get())
                                    .size(22.0)
                                    .color(INK)
                                    .placeholder("Your name")
                                    .placeholder_color(MUTED)
                                    .single_line()
                                    .on_changed(Rc::new(move |next| {
                                        name_write.set_name(next)
                                    })),
                            ),
                        )
                        .push(place(127.0, top_of(88.0, 10.5)).child(
                            Text::new(format!("{voice}  ·  {manner}"))
                                .size(10.5)
                                .color(Color::rgb(166, 154, 203)),
                        ))
                        .push(place(317.0, 42.0).child(pencil_disc())),
                ),
        ),
    )
    .into()
}

/// The hero's own wash — the reference's ambient overlay, a soft violet bloom
/// over the card at 48%.
fn hero_wash() -> Painting {
    Painting::new(PaintWith::new(move |book, s| {
        let c = Offset::new(s.width / 2.0, s.height / 2.0);
        book.circle(
            c,
            s.width / 2.0,
            Gradient::radial_fill().with_stops(&[
                (0.0, Color::rgb(156, 123, 247).with_alpha(43)),
                (0.55, Color::rgb(101, 223, 208).with_alpha(12)),
                (1.0, Color::TRANSPARENT),
            ]),
        );
    }))
}

/// The pencil's disc — `#202a3d` at (331, 56) in-card, r = 14.
fn pencil_disc() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.circle(c, 14.0, Color::rgb(32, 42, 61));
            let mut path = Path::new();
            path.move_to(Offset::new(c.dx - 5.0, c.dy + 5.0));
            path.line_to(Offset::new(c.dx - 4.0, c.dy + 1.0));
            path.line_to(Offset::new(c.dx + 3.0, c.dy - 6.0));
            path.line_to(Offset::new(c.dx + 6.0, c.dy - 3.0));
            path.line_to(Offset::new(c.dx - 1.0, c.dy + 4.0));
            path.line_to(Offset::new(c.dx - 5.0, c.dy + 5.0));
            book.stroke(path, Color::rgb(185, 167, 247), 1.25);
        }),
    )
}

// ---- 01 · voice -------------------------------------------------------------


/// The voice card — the reference's own: circles at (87, 283), (210, 283)
/// and (333, 283), the selected one larger and lit, its tick at the corner.
fn voice_section(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let tone = state.tone.get();
    let tone_direct = state.clone();
    let tone_warm = state.clone();
    let tone_analytical = state.clone();

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, 235.0).child(section_card(372.0, 116.0)))
            .push(place(24.0, top_of(222.0, 9.0)).child(section_label("01  /  VOICE")))
            .push(voice(
                ctx,
                87.0,
                "Direct",
                Voice::Direct,
                tone == 0,
                Rc::new(move || tone_direct.set_tone(0)),
            ))
            .push(voice(
                ctx,
                210.0,
                "Warm",
                Voice::Warm,
                tone == 1,
                Rc::new(move || tone_warm.set_tone(1)),
            ))
            .push(voice(
                ctx,
                333.0,
                "Analytical",
                Voice::Analytical,
                tone == 2,
                Rc::new(move || tone_analytical.set_tone(2)),
            )),
    )
    .into()
}

/// One of the three voices U can answer in.
#[derive(Debug, Clone, Copy)]
enum Voice {
    Direct,
    Warm,
    Analytical,
}

/// A voice, as the reference draws it: its circle centred at `(x, 283)` —
/// r = 27 and lit when it is the one, r = 23 and quiet when it is not — its
/// glyph, and its name below.
fn voice(
    ctx: &BuildContext,
    x: f32,
    label: &'static str,
    which: Voice,
    selected: bool,
    on_pick: Rc<dyn Fn()>,
) -> WidgetNode {
    // The column the finger taps: the circle, then the name. Warm's circle
    // is four points larger and its name sits four lower, as drawn.
    let (disc, name_size, circle_top, name_top) = if selected {
        (54.0_f32, 10.5_f32, 256.0_f32, 322.1_f32)
    } else {
        (46.0, 10.0, 260.0, 318.6)
    };

    let pressable = Pressable::themed(ctx, move |_press| {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children![
                voice_disc(which, selected),
                SizedBox::height((name_top - (circle_top + disc)).max(0.0)),
                SizedBox::width(120.0).child(
                    Text::new(label).style(
                        TextStyle::new(name_size)
                            .weight(FontWeight::Medium)
                            .color(if selected {
                                Color::rgb(225, 216, 255)
                            } else {
                                Color::rgb(145, 160, 184)
                            }),
                    )
                    .align(TextAlign::Center),
                ),
            ])
            .into()
    })
    .on_tap(move || on_pick());

    Stack::new()
        .fit(StackFit::Expand)
        .push(place(x - 60.0, circle_top).child(pressable))
        .into()
}

/// The voice's own disc, glyph and — when it is the one — its tick, drawn as
/// the reference draws them.
fn voice_disc(which: Voice, selected: bool) -> Painting {
    let size = if selected { 54.0 } else { 46.0 };
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let style = StrokeStyle::rounded();

            if selected {
                // Warm: r = 27 of `#2a2144` under an `#aa92fa` rim, a r = 21
                // inner disc of `#342751`, and the tick at its corner.
                book.circle(c, 27.0, Color::rgb(42, 33, 68));
                book.ring(c, 27.0, 1.5, Color::rgb(170, 146, 250));
                book.circle(c, 21.0, Color::rgb(52, 39, 81));
            } else {
                book.circle(c, 23.0, Color::rgb(25, 36, 58));
                book.ring(c, 23.0, 1.0, Color::rgba(83, 98, 126, 115));
            }

            let color = Color::rgb(158, 172, 195);
            match which {
                Voice::Direct => {
                    // The reference's document: an 18 × 13 face, two lines.
                    book.stroke_rrect(
                        vieww_foundation::Rect::new(c.dx - 9.0, c.dy - 7.0, c.dx + 9.0, c.dy + 6.0),
                        2.0,
                        color,
                        1.4,
                    );
                    let mut lines = Path::new();
                    lines.move_to(Offset::new(c.dx - 5.0, c.dy - 2.0));
                    lines.line_to(Offset::new(c.dx + 5.0, c.dy - 2.0));
                    lines.move_to(Offset::new(c.dx - 5.0, c.dy + 2.0));
                    lines.line_to(Offset::new(c.dx + 2.0, c.dy + 2.0));
                    book.stroke_styled(lines, color, 1.4, style);
                }
                Voice::Warm => {
                    // The reference's heart, in its four bows, `#d1c2ff`.
                    let heart = Color::rgb(209, 194, 255);
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx, c.dy + 10.0));
                    path.cubic_to(
                        Offset::new(c.dx - 11.0, c.dy + 3.0),
                        Offset::new(c.dx - 11.0, c.dy - 4.0),
                        Offset::new(c.dx, c.dy - 6.0),
                    );
                    path.cubic_to(
                        Offset::new(c.dx + 11.0, c.dy - 4.0),
                        Offset::new(c.dx + 11.0, c.dy + 3.0),
                        Offset::new(c.dx, c.dy + 10.0),
                    );
                    book.stroke_styled(path, heart, 1.6, style.clone());

                    // The tick: r = 5 of `#9c83ed`, rimmed with the card's
                    // own ground so it reads as separate, check in white.
                    let badge = Offset::new(c.dx + 20.0, c.dy - 20.0);
                    book.circle(badge, 7.0, Color::rgb(18, 27, 45));
                    book.circle(badge, 5.0, Color::rgb(156, 131, 237));
                    let mut check = Path::new();
                    check.move_to(Offset::new(badge.dx - 2.0, badge.dy));
                    check.line_to(Offset::new(badge.dx - 0.5, badge.dy + 1.5));
                    check.line_to(Offset::new(badge.dx + 2.5, badge.dy - 1.5));
                    book.stroke_styled(check, Color::rgb(255, 255, 255), 1.0, style.clone());
                }
                Voice::Analytical => {
                    // The reference's measured zigzag over its baseline.
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx - 10.0, c.dy + 8.0));
                    path.line_to(Offset::new(c.dx - 5.0, c.dy - 8.0));
                    path.line_to(Offset::new(c.dx, c.dy + 8.0));
                    path.line_to(Offset::new(c.dx + 5.0, c.dy - 8.0));
                    path.line_to(Offset::new(c.dx + 10.0, c.dy + 8.0));
                    book.stroke_styled(path, color, 1.35, style.clone());
                    let mut base = Path::new();
                    base.move_to(Offset::new(c.dx - 11.0, c.dy + 10.0));
                    base.line_to(Offset::new(c.dx + 11.0, c.dy + 10.0));
                    book.stroke_styled(base, color, 1.35, style.clone());
                }
            }
        }),
    )
}

// ---- 02 · decision style ----------------------------------------------------


/// The two style cards, standing side by side at the reference's own marks:
/// (24, 382) and (218, 382), 178 × 105 each.
fn style_section(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let style = state.style.get();
    let style_fast = state.clone();
    let style_slow = state.clone();

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, top_of(369.0, 9.0)).child(section_label("02  /  DECISION STYLE")))
            .push(style_card(
                ctx,
                24.0,
                "Intuitive",
                "Fast & instinctive",
                Style::Intuitive,
                style == 0,
                Rc::new(move || style_fast.set_style(0)),
            ))
            .push(style_card(
                ctx,
                218.0,
                "Deliberate",
                "Thorough & measured",
                Style::Deliberate,
                style == 1,
                Rc::new(move || style_slow.set_style(1)),
            )),
    )
    .into()
}

/// One of the two ways U can weigh a choice.
#[derive(Debug, Clone, Copy)]
enum Style {
    Intuitive,
    Deliberate,
}

/// A decision style, as its own card: glyph and name centred, and — when it
/// is the one — a lit border and a tick at the corner.
fn style_card(
    ctx: &BuildContext,
    x: f32,
    title: &'static str,
    caption: &'static str,
    which: Style,
    selected: bool,
    on_pick: Rc<dyn Fn()>,
) -> WidgetNode {
    let card = Pressable::themed(ctx, move |press| {
        SizedBox::from_size(Size::new(178.0, 105.0)).child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(if selected {
                            Color::rgb(37, 32, 58)
                        } else {
                            Color::rgb(18, 29, 46)
                        })
                        .radius(19.0)
                        .border(Border::new(
                            if selected {
                                AXON.with_alpha((204.0 + press * 51.0) as u8)
                            } else {
                                Color::rgba(255, 255, 255, 20)
                            },
                            if selected { 1.35 } else { 1.0 },
                        )),
                )
                .child(
                    Stack::new()
                        .fit(StackFit::Expand)
                        .push(place(70.0, 15.0).child(style_disc(which, selected)))
                        .push(place(9.0, top_of(76.0, 11.5)).width(160.0).child(
                            Text::new(title).style(
                                TextStyle::new(11.5)
                                    .weight(FontWeight::Medium)
                                    .color(if selected {
                                        Color::rgb(238, 233, 255)
                                    } else {
                                        Color::rgb(196, 204, 218)
                                    }),
                            )
                            .align(TextAlign::Center),
                        ))
                        .push(place(9.0, top_of(91.0, 9.0)).width(160.0).child(
                            Text::new(caption).style(
                                TextStyle::new(9.0)
                                    .color(if selected {
                                        Color::rgb(169, 156, 202)
                                    } else {
                                        Color::rgb(135, 150, 173)
                                    }),
                            )
                            .align(TextAlign::Center),
                        ))
                        .push(if selected {
                            let badge: WidgetNode = place(154.0, 11.0).child(tick_badge()).into();
                            badge
                        } else {
                            SizedBox::shrink().into()
                        }),
                ),
        )
        .into()
    })
    .on_tap(move || on_pick());

    place(x, 382.0).child(card).into()
}

/// The style's own disc — r = 19, `#33294d` when it is the one, `#1c2b3b`
/// when it is not — carrying the reference's own glyph.
fn style_disc(which: Style, selected: bool) -> Painting {
    Painting::sized(
        Size::square(38.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let style = StrokeStyle::rounded();
            book.circle(
                c,
                19.0,
                if selected {
                    Color::rgb(51, 41, 77)
                } else {
                    Color::rgb(28, 43, 59)
                },
            );
            match which {
                Style::Intuitive => {
                    // The reference's bolt: fast, and gone.
                    let color = Color::rgb(221, 206, 255);
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx + 3.0, c.dy - 14.0));
                    path.line_to(Offset::new(c.dx - 9.0, c.dy + 3.0));
                    path.line_to(Offset::new(c.dx, c.dy + 3.0));
                    path.line_to(Offset::new(c.dx - 2.0, c.dy + 15.0));
                    path.line_to(Offset::new(c.dx + 12.0, c.dy - 5.0));
                    path.line_to(Offset::new(c.dx + 3.0, c.dy - 5.0));
                    path.close();
                    book.stroke_styled(path, color, 1.7, style);
                }
                Style::Deliberate => {
                    // The reference's balance: a post, a beam, two pans, a
                    // base — weighed, level, held.
                    let color = Color::rgb(139, 222, 211);
                    let mut post = Path::new();
                    post.move_to(Offset::new(c.dx, c.dy - 14.0));
                    post.line_to(Offset::new(c.dx, c.dy + 14.0));
                    post.move_to(Offset::new(c.dx - 12.0, c.dy - 8.0));
                    post.line_to(Offset::new(c.dx + 12.0, c.dy - 8.0));
                    post.move_to(Offset::new(c.dx - 12.0, c.dy + 8.0));
                    post.line_to(Offset::new(c.dx + 12.0, c.dy + 8.0));
                    book.stroke_styled(post, color, 1.35, style.clone());
                    for side in [-1.0_f32, 1.0] {
                        let mut pan = Path::new();
                        pan.move_to(Offset::new(c.dx + side * 10.0, c.dy - 8.0));
                        pan.line_to(Offset::new(c.dx + side * 16.0, c.dy + 2.0));
                        pan.line_to(Offset::new(c.dx + side * 4.0, c.dy + 2.0));
                        book.stroke_styled(pan, color, 1.35, style.clone());
                    }
                }
            }
        }),
    )
}

/// The selected card's corner tick — r = 6 of `#9e86f0`, check in white.
fn tick_badge() -> Painting {
    Painting::sized(
        Size::square(12.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.circle(c, 6.0, Color::rgb(158, 134, 240));
            let mut check = Path::new();
            check.move_to(Offset::new(c.dx - 3.0, c.dy));
            check.line_to(Offset::new(c.dx - 1.0, c.dy + 2.0));
            check.line_to(Offset::new(c.dx + 3.0, c.dy - 2.0));
            book.stroke_styled(check, Color::rgb(255, 255, 255), 1.1, StrokeStyle::rounded());
        }),
    )
}

// ---- 03 · training ----------------------------------------------------------


/// The training card — the reference's own 372 × 147 at (24, 522): the plus
/// badge, the count, the mint slider with its eleven dots, and the spread.
fn training_section(state: &AppState, t: f32) -> WidgetNode {
    let daily = state.daily.get();
    let daily_pick = state.clone();

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, 522.0).child(section_card(372.0, 147.0)))
            .push(place(24.0, top_of(509.0, 9.0)).child(section_label("03  /  TRAINING")))
            .push(place(42.0, 539.0).child(training_plus()))
            .push(place(82.0, top_of(551.0, 12.0)).child(Text::new("Daily sessions").style(
                TextStyle::new(12.0)
                    .weight(FontWeight::Medium)
                    .color(Color::rgb(233, 237, 245)),
            )))
            .push(place(82.0, top_of(566.0, 9.5)).child(
                Text::new("Small moments, through the day")
                    .size(9.5)
                    .color(Color::rgb(129, 144, 168)),
            ))
            .push(place(141.0, top_of(561.0, 20.0)).width(200.0).child(
                Text::new(format!("{daily:.0}")).style(
                    TextStyle::new(20.0)
                        .weight(FontWeight::Bold)
                        .color(SLIDER_MINT),
                )
                .align(TextAlign::End),
            ))
            .push(place(349.0, top_of(560.0, 10.0)).child(
                Text::new("/ day").size(10.0).color(Color::rgb(138, 153, 177)),
            ))
            .push(place(48.0, 590.0).child(controls::daily_slider(
                daily,
                t,
                Rc::new(move |value| daily_pick.set_daily(value)),
            )))
            .push(place(48.0, top_of(627.0, 8.5)).child(
                Text::new("1").size(8.5).color(Color::rgb(116, 132, 157)),
            ))
            .push(place(272.0, top_of(627.0, 8.5)).width(100.0).child(
                Text::new("12").size(8.5).color(Color::rgb(116, 132, 157)).align(TextAlign::End),
            ))
            .push(place(42.0, 636.0).child(divider()))
            .push(place(45.0, 642.0).child(clock_badge()))
            .push(place(76.0, top_of(656.0, 10.5)).child(
                Text::new("Spread through the day")
                    .size(10.5)
                    .color(Color::rgb(203, 211, 224)),
            ))
            .push(place(276.0, top_of(656.0, 9.0)).width(100.0).child(
                Text::new("ON").style(
                    TextStyle::new(9.0)
                        .weight(FontWeight::Bold)
                        .color(SLIDER_MINT),
                )
                .align(TextAlign::End),
            )),
    )
    .into()
}

/// The training card's plus — the reference's: r = 14 of `#183a3c`, a cross
/// eighteen across, `#67e4d3` at 1.4.
fn training_plus() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.circle(c, 14.0, Color::rgb(24, 58, 60));
            let mut plus = Path::new();
            plus.move_to(Offset::new(c.dx, c.dy - 9.0));
            plus.line_to(Offset::new(c.dx, c.dy + 9.0));
            plus.move_to(Offset::new(c.dx - 9.0, c.dy));
            plus.line_to(Offset::new(c.dx + 9.0, c.dy));
            book.stroke_styled(plus, Color::rgb(103, 228, 211), 1.4, StrokeStyle::rounded());
        }),
    )
}

/// The spread-through-the-day clock — r = 10 of `#202d42`.
fn clock_badge() -> Painting {
    Painting::sized(
        Size::square(20.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(166, 181, 202);
            book.circle(c, 10.0, Color::rgb(32, 45, 66));
            book.ring(c, 5.0, 1.3, color);
            let mut hands = Path::new();
            hands.move_to(Offset::new(c.dx, c.dy - 2.6));
            hands.line_to(Offset::new(c.dx, c.dy));
            hands.line_to(Offset::new(c.dx + 2.2, c.dy + 1.2));
            book.stroke_styled(hands, color, 1.3, StrokeStyle::rounded());
        }),
    )
}

// ---- 04 · feel --------------------------------------------------------------


/// The feel card — the reference's 372 × 116 at (24, 697): haptics and calm,
/// each a round badge, two lines, and a pill.
fn feel_section(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let haptics = state.haptics.get();
    let calm = state.calm.get();
    let haptics_flip = state.clone();
    let calm_flip = state.clone();

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, 697.0).child(section_card(372.0, 116.0)))
            .push(place(24.0, top_of(684.0, 9.0)).child(section_label("04  /  FEEL")))
            .push(place(42.0, 712.0).child(haptics_badge()))
            .push(place(82.0, top_of(724.0, 11.5)).child(
                Text::new("Haptic feedback").style(
                    TextStyle::new(11.5)
                        .weight(FontWeight::Medium)
                        .color(Color::rgb(233, 237, 245)),
                ),
            ))
            .push(place(82.0, top_of(739.0, 9.0)).child(
                Text::new("A pulse when a memory lands")
                    .size(9.0)
                    .color(Color::rgb(129, 144, 168)),
            ))
            .push(place(338.0, 715.0).child(controls::pill(
                ctx,
                haptics,
                Rc::new(move |on| haptics_flip.set_haptics(on)),
            )))
            .push(place(42.0, 755.0).child(divider()))
            .push(place(42.0, 769.0).child(calm_badge()))
            .push(place(82.0, top_of(781.0, 11.5)).child(
                Text::new("Calm animations").style(
                    TextStyle::new(11.5)
                        .weight(FontWeight::Medium)
                        .color(Color::rgb(233, 237, 245)),
                ),
            ))
            .push(place(82.0, top_of(796.0, 9.0)).child(
                Text::new("Reduce movement and glow")
                    .size(9.0)
                    .color(Color::rgb(129, 144, 168)),
            ))
            .push(place(338.0, 772.0).child(controls::pill(
                ctx,
                calm,
                Rc::new(move |on| calm_flip.set_calm(on)),
            ))),
    )
    .into()
}

/// Haptic feedback — the reference's heart in `#292342`, `#c5b3ff` at 1.2.
fn haptics_badge() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(197, 179, 255);
            book.circle(c, 14.0, Color::rgb(41, 35, 66));
            let mut heart = Path::new();
            heart.move_to(Offset::new(c.dx, c.dy - 8.0));
            heart.cubic_to(
                Offset::new(c.dx - 4.0, c.dy - 8.0),
                Offset::new(c.dx - 6.0, c.dy - 5.0),
                Offset::new(c.dx - 6.0, c.dy - 1.0),
            );
            heart.cubic_to(
                Offset::new(c.dx - 6.0, c.dy + 3.0),
                Offset::new(c.dx - 2.0, c.dy + 5.0),
                Offset::new(c.dx, c.dy + 8.0),
            );
            heart.cubic_to(
                Offset::new(c.dx + 2.0, c.dy + 5.0),
                Offset::new(c.dx + 6.0, c.dy + 3.0),
                Offset::new(c.dx + 6.0, c.dy - 1.0),
            );
            heart.cubic_to(
                Offset::new(c.dx + 6.0, c.dy - 5.0),
                Offset::new(c.dx + 4.0, c.dy - 8.0),
                Offset::new(c.dx, c.dy - 8.0),
            );
            book.stroke_styled(heart, color, 1.2, StrokeStyle::rounded());
            let mut pulse = Path::new();
            pulse.move_to(Offset::new(c.dx, c.dy - 5.0));
            pulse.line_to(Offset::new(c.dx, c.dy + 3.0));
            book.stroke_styled(pulse, color, 1.2, StrokeStyle::rounded());
        }),
    )
}

/// Calm animations — the reference's window in `#1d2a3c`, `#8dded4` at 1.2.
fn calm_badge() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(141, 222, 212);
            book.circle(c, 14.0, Color::rgb(29, 42, 60));
            book.stroke_rrect(
                vieww_foundation::Rect::new(c.dx - 7.0, c.dy - 6.0, c.dx + 7.0, c.dy + 6.0),
                1.0,
                color,
                1.2,
            );
            let mut lines = Path::new();
            lines.move_to(Offset::new(c.dx - 4.0, c.dy - 2.0));
            lines.line_to(Offset::new(c.dx + 4.0, c.dy - 2.0));
            lines.move_to(Offset::new(c.dx - 4.0, c.dy + 2.0));
            lines.line_to(Offset::new(c.dx + 1.0, c.dy + 2.0));
            book.stroke_styled(lines, color, 1.2, StrokeStyle::rounded());
        }),
    )
}

// ---- 05 · privacy -----------------------------------------------------------


/// The privacy card — the reference's 372 × 133 at (24, 840): the biometric
/// lock, and the offline posture that is not a preference at all.
fn privacy_section(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let biometric = state.biometric.get();
    let biometric_flip = state.clone();

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(24.0, 840.0).child(section_card(372.0, 133.0)))
            .push(place(24.0, top_of(827.0, 9.0)).child(section_label("05  /  PRIVACY")))
            .push(place(42.0, 855.0).child(lock_badge()))
            .push(place(82.0, top_of(867.0, 11.5)).child(
                Text::new("Biometric lock").style(
                    TextStyle::new(11.5)
                        .weight(FontWeight::Medium)
                        .color(Color::rgb(233, 237, 245)),
                ),
            ))
            .push(place(82.0, top_of(882.0, 9.0)).child(
                Text::new("Use your fingerprint to open U")
                    .size(9.0)
                    .color(Color::rgb(129, 144, 168)),
            ))
            .push(place(338.0, 858.0).child(controls::pill(
                ctx,
                biometric,
                Rc::new(move |on| biometric_flip.set_biometric(on)),
            )))
            .push(place(42.0, 902.0).child(divider()))
            .push(place(42.0, 923.0).child(shield_badge()))
            .push(place(82.0, top_of(935.0, 11.5)).child(
                Text::new("Offline by design").style(
                    TextStyle::new(11.5)
                        .weight(FontWeight::Medium)
                        .color(Color::rgb(233, 237, 245)),
                ),
            ))
            .push(place(82.0, top_of(950.0, 9.0)).child(
                Text::new("Network access unavailable")
                    .size(9.0)
                    .color(Color::rgb(129, 144, 168)),
            ))
            .push(place(355.0, 927.0).child(offline_disc())),
    )
    .into()
}

/// Biometric lock — the reference's lock in `#18372f`, `#72e1b9` at 1.2.
fn lock_badge() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(114, 225, 185);
            book.circle(c, 14.0, Color::rgb(24, 55, 47));
            book.stroke_rrect(
                vieww_foundation::Rect::new(c.dx - 6.0, c.dy - 2.0, c.dx + 6.0, c.dy + 9.0),
                2.0,
                color,
                1.2,
            );
            let mut shackle = Path::new();
            shackle.move_to(Offset::new(c.dx - 4.0, c.dy - 2.0));
            shackle.line_to(Offset::new(c.dx - 4.0, c.dy - 5.0));
            shackle.cubic_to(
                Offset::new(c.dx - 4.0, c.dy - 9.0),
                Offset::new(c.dx + 4.0, c.dy - 9.0),
                Offset::new(c.dx + 4.0, c.dy - 5.0),
            );
            shackle.line_to(Offset::new(c.dx + 4.0, c.dy - 2.0));
            book.stroke_styled(shackle, color, 1.2, StrokeStyle::rounded());
        }),
    )
}

/// Offline by design — the reference's shield with its check, in `#18372f`.
fn shield_badge() -> Painting {
    Painting::sized(
        Size::square(28.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(114, 225, 185);
            book.circle(c, 14.0, Color::rgb(24, 55, 47));
            let mut shield = Path::new();
            shield.move_to(Offset::new(c.dx, c.dy - 9.0));
            shield.line_to(Offset::new(c.dx + 7.0, c.dy - 6.0));
            shield.line_to(Offset::new(c.dx + 7.0, c.dy - 1.0));
            shield.cubic_to(
                Offset::new(c.dx + 7.0, c.dy + 4.0),
                Offset::new(c.dx + 4.0, c.dy + 7.0),
                Offset::new(c.dx, c.dy + 9.0),
            );
            shield.cubic_to(
                Offset::new(c.dx - 4.0, c.dy + 7.0),
                Offset::new(c.dx - 7.0, c.dy + 4.0),
                Offset::new(c.dx - 7.0, c.dy - 1.0),
            );
            shield.line_to(Offset::new(c.dx - 7.0, c.dy - 6.0));
            shield.close();
            book.stroke_styled(shield, color, 1.2, StrokeStyle::rounded());
            let mut check = Path::new();
            check.move_to(Offset::new(c.dx - 4.0, c.dy));
            check.line_to(Offset::new(c.dx - 1.0, c.dy + 3.0));
            check.line_to(Offset::new(c.dx + 4.0, c.dy - 3.0));
            book.stroke_styled(check, color, 1.2, StrokeStyle::rounded());
        }),
    )
}

/// The offline row's own indicator — r = 10 of `#1c3c34`, check `#72e1b9`
/// at 1.5. Not a control: there is no preference to express where the
/// manifest never asked for the network.
fn offline_disc() -> Painting {
    Painting::sized(
        Size::square(20.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(114, 225, 185);
            book.circle(c, 10.0, Color::rgb(28, 60, 52));
            let mut check = Path::new();
            check.move_to(Offset::new(c.dx - 4.0, c.dy));
            check.line_to(Offset::new(c.dx - 1.0, c.dy + 3.0));
            check.line_to(Offset::new(c.dx + 4.0, c.dy - 3.0));
            book.stroke_styled(check, color, 1.5, StrokeStyle::rounded());
        }),
    )
}

// ---- the signature ----------------------------------------------------------


/// The line that signs the form — the reference's y = 1005, centred.
fn footer() -> WidgetNode {
    place(0.0, top_of(1005.0, 9.0)).width(420.0).child(
        Text::new("U  ·  ON-DEVICE PERSONA")
            .style(
                TextStyle::new(9.0)
                    .letter_spacing(0.6)
                    .color(Color::rgb(94, 109, 132)),
            )
            .align(TextAlign::Center),
    )
    .into()
}

// ---- the shared parts -------------------------------------------------------

/// One of the numbered cards — `#121b2d` under a hairline rim, the
/// reference's own radius of 21.
fn section_card(width: f32, height: f32) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(CARD)
                .radius(21.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .size(width, height)
        .into()
}

/// A section's label: the reference's numbered, violet, letterspaced caps.
fn section_label(title: &str) -> WidgetNode {
    Text::new(title).style(
        TextStyle::new(9.0)
            .weight(FontWeight::Bold)
            .letter_spacing(2.0)
            .color(Color::rgb(155, 139, 210)),
    )
    .into()
}

/// A hairline between the rows of a card — `#2b374a`, from x = 18 to 354
/// of the card it lives in.
fn divider() -> WidgetNode {
    Container::new()
        .decoration(BoxDecoration::new().color(Color::rgb(43, 55, 74)))
        .width(336.0)
        .height(1.0)
        .into()
}
