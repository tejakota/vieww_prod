//! Home — the screen the app opens on.
//!
//! One decision, presented whole: a question, two answers, a way to pass.
//! Everything else on the screen is either who it is for (the greeting) or
//! what it adds up to (the day's progress). The persona's cell breathes
//! above the question — the whole 3D organ replaced by the one cell that
//! carried it, which a phone can paint without dropping frames.
//!
//! # What moves, and why
//!
//! The background field and the cell are pure functions of the stepped
//! clock, and the field fires when a choice lands: a screen alive at rest,
//! and one that reacts to the only thing it asked for. `calm` (from
//! settings) dims the field to a third rather than stopping it.

use vieww::prelude::*;
use vieww_foundation::{Color, Offset};

use crate::ask::{Ask, ASKS};
use crate::neurons;
use crate::palette::{self, EDGE, FAINT, INK, MEMBRANE, MUTED, SYNAPSE};
use crate::{settings_route, take, AppState, Shared};

#[derive(Debug)]
pub struct HomeScreen {
    pub state: AppState,
    pub shared: Shared,
}

#[widget]
impl HomeScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let theme = ThemeData::of(ctx);
        let state = self.state.clone();

        let t = state.time.get();
        let burst = state.burst.get();
        let sessions = state.sessions.get();
        let daily = state.daily.get();
        let index = state.ask.get() % ASKS.len();
        let noted = state.noted.get();
        let calm = state.calm.get();
        let name = state.display_name();

        let field = if calm { 0.3 } else { 1.0 };
        #[allow(clippy::cast_precision_loss)]
        let sessions_f = sessions as f32;
        let target = daily.round().max(1.0);

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
                                        greeting(&name, &theme),
                                        SizedBox::height(20.0),
                                        Flexible::expanded(1).child(
                                            Align::new(Alignment::CENTER,).child(question_card(
                                                &state,
                                                ctx,
                                                &ASKS[index],
                                                index,
                                                noted,
                                                t,
                                                burst,
                                                sessions_f,
                                                target,
                                            ))
                                        ),
                                        SizedBox::height(16.0),
                                        progress_row(sessions_f, target),
                                        SizedBox::height(10.0),
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

/// Who this one is for, and what is being asked of them.
fn greeting(name: &str, theme: &ThemeData) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(4.0)
        .children(children![
            Text::new("Good to see you,")
                .style(theme.text.body)
                .color(MUTED),
            Text::new(name).style(theme.text.headline).color(INK),
            SizedBox::height(2.0),
            Text::new("One small decision — that's how U learns you.")
                .style(theme.text.label)
                .color(MUTED),
        ])
        .into()
}

/// The day's question: the persona's cell, the prompt, two answers, and a
/// way to pass.
#[allow(clippy::too_many_arguments)]
fn question_card(
    state: &AppState,
    ctx: &BuildContext,
    ask: &Ask,
    index: usize,
    noted: Option<&'static str>,
    t: f32,
    burst: f32,
    sessions: f32,
    target: f32,
) -> WidgetNode {
    let remaining = (target - sessions).max(0.0);
    let header = if remaining >= 1.0 {
        format!("TODAY'S TRAINING · {remaining:.0} TO GO")
    } else {
        "BONUS ROUND · TARGET MET".to_owned()
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
                .spacing(16.0)
                .children(children![
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Text::new(header).size(9.0).bold().color(FAINT),
                            Flexible::expanded(1).child(SizedBox::shrink()),
                            Text::new(format!("{} of {}", index + 1, ASKS.len()))
                                .size(9.0)
                                .color(FAINT),
                        ]),
                    Align::new(Alignment::CENTER).child(neurons::persona_cell(112.0, t, burst)),
                    Text::new(ask.prompt)
                        .size(17.0)
                        .color(INK)
                        .align(TextAlign::Center),
                    choices(state, ctx, ask),
                    footer(state, ctx, noted),
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

/// What U recorded, and the way to pass without recording anything.
fn footer(state: &AppState, ctx: &BuildContext, noted: Option<&'static str>) -> WidgetNode {
    let state_for_skip = state.clone();
    let skip = Pressable::themed(ctx, move |press| {
        Text::new("not today · skip")
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

/// The day's arithmetic: how many, of how many.
fn progress_row(sessions: f32, target: f32) -> WidgetNode {
    let frac = (sessions / target).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let done = sessions as u32;
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(10.0)
        .children(children![
            Flexible::expanded(1).child(
                LinearProgress::new(frac)
                    .thickness(4.0)
                    .label("today's sessions"),
            ),
            Text::new(format!("{done} of {target:.0}"))
                .size(10.5)
                .color(MUTED),
        ])
        .into()
}
