//! Home — the screen the app opens on, and the whole of what the app asks
//! for: one decision at a time.
//!
//! The screen has two phases, and the difference between them is the answer
//! to "what do I do here?":
//!
//! - **The first sketch** — until twelve answers are recorded, the card
//!   counts them ("4 of 12"). The twelve are a pass, not a loop: the day the
//!   twelfth lands, the card says so — once, ever — and the counter never
//!   comes back.
//! - **The daily rhythm** — after the sketch, one question at a time against
//!   the day's target ("today's training · 2 of 4"). The bank still supplies
//!   the questions, rotating for variety, but it is never counted again.
//!
//! Everything else on the screen is either who it is for (the greeting) or
//! the proof it is working (the counter in the card, the field firing when a
//! choice lands). The persona's cell breathes above the question. `calm`
//! dims the field to a third rather than stopping it.

use vieww::prelude::*;
use vieww_foundation::{Color, Offset};

use crate::ask::{Ask, ASKS};
use crate::neurons;
use crate::palette::{self, AXON, EDGE, FAINT, INK, MEMBRANE, MUTED, SYNAPSE};
use crate::{settings_route, take, AppState, Shared};

/// The sketch is one pass through the bank: [`ASKS.len()`] answers, once.
#[allow(clippy::cast_possible_truncation)]
const SKETCH: u32 = ASKS.len() as u32;

/// Which asking phase the card is in — the whole of the home screen's state
/// machine, so the build reads as one decision about layout rather than a
/// nest of counters disagreeing.
enum Asking {
    /// The first sketch: `done` of the twelve answers are recorded.
    Sketch {
        /// Answers so far, ever.
        done: u32,
    },
    /// The daily rhythm: `done` of the day's `target`.
    Daily {
        /// Sessions today.
        done: u32,
        /// The day's target, from settings.
        target: u32,
    },
}

#[derive(Debug)]
pub struct HomeScreen {
    pub state: AppState,
    pub shared: Shared,
}

#[widget]
impl HomeScreen {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let theme = ThemeData::of(ctx);
        let state = self.state.clone();

        let t = state.time.get();
        let burst = state.burst.get();
        let calm = state.calm.get();
        let name = state.display_name();

        // Which question, and which phase. The sketch is done when twelve
        // answers exist; the completion card stands between the phases until
        // it is seen, and never again.
        let answers = state.answers.get();
        let complete = answers >= SKETCH && !state.seen_sketch.get();
        let asking = if answers < SKETCH {
            Asking::Sketch { done: answers }
        } else {
            Asking::Daily {
                done: state.sessions.get(),
                target: state.daily.get().round().max(1.0) as u32,
            }
        };

        // One line about where the two of you stand — the only text on the
        // screen that is not the question itself.
        let stand = match (&asking, complete) {
            (Asking::Sketch { .. }, _) => "U doesn't know you yet".to_owned(),
            (_, true) => "The sketch is done — the portrait begins".to_owned(),
            (_, false) => {
                format!("Good to see you — U holds {answers} of your decisions")
            }
        };

        let index = state.ask.get() % ASKS.len();
        let card = if complete {
            sketch_card(&state, ctx, t, burst)
        } else {
            question_card(&state, ctx, &ASKS[index], &asking, t, burst)
        };

        let field = if calm { 0.3 } else { 1.0 };

        WidgetNode::new(
            Stack::new()
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
                                    .spacing(0.0)
                                    .children(children![
                                        top_bar(&self.shared, ctx, t),
                                        SizedBox::height(24.0),
                                        greeting(&stand, &name, &theme),
                                        SizedBox::height(20.0),
                                        Flexible::expanded(1)
                                            .child(Align::new(Alignment::CENTER).child(card)),
                                        SizedBox::height(12.0),
                                        Text::new("everything stays on this device")
                                            .size(10.5)
                                            .color(FAINT)
                                            .align(TextAlign::Center),
                                    ]),
                            ),
                        ),
                    ),
                ),
        )
    }
}

/// The wordmark and the gear.
fn top_bar(shared: &Shared, ctx: &BuildContext, t: f32) -> WidgetNode {
    // The gear: slowly turning at rest, a further turn as it is pressed. A
    // plain fill rather than frosted glass — a backdrop blur re-reads the
    // whole screen behind the chip every frame it animates, which on a
    // phone is real money.
    let shared_for_gear = shared.clone();
    let angle = t * 0.12;
    let gear = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(16, 23, 40, (205.0 + press * 50.0) as u8))
                    .radius(21.0)
                    .border(Border::new(EDGE, 1.0)),
            )
            .size(42.0, 42.0)
            .alignment(Alignment::CENTER)
            .child(neurons::gear(21.0, SYNAPSE, angle + press * 0.8))
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_gear);
        state.nav.push(settings_route(shared_for_gear.clone()));
    });

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(children![
            // The soma — the app's own mark, one warm cell.
            neurons::soma_mark(16.0),
            SizedBox::width(8.0),
            Text::new("U").size(24.0).bold().color(INK),
            SizedBox::width(7.0),
            Text::new("persona").size(11.0).color(MUTED),
            Flexible::expanded(1).child(SizedBox::shrink()),
            gear,
        ])
        .into()
}

/// Who this one is for, and where the two of them stand.
fn greeting(stand: &str, name: &str, theme: &ThemeData) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(4.0)
        .children(children![
            Text::new(stand).style(theme.text.body).color(MUTED),
            Text::new(name).style(theme.text.headline).color(INK),
        ])
        .into()
}

/// The header every card shares: the phase's name on the left, where it
/// stands on the right, and the one progress bar on the screen under both —
/// one counter, one place, rather than a card counter and a bottom row
/// disagreeing about which of them is the truth.
fn card_head(label: &str, counter: &str, frac: f32) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(7.0)
        .children(children![
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    Text::new(label).size(9.0).bold().color(FAINT),
                    Flexible::expanded(1).child(SizedBox::shrink()),
                    Text::new(counter).size(9.0).color(FAINT),
                ]),
            LinearProgress::new(frac).thickness(3.0).label(label),
        ])
        .into()
}

/// The day's question: the persona's cell, the prompt, two answers, and a
/// way to pass.
#[allow(clippy::cast_precision_loss)]
fn question_card(
    state: &AppState,
    ctx: &BuildContext,
    ask: &Ask,
    asking: &Asking,
    t: f32,
    burst: f32,
) -> WidgetNode {
    let (label, counter, frac) = match asking {
        Asking::Sketch { done } => (
            "FIRST SKETCH",
            format!("{} of {}", u64::from(done + 1), ASKS.len()),
            *done as f32 / SKETCH as f32,
        ),
        Asking::Daily { done, target } => (
            "TODAY'S TRAINING",
            if done >= target {
                "target met".to_owned()
            } else {
                format!("{done} of {target}")
            },
            *done as f32 / *target as f32,
        ),
    };

    Container::new()
        .decoration(
            BoxDecoration::new()
                .gradient(palette::card_gradient())
                .radius(26.0)
                .border(Border::new(EDGE, 1.0))
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, 110),
                    Offset::new(0.0, 18.0),
                    30.0,
                )),
        )
        .padding(EdgeInsets::symmetric(18.0, 22.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(14.0)
                .children(children![
                    card_head(label, &counter, frac),
                    Align::new(Alignment::CENTER).child(neurons::persona_cell(112.0, t, burst)),
                    Text::new(ask.prompt)
                        .size(17.0)
                        .color(INK)
                        .align(TextAlign::Center),
                    choices(state, ctx, ask),
                    footer(state, ctx, asking),
                ]),
        )
        .into()
}

/// The once-ever card between the two phases: twelve answers are in, the
/// sketch exists, and the daily rhythm is one tap away. Nothing here asks a
/// question — the whole point is that the asking can stop being counted.
fn sketch_card(state: &AppState, ctx: &BuildContext, t: f32, burst: f32) -> WidgetNode {
    let state_for_tap = state.clone();
    Container::new()
        .decoration(
            BoxDecoration::new()
                .gradient(palette::card_gradient())
                .radius(26.0)
                .border(Border::new(EDGE, 1.0))
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, 110),
                    Offset::new(0.0, 18.0),
                    30.0,
                )),
        )
        .padding(EdgeInsets::symmetric(18.0, 22.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(14.0)
                .children(children![
                    card_head(
                        "FIRST SKETCH",
                        &format!("{} of {SKETCH}", u64::from(SKETCH)),
                        1.0,
                    ),
                    Align::new(Alignment::CENTER).child(neurons::persona_cell(128.0, t, burst)),
                    Text::new("That's twelve.")
                        .size(19.0)
                        .bold()
                        .color(INK)
                        .align(TextAlign::Center),
                    Text::new(
                        "U has a first sketch of you — loosely held, easily redrawn. \
                         A decision a day sharpens it into a portrait.",
                    )
                    .size(12.5)
                    .color(MUTED)
                    .align(TextAlign::Center),
                    Pressable::themed(ctx, move |press| {
                        let sink = 1.0 - press * 0.03;
                        Container::new()
                            .decoration(
                                BoxDecoration::new()
                                    .gradient(palette::action_gradient())
                                    .radius(17.0)
                                    .shadow(Shadow::new(
                                        AXON.with_alpha(70),
                                        Offset::new(0.0, 12.0),
                                        26.0,
                                    )),
                            )
                            .height(52.0)
                            .alignment(Alignment::CENTER)
                            .child(
                                Transformed::new(Transform::scale_around(
                                    Offset::new(172.0, 26.0),
                                    sink,
                                    sink,
                                ))
                                .child(
                                    Text::new("Begin the daily rhythm")
                                        .size(14.0)
                                        .bold()
                                        .color(Color::rgb(18, 11, 32)),
                                ),
                            )
                            .into()
                    })
                    .on_tap(move || state_for_tap.see_sketch()),
                ]),
        )
        .into()
}

/// The two answers, equal in weight — a choice with a right answer would
/// not be a preference. Equal weight, not equal invisibility: answering is
/// the one thing the screen asks for, so the answers are the most
/// button-like objects on it.
fn choices(state: &AppState, ctx: &BuildContext, ask: &Ask) -> WidgetNode {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(12.0)
        .children(children![
            Flexible::new(1).child(choice(state.clone(), ctx, ask.a, true)),
            Flexible::new(1).child(choice(state.clone(), ctx, ask.b, false)),
        ])
        .into()
}

/// One answer. The press lands as light: the fill lifts and the border
/// brightens, and nothing moves — a button that slid under a thumb mid-tap
/// would be answering a different question.
fn choice(state: AppState, ctx: &BuildContext, label: &'static str, which: bool) -> WidgetNode {
    Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(
                        (30.0 + press * 12.0) as u8,
                        (42.0 + press * 16.0) as u8,
                        (70.0 + press * 24.0) as u8,
                        240,
                    ))
                    .radius(17.0)
                    .border(Border::new(
                        EDGE.with_alpha((46.0 + press * 130.0) as u8),
                        1.0,
                    )),
            )
            .height(52.0)
            .alignment(Alignment::CENTER)
            .padding(EdgeInsets::symmetric(10.0, 0.0))
            .child(
                Text::new(label)
                    .size(13.0)
                    .color(INK)
                    .align(TextAlign::Center),
            )
            .into()
    })
    .on_tap(move || state.answer(which))
    .into()
}

/// What U recorded, and the way to pass without recording anything. The way
/// to pass names the phase it is in: during the sketch a pass defers to a
/// later question, after it a pass declines the day's.
fn footer(state: &AppState, ctx: &BuildContext, asking: &Asking) -> WidgetNode {
    let noted = state.noted.get();
    let state_for_skip = state.clone();
    let skip_text = match asking {
        Asking::Sketch { .. } => "skip this one",
        Asking::Daily { .. } => "not today · skip",
    };
    let skip = Pressable::themed(ctx, move |press| {
        Text::new(skip_text)
            .size(10.5)
            .color(FAINT.with_alpha((150.0 + press * 105.0) as u8))
            .into()
    })
    .on_tap(move || state_for_skip.skip());

    let row = Flex::row()
        .main_axis_alignment(MainAxisAlignment::Center)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(10.0);
    let row = if let Some(noted) = noted {
        row.children(children![
            Text::new(format!("U noted — {noted}"))
                .size(10.5)
                .color(MEMBRANE),
            skip,
        ])
    } else {
        row.children(children![skip])
    };
    row.into()
}
