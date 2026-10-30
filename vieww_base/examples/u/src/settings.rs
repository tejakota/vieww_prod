//! Settings — the studio where the persona is shaped.
//!
//! Reached by the gear on home or a right-to-left swipe; left by the back
//! button or a swipe the other way. The portrait at the top is who U is
//! becoming — the companion at small scale, the name it calls you (edited in
//! place), and the voice and style from the two cards below it, so what the
//! portrait says and what the form says cannot drift apart.
//!
//! The form is **controlled throughout**: every control is handed its value
//! from a signal and reports the change it was asked for, so nothing here
//! holds a second copy of anything. The screen scrolls with a real
//! `ScrollController`, so a fling survives the screen being covered and
//! returned to, and the soft keyboard pushing `view_insets` moves the content
//! rather than hiding it.

use std::cell::Cell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Axis, Color, FontWeight, Offset, Path, Size};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::neurons::{self, Reveal};
use crate::palette::{self, AXON, EDGE, FAINT, INK, MEMBRANE, MUTED, SYNAPSE};
use crate::AppState;

#[derive(Debug)]
pub struct SettingsScreen {
    pub state: AppState,
}

#[widget]
impl SettingsScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();

        let t = state.time.get();
        let burst = state.burst.get();
        let calm = state.calm.get();

        // Dimmer than home: the form, not the biology, is the subject here.
        let field = if calm { 0.12 } else { 0.4 };

        let scroll = state.scroll.clone();

        let screen = Stack::new()
            .fit(StackFit::Expand)
            .push(
                Positioned::fill().child(
                    Container::new()
                        .gradient(palette::ground_gradient())
                        .child(neurons::field(t, burst, field)),
                ),
            )
            .push(
                Positioned::fill().child(
                    SafeArea::new().child(
                        Padding::new(EdgeInsets::symmetric(20.0, 14.0)).child(
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .children(children![
                                    top_bar(&state, ctx),
                                    SizedBox::height(10.0),
                                    // The form is taller than the screen on
                                    // every phone this targets: it scrolls.
                                    Flexible::expanded(1).child(
                                        Scrollable::vertical(scroll.offset())
                                            .key("u-settings-scroll")
                                            .on_drag(scroll.on_drag())
                                            .on_drag_end(scroll.on_drag_end())
                                            .on_extents(scroll.on_extents())
                                            .child(
                                                Padding::new(EdgeInsets::symmetric(0.0, 6.0))
                                                    .child(form(&state, ctx, t)),
                                            ),
                                    ),
                                ]),
                        ),
                    ),
                ),
            );

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
                .child(screen),
        )
    }
}

/// Back button, title, and the person this persona is for.
fn top_bar(state: &AppState, ctx: &BuildContext) -> WidgetNode {
    let initials: String = state
        .display_name()
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();

    let state_for_back = state.clone();
    let back = Pressable::themed(ctx, move |press| {
        Filtered::blur(9.0)
            .with_backdrop()
            .child(
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .color(Color::rgba(14, 20, 34, (140.0 + press * 70.0) as u8))
                            .radius(21.0)
                            .border(Border::new(EDGE, 1.0)),
                    )
                    .size(42.0, 42.0)
                    .alignment(Alignment::CENTER)
                    .child(Icon::new(icons::chevron_left()).size(17.0).color(SYNAPSE)),
            )
            .into()
    })
    .on_tap(move || {
        state_for_back.nav.pop();
    });

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(children![
            back,
            SizedBox::width(14.0),
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(1.0)
                .children(children![
                    Text::new("Settings").size(20.0).bold().color(INK),
                    Text::new("PERSONA STUDIO").size(9.5).bold().color(AXON),
                ]),
            Flexible::expanded(1).child(SizedBox::shrink()),
            Avatar::initials(if initials.is_empty() {
                "U".to_owned()
            } else {
                initials
            })
            .size(38.0)
            .color(AXON),
        ])
        .into()
}

/// The form itself: the portrait, then the numbered cards that shape it.
/// Every write goes through the state, so every change is an event in the
/// log — the form restores through the same fold the answers do.
fn form(state: &AppState, ctx: &BuildContext, t: f32) -> WidgetNode {
    let name = state.name.get();
    let tone = state.tone.get();
    let style = state.style.get();
    let daily = state.daily.get();
    let haptics = state.haptics.get();
    let calm = state.calm.get();
    let biometric = state.biometric.get();

    let tone_direct = state.clone();
    let tone_warm = state.clone();
    let tone_analytical = state.clone();
    let style_fast = state.clone();
    let style_slow = state.clone();
    let daily_pick = state.clone();
    let haptics_flip = state.clone();
    let calm_flip = state.clone();
    let biometric_flip = state.clone();

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(14.0)
        .children(children![
            // -------------------------------------------------- the portrait
            portrait(state, &name, t),
            // ------------------------------------------------------- 01 voice
            section(
                "01  /  VOICE",
                vec![Flex::row()
                    .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .children(children![
                        voice(
                            ctx,
                            "Direct",
                            Voice::Direct,
                            tone == 0,
                            Rc::new(move || tone_direct.set_tone(0)),
                        ),
                        voice(
                            ctx,
                            "Warm",
                            Voice::Warm,
                            tone == 1,
                            Rc::new(move || tone_warm.set_tone(1)),
                        ),
                        voice(
                            ctx,
                            "Analytical",
                            Voice::Analytical,
                            tone == 2,
                            Rc::new(move || tone_analytical.set_tone(2)),
                        ),
                    ])
                    .into()],
            ),
            // --------------------------------------------- 02 decision style
            section(
                "02  /  DECISION STYLE",
                vec![Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(12.0)
                    .children(children![
                        Flexible::new(1).child(style_card(
                            ctx,
                            "Intuitive",
                            "Fast & instinctive",
                            Style::Intuitive,
                            style == 0,
                            Rc::new(move || style_fast.set_style(0)),
                        )),
                        Flexible::new(1).child(style_card(
                            ctx,
                            "Deliberate",
                            "Thorough & measured",
                            Style::Deliberate,
                            style == 1,
                            Rc::new(move || style_slow.set_style(1)),
                        )),
                    ])
                    .into()],
            ),
            // -------------------------------------------------- 03 training
            section(
                "03  /  TRAINING",
                vec![
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .spacing(1.0)
                                .children(children![
                                    Text::new("Daily sessions").size(13.0).bold().color(INK),
                                    Text::new("short moments, spread through the day")
                                        .size(10.5)
                                        .color(MUTED),
                                ]),
                            Flexible::expanded(1).child(SizedBox::shrink()),
                            Text::new(format!("{daily:.0} / day"))
                                .size(20.0)
                                .bold()
                                .color(SYNAPSE),
                        ])
                        .into(),
                    SizedBox::height(6.0).into(),
                    Slider::new(daily)
                        .range(1.0, 12.0)
                        .divisions(11)
                        .on_changed(Rc::new(move |value| daily_pick.set_daily(value)))
                        .into(),
                ],
            ),
            // ------------------------------------------------------- 04 feel
            section(
                "04  /  FEEL",
                vec![
                    switch_row(
                        "Haptic feedback",
                        "a pulse when a memory lands",
                        haptics,
                        Rc::new(move |on| haptics_flip.set_haptics(on)),
                    ),
                    SizedBox::height(8.0).into(),
                    switch_row(
                        "Calm animations",
                        "dim the field, still the sparks",
                        calm,
                        Rc::new(move |on| calm_flip.set_calm(on)),
                    ),
                ],
            ),
            // --------------------------------------------------- 05 privacy
            section(
                "05  /  PRIVACY",
                vec![
                    switch_row(
                        "Biometric lock",
                        "ask for your fingerprint to open U",
                        biometric,
                        Rc::new(move |on| biometric_flip.set_biometric(on)),
                    ),
                    SizedBox::height(10.0).into(),
                    // Locked on, and not a control: there is no preference to
                    // express where the manifest never asked for the network,
                    // and a switch that cannot move is a lie about there
                    // being a choice.
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .spacing(1.0)
                                .children(children![
                                    Text::new("Offline by design").size(12.5).color(INK),
                                    Text::new("U holds no network permission at all")
                                        .size(10.5)
                                        .color(MUTED),
                                ]),
                            Flexible::expanded(1).child(SizedBox::shrink()),
                            Container::new()
                                .decoration(
                                    BoxDecoration::new()
                                        .color(Color::rgb(28, 60, 52))
                                        .radius(11.0)
                                        .border(Border::new(MEMBRANE.with_alpha(90), 1.0)),
                                )
                                .size(22.0, 22.0)
                                .alignment(Alignment::CENTER)
                                .child(Icon::new(icons::check()).size(12.0).color(MEMBRANE),),
                        ])
                        .into(),
                ],
            ),
            // ---------------------------------------------------- the signature
            SizedBox::height(4.0),
            Text::new("U  ·  ON-DEVICE PERSONA")
                .size(9.0)
                .color(FAINT)
                .align(TextAlign::Center),
        ])
        .into()
}

/// The portrait: the companion at small scale, the name it calls you —
/// edited in place, no separate field to visit — and, under both, the voice
/// and style the cards below have given it, so the portrait cannot disagree
/// with the form.
fn portrait(state: &AppState, name: &vieww_foundation::TextEditingValue, t: f32) -> WidgetNode {
    let name_write = state.clone();
    let tone = state.tone.get();
    let style = state.style.get();
    let voice = ["Direct", "Warm", "Analytical"][tone.min(2)];
    let manner = ["Intuitive", "Deliberate"][style.min(1)];

    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(20, 29, 48, 225))
                .radius(23.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::symmetric(16.0, 14.0))
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    neurons::companion(64.0, t, 0.0, 0.0, Reveal::complete()),
                    SizedBox::width(15.0),
                    Flexible::expanded(1).child(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(2.0)
                            .children(children![
                                Text::new("U CALLS YOU").size(8.5).bold().color(FAINT),
                                TextField::new(name.clone())
                                    .size(21.0)
                                    .color(INK)
                                    .placeholder("Your name")
                                    .placeholder_color(MUTED)
                                    .single_line()
                                    .on_changed(Rc::new(move |next| { name_write.set_name(next) })),
                                Text::new(format!("{voice}  ·  {manner}"))
                                    .size(10.5)
                                    .color(AXON),
                            ]),
                    ),
                    SizedBox::width(8.0),
                    // The pencil: not a button — the name beside it is the
                    // control, and this marks it as one.
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .color(Color::rgb(32, 42, 61))
                                .radius(14.0),
                        )
                        .size(28.0, 28.0)
                        .alignment(Alignment::CENTER)
                        .child(pencil(AXON)),
                ]),
        )
        .into()
}

/// One titled card of the form.
fn section(title: &str, items: Vec<WidgetNode>) -> WidgetNode {
    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .push(Text::new(title).size(9.0).bold().color(FAINT))
        .push(SizedBox::height(10.0));
    for item in items {
        column = column.push(item);
    }
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(18, 25, 44, 216))
                .radius(22.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(column)
        .into()
}

/// A labelled switch row.
fn switch_row(head: &str, caption: &str, on: bool, flip: Rc<dyn Fn(bool)>) -> WidgetNode {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(children![
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(1.0)
                .children(children![
                    Text::new(head).size(12.5).color(INK),
                    Text::new(caption).size(10.5).color(MUTED),
                ]),
            Flexible::expanded(1).child(SizedBox::shrink()),
            Switch::new(on).on_changed(flip),
        ])
        .into()
}

// ---- the voice and the style, as the studio draws them ---------------------

/// One of the three voices U can answer in.
#[derive(Debug, Clone, Copy)]
enum Voice {
    Direct,
    Warm,
    Analytical,
}

/// A voice, as a circle: its glyph, its name, and — when it is the one —
/// a lit ring and a tick.
fn voice(
    ctx: &BuildContext,
    label: &'static str,
    which: Voice,
    selected: bool,
    on_pick: Rc<dyn Fn()>,
) -> WidgetNode {
    Pressable::themed(ctx, move |_press| {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(9.0)
            .children(children![
                SizedBox::square(60.0).child(
                    Stack::new()
                        .fit(StackFit::Expand)
                        .push(
                            Positioned::fill().child(
                                Align::new(Alignment::CENTER).child(
                                    Container::new()
                                        .decoration(
                                            BoxDecoration::new()
                                                .color(if selected {
                                                    Color::rgb(42, 33, 68)
                                                } else {
                                                    Color::rgb(25, 36, 58)
                                                })
                                                .radius(if selected { 27.0 } else { 23.0 },)
                                                .border(Border::new(
                                                    if selected {
                                                        AXON.with_alpha(200)
                                                    } else {
                                                        Color::rgba(83, 98, 126, 115)
                                                    },
                                                    if selected { 1.5 } else { 1.0 },
                                                )),
                                        )
                                        .size(
                                            if selected { 54.0 } else { 46.0 },
                                            if selected { 54.0 } else { 46.0 },
                                        )
                                        .alignment(Alignment::CENTER)
                                        .child(voice_glyph(which)),
                                ),
                            ),
                        )
                        .push(
                            Positioned::new()
                                .top(0.0)
                                .right(0.0)
                                .width(18.0)
                                .height(18.0)
                                .child({
                                    let badge: WidgetNode = if selected {
                                        Container::new()
                                            .decoration(
                                                BoxDecoration::new().color(AXON).radius(9.0),
                                            )
                                            .size(18.0, 18.0)
                                            .alignment(Alignment::CENTER)
                                            .child(
                                                Icon::new(icons::check())
                                                    .size(10.0)
                                                    .color(Color::rgb(18, 11, 32)),
                                            )
                                            .into()
                                    } else {
                                        SizedBox::shrink().into()
                                    };
                                    badge
                                }),
                        ),
                ),
                Text::new(label)
                    .size(10.0)
                    .weight(if selected {
                        FontWeight::Bold
                    } else {
                        FontWeight::default()
                    })
                    .color(if selected { INK } else { MUTED }),
            ])
            .into()
    })
    .on_tap(move || on_pick())
    .into()
}

/// One voice's glyph: a speech bubble, a heart, or a measured pulse.
fn voice_glyph(which: Voice) -> Painting {
    let color = Color::rgb(196, 178, 245);
    let heart_color = Color::rgb(209, 194, 255);
    Painting::sized(
        Size::square(24.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let style = StrokeStyle::rounded();
            match which {
                Voice::Direct => {
                    // A speech bubble, waiting to speak.
                    book.stroke_rrect(
                        vieww_foundation::Rect::new(c.dx - 8.0, c.dy - 7.0, c.dx + 8.0, c.dy + 4.0),
                        3.0,
                        color,
                        1.4,
                    );
                    let mut tail = Path::new();
                    tail.move_to(Offset::new(c.dx - 3.0, c.dy + 4.0));
                    tail.line_to(Offset::new(c.dx - 4.0, c.dy + 8.0));
                    tail.line_to(Offset::new(c.dx + 1.0, c.dy + 4.0));
                    book.stroke_styled(tail, color, 1.4, style.clone());
                    for (i, width) in [5.0, 3.0].iter().enumerate() {
                        let mut line = Path::new();
                        line.move_to(Offset::new(c.dx - *width, c.dy - 2.5 + i as f32 * 3.0));
                        line.line_to(Offset::new(c.dx + *width, c.dy - 2.5 + i as f32 * 3.0));
                        book.stroke_styled(line, color, 1.2, style.clone());
                    }
                }
                Voice::Warm => {
                    // A heart.
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx, c.dy + 7.0));
                    path.cubic_to(
                        Offset::new(c.dx - 10.0, c.dy + 1.0),
                        Offset::new(c.dx - 8.0, c.dy - 8.0),
                        Offset::new(c.dx, c.dy - 3.0),
                    );
                    path.cubic_to(
                        Offset::new(c.dx + 8.0, c.dy - 8.0),
                        Offset::new(c.dx + 10.0, c.dy + 1.0),
                        Offset::new(c.dx, c.dy + 7.0),
                    );
                    book.stroke_styled(path, heart_color, 1.6, style);
                }
                Voice::Analytical => {
                    // A measured pulse.
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx - 9.0, c.dy));
                    path.line_to(Offset::new(c.dx - 4.0, c.dy));
                    path.line_to(Offset::new(c.dx - 1.5, c.dy - 6.0));
                    path.line_to(Offset::new(c.dx + 1.5, c.dy + 6.0));
                    path.line_to(Offset::new(c.dx + 4.0, c.dy));
                    path.line_to(Offset::new(c.dx + 9.0, c.dy));
                    book.stroke_styled(path, color, 1.5, style);
                }
            }
        }),
    )
}

/// One of the two ways U can weigh a choice.
#[derive(Debug, Clone, Copy)]
enum Style {
    Intuitive,
    Deliberate,
}

/// A decision style, as a card: glyph, name, promise, and — when it is the
/// one — a lit border and a tick.
fn style_card(
    ctx: &BuildContext,
    title: &'static str,
    caption: &'static str,
    which: Style,
    selected: bool,
    on_pick: Rc<dyn Fn()>,
) -> WidgetNode {
    Pressable::themed(ctx, move |press| {
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
                            AXON.with_alpha((200.0 + press * 55.0) as u8)
                        } else {
                            Color::rgba(255, 255, 255, 20)
                        },
                        if selected { 1.35 } else { 1.0 },
                    )),
            )
            .padding(EdgeInsets::symmetric(12.0, 12.0))
            .child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(10.0)
                    .children(children![
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children(children![
                                Container::new()
                                    .decoration(BoxDecoration::new().color(if selected {
                                        Color::rgb(51, 41, 77)
                                    } else {
                                        Color::rgb(28, 43, 59)
                                    }),)
                                    .size(38.0, 38.0)
                                    .alignment(Alignment::CENTER)
                                    .child(style_glyph(which)),
                                Flexible::expanded(1).child(SizedBox::shrink()),
                                {
                                    let badge: WidgetNode = if selected {
                                        Container::new()
                                            .decoration(
                                                BoxDecoration::new().color(AXON).radius(9.0),
                                            )
                                            .size(18.0, 18.0)
                                            .alignment(Alignment::CENTER)
                                            .child(
                                                Icon::new(icons::check())
                                                    .size(10.0)
                                                    .color(Color::rgb(18, 11, 32)),
                                            )
                                            .into()
                                    } else {
                                        SizedBox::shrink().into()
                                    };
                                    badge
                                },
                            ]),
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(2.0)
                            .children(children![
                                Text::new(title)
                                    .size(11.5)
                                    .weight(if selected {
                                        FontWeight::Bold
                                    } else {
                                        FontWeight::default()
                                    })
                                    .color(if selected {
                                        Color::rgb(238, 233, 255)
                                    } else {
                                        Color::rgb(196, 204, 218)
                                    }),
                                Text::new(caption).size(9.0).color(MUTED),
                            ]),
                    ]),
            )
            .into()
    })
    .on_tap(move || on_pick())
    .into()
}

/// One style's glyph: a bolt, or a balance.
fn style_glyph(which: Style) -> Painting {
    let bolt_color = Color::rgb(221, 206, 255);
    let scale_color = Color::rgb(139, 222, 211);
    Painting::sized(
        Size::square(24.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let style = StrokeStyle::rounded();
            match which {
                Style::Intuitive => {
                    // A bolt: fast, and gone.
                    let mut path = Path::new();
                    path.move_to(Offset::new(c.dx + 3.0, c.dy - 9.0));
                    path.line_to(Offset::new(c.dx - 5.0, c.dy + 1.0));
                    path.line_to(Offset::new(c.dx + 1.0, c.dy + 1.0));
                    path.line_to(Offset::new(c.dx - 3.0, c.dy + 9.0));
                    path.line_to(Offset::new(c.dx + 5.0, c.dy - 2.0));
                    path.line_to(Offset::new(c.dx - 1.0, c.dy - 2.0));
                    path.line_to(Offset::new(c.dx + 3.0, c.dy - 9.0));
                    book.stroke_styled(path, bolt_color, 1.7, style);
                }
                Style::Deliberate => {
                    // A balance: weighed, level, held.
                    let mut beam = Path::new();
                    beam.move_to(Offset::new(c.dx - 8.0, c.dy - 3.0));
                    beam.line_to(Offset::new(c.dx + 8.0, c.dy - 3.0));
                    book.stroke_styled(beam, scale_color, 1.4, style.clone());
                    let mut post = Path::new();
                    post.move_to(Offset::new(c.dx, c.dy - 5.0));
                    post.line_to(Offset::new(c.dx, c.dy + 7.0));
                    post.move_to(Offset::new(c.dx - 4.0, c.dy + 7.0));
                    post.line_to(Offset::new(c.dx + 4.0, c.dy + 7.0));
                    book.stroke_styled(post, scale_color, 1.4, style.clone());
                    for side in [-1.0_f32, 1.0] {
                        let arm = Offset::new(c.dx + side * 8.0, c.dy - 3.0);
                        let mut pan = Path::new();
                        pan.move_to(Offset::new(arm.dx - 3.5, arm.dy + 5.0));
                        pan.line_to(arm);
                        pan.line_to(Offset::new(arm.dx + 3.5, arm.dy + 5.0));
                        book.stroke_styled(pan, scale_color, 1.4, style.clone());
                    }
                }
            }
        }),
    )
}

/// A pencil, marking the name as editable.
fn pencil(color: Color) -> Painting {
    Painting::sized(
        Size::square(14.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let mut path = Path::new();
            path.move_to(Offset::new(c.dx - 3.5, c.dy + 3.5));
            path.line_to(Offset::new(c.dx + 2.0, c.dy - 2.0));
            path.line_to(Offset::new(c.dx + 4.0, c.dy - 4.0));
            path.line_to(Offset::new(c.dx + 5.5, c.dy - 2.5));
            path.line_to(Offset::new(c.dx, c.dy + 3.5));
            path.line_to(Offset::new(c.dx - 3.5, c.dy + 3.5));
            book.fill(path, color);
            let mut tip = Path::new();
            tip.move_to(Offset::new(c.dx - 3.5, c.dy + 3.5));
            tip.line_to(Offset::new(c.dx - 1.5, c.dy + 3.5));
            tip.line_to(Offset::new(c.dx - 3.5, c.dy + 1.5));
            tip.line_to(Offset::new(c.dx - 3.5, c.dy + 3.5));
            book.fill(tip, Color::rgb(18, 11, 32));
        }),
    )
}
