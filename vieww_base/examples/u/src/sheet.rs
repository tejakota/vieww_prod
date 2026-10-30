//! The ask sheet — the one question, wherever it is summoned from.
//!
//! Home offers the asking through its action, its domains, its recent strip
//! and its nudge; this is what all of them open. A modal route over a scrim,
//! rising from the bottom: the phase's name and where it stands, the
//! companion's small signature, the prompt, two answers of equal weight, and
//! the way to pass.
//!
//! The card carries the same two phases home used to carry inline — the
//! **first sketch**, twelve answers counted once and never repeated, and the
//! **daily rhythm** after it, one question at a time against the day's
//! target. Between them stands the once-ever completion card, shown the
//! moment the twelfth answer lands and dismissed into the rhythm it begins.

use vieww::prelude::*;
use vieww_foundation::{Color, Offset};

use crate::ask::{Ask, ASKS};
use crate::neurons;
use crate::palette::{self, AXON, EDGE, FAINT, INK, MEMBRANE, MUTED};
use crate::{take, AppState, Shared};

/// Which asking phase the card is in — the whole of the sheet's state
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

/// The ask sheet: a modal route over home, rising from the bottom.
#[derive(Debug)]
pub struct AskSheet {
    pub state: AppState,
    pub shared: Shared,
}

#[widget]
impl AskSheet {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();
        let t = state.time.get();
        let burst = state.burst.get();

        let answers = state.answers.get();
        let complete = answers >= ASKS.len() as u32 && !state.seen_sketch.get();
        let asking = if answers < ASKS.len() as u32 {
            Asking::Sketch { done: answers }
        } else {
            Asking::Daily {
                done: state.sessions.get(),
                target: state.daily.get().round().max(1.0) as u32,
            }
        };

        let index = state.ask.get() % ASKS.len();
        let card = if complete {
            completion_card(&state, &self.shared, ctx, t, burst)
        } else {
            question_card(&state, ctx, &ASKS[index], &asking, t, burst)
        };

        // The scrim, and the card rising out of the bottom. The barrier is
        // the sheet's edge: tapping outside it is a decline, gentler than
        // either answer and quieter than the skip.
        let shared_for_dismiss = self.shared.clone();
        WidgetNode::new(
            Stack::new()
                .fit(StackFit::Expand)
                .push(
                    ModalBarrier::new()
                        .color(Color::rgba(3, 5, 10, 196))
                        .on_dismiss(move || {
                            take(&shared_for_dismiss).nav.pop();
                        }),
                )
                .push(
                    Positioned::fill().child(
                        SafeArea::new()
                            .child(Align::new(Alignment::BOTTOM_CENTER).child(
                                Padding::new(EdgeInsets::symmetric(20.0, 14.0)).child(card),
                            )),
                    ),
                ),
        )
    }
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

/// The day's question: the companion's signature, the prompt, two answers,
/// and a way to pass.
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
            *done as f32 / ASKS.len() as f32,
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
                    Color::rgba(0, 0, 0, 130),
                    Offset::new(0.0, 18.0),
                    34.0,
                )),
        )
        .padding(EdgeInsets::symmetric(18.0, 20.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(13.0)
                .children(children![
                    card_head(label, &counter, frac),
                    Align::new(Alignment::CENTER).child(neurons::sigil(46.0, t, burst)),
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
fn completion_card(
    state: &AppState,
    shared: &Shared,
    ctx: &BuildContext,
    t: f32,
    burst: f32,
) -> WidgetNode {
    let state_for_tap = state.clone();
    let shared_for_pop = shared.clone();
    Container::new()
        .decoration(
            BoxDecoration::new()
                .gradient(palette::card_gradient())
                .radius(26.0)
                .border(Border::new(EDGE, 1.0))
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, 130),
                    Offset::new(0.0, 18.0),
                    34.0,
                )),
        )
        .padding(EdgeInsets::symmetric(18.0, 20.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(13.0)
                .children(children![
                    card_head(
                        "FIRST SKETCH",
                        &format!("{} of {}", ASKS.len(), ASKS.len()),
                        1.0,
                    ),
                    Align::new(Alignment::CENTER).child(neurons::sigil(54.0, t, burst)),
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
                    .on_tap(move || {
                        state_for_tap.see_sketch();
                        take(&shared_for_pop).nav.pop();
                    }),
                ]),
        )
        .into()
}

/// The two answers, equal in weight — a choice with a right answer would
/// not be a preference. Equal weight, not equal invisibility: answering is
/// the one thing the sheet asks for, so the answers are the most button-like
/// objects on it.
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
