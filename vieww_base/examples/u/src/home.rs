//! Home — the screen the app opens on.
//!
//! The persona at a glance: the neuron it lives in (a live 3D render), how
//! coherent it is, what it remembers, and one button that trains it. The gear
//! at the top right is the only way out, and it leads to settings.
//!
//! # What moves, and why
//!
//! The background field is a pure function of the clock, the hero viewport
//! orbits, the coherence ring breathes, and the sync dot pulses — a screen
//! that is alive at rest, because a persona that only moves when poked reads
//! as a database. `calm` (from settings) dims the field to a third rather
//! than stopping it: stillness where the motion was is itself information.

use vieww::prelude::*;
use vieww_foundation::{Color, Offset};

use crate::brain;
use crate::neurons;
use crate::palette::{self, AXON, EDGE, FAINT, GROUND_2, INK, MEMBRANE, MUTED, SYNAPSE};
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
        let tone = state.tone.get();
        let calm = state.calm.get();
        let name = state.display_name();

        let field = if calm { 0.3 } else { 1.0 };
        #[allow(clippy::cast_precision_loss)]
        let sessions_f = sessions as f32;
        let coherence = (0.38 + 0.06 * sessions_f.min(9.0) + 0.012 * (t * 0.8).sin()).min(0.97);
        let memories = 120 + sessions * 3;
        let streak = (0.64 + 0.02 * (t * 0.5).sin()).clamp(0.0, 1.0);

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
                                        SizedBox::height(22.0),
                                        greeting(&name, tone, &theme),
                                        SizedBox::height(18.0),
                                        hero_card(&state, t, burst),
                                        SizedBox::height(14.0),
                                        vitals(coherence, memories, streak, &theme),
                                        Flexible::expanded(1).child(SizedBox::shrink()),
                                        train_button(&state, ctx, burst),
                                        SizedBox::height(10.0),
                                        Text::new(format!(
                                            "{} session{} today · everything stays on this device",
                                            sessions,
                                            if sessions == 1 { "" } else { "s" },
                                        ))
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

/// The wordmark, the live dot, and the gear.
fn top_bar(shared: &Shared, ctx: &BuildContext, t: f32) -> WidgetNode {
    let pulse = 0.5 + 0.5 * (t * 2.2).sin();

    // The gear: slowly turning at rest, a further turn as it is pressed, and
    // frosted glass so the field shows through the chip it sits in.
    let shared_for_gear = shared.clone();
    let angle = t * 0.12;
    let gear = Pressable::themed(ctx, move |press| {
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
                    .child(neurons::gear(21.0, SYNAPSE, angle + press * 0.8)),
            )
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
            Container::new()
                .color(MEMBRANE.with_alpha((110.0 + 90.0 * pulse) as u8))
                .radius(5.0)
                .size(10.0, 10.0),
            SizedBox::width(6.0),
            Text::new("synced").size(10.0).color(MUTED),
            SizedBox::width(12.0),
            gear,
        ])
        .into()
}

/// Who this one is for, and how it is listening.
fn greeting(name: &str, tone: usize, theme: &ThemeData) -> WidgetNode {
    let ear = match tone {
        0 => "listening closely, speaking plainly.",
        1 => "listening warmly, and remembering.",
        _ => "listening, and taking notes.",
    };
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(4.0)
        .children(children![
            Text::new("Good to see you,")
                .style(theme.text.body)
                .color(MUTED),
            Text::new(name).style(theme.text.headline).color(INK),
            SizedBox::height(2.0),
            Text::new(ear).style(theme.text.label).color(MUTED),
        ])
        .into()
}

/// The 3D neuron in its card, with the readouts a research prototype earns.
fn hero_card(state: &AppState, t: f32, burst: f32) -> WidgetNode {
    let firing = 9.5 + 3.2 * (t * 0.7).sin() + burst * 14.0;
    let theta = 0.03 * (t * 0.4).sin();
    let triangles = state.stats.borrow().triangles;
    // The whole card lifts a hair while the neuron is excited.
    let lift = 1.0 + 0.015 * burst;

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
        .padding(EdgeInsets::all(14.0))
        .child(
            Transformed::new(Transform::scale_around(
                Offset::new(190.0, 175.0),
                lift,
                lift,
            ))
            .child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(10.0)
                    .children(children![
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children(children![
                                Text::new("CORTICAL RENDER · LIVE")
                                    .size(9.0)
                                    .bold()
                                    .color(FAINT),
                                Flexible::expanded(1).child(SizedBox::shrink()),
                                Text::new("vieww-3d · software raster")
                                    .size(9.0)
                                    .color(FAINT),
                            ]),
                        Align::new(Alignment::CENTER).child(Clip::rounded(18.0).child(
                            Container::new().color(GROUND_2).child(brain::viewport(
                                &state.scene,
                                &state.stats,
                                t,
                                burst
                            )),
                        )),
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children(children![
                                Container::new()
                                    .color(MEMBRANE.with_alpha((120.0 + 120.0 * burst) as u8))
                                    .radius(3.0)
                                    .size(6.0, 6.0),
                                SizedBox::width(6.0),
                                Text::new(format!("firing {firing:.1} Hz"))
                                    .size(10.0)
                                    .color(MUTED),
                                Flexible::expanded(1).child(SizedBox::shrink()),
                                Text::new(format!("θ drift {theta:+.2}"))
                                    .size(10.0)
                                    .color(MUTED),
                                SizedBox::width(10.0),
                                Text::new(format!("{triangles} tris"))
                                    .size(10.0)
                                    .color(FAINT),
                            ]),
                    ]),
            ),
        )
        .into()
}

/// Coherence, memory, and the streak — three small vitals.
///
/// The ring card keeps its number *beside* the ring, not under it: the card
/// is ~93pt of inner width and a 44pt ring plus a number column will not fit
/// beside it — the overflow this layout shipped with first.
fn vitals(coherence: f32, memories: u32, streak: f32, theme: &ThemeData) -> WidgetNode {
    let ring = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(6.0)
        .children(children![
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(8.0)
                .children(children![
                    CircularProgress::new(coherence)
                        .size(34.0)
                        .thickness(4.0)
                        .label("coherence"),
                    Text::new(format!("{}%", (coherence * 100.0).round()))
                        .style(theme.text.title)
                        .color(INK),
                ]),
            Text::new("coherence").size(10.0).color(MUTED),
        ]);

    let memories = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(3.0)
        .children(children![
            Text::new(memories.to_string())
                .style(theme.text.title)
                .color(SYNAPSE),
            Text::new("memories held").size(10.0).color(MUTED),
            LinearProgress::new(0.42)
                .thickness(3.0)
                .label("memory load"),
        ]);

    let streak = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(3.0)
        .children(children![
            Text::new("7 days").style(theme.text.title).color(AXON),
            Text::new("training streak").size(10.0).color(MUTED),
            LinearProgress::new(streak).thickness(3.0).label("streak"),
        ]);

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(12.0)
        .children(children![
            Flexible::new(1).child(vital_card(ring)),
            Flexible::new(1).child(vital_card(memories)),
            Flexible::new(1).child(vital_card(streak)),
        ])
        .into()
}

/// One vital's raised surface.
fn vital_card(child: impl Into<WidgetNode>) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgba(18, 25, 44, 216))
                .radius(20.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(13.0))
        .child(child)
        .into()
}

/// The one action: train, and watch the neuron fire.
fn train_button(state: &AppState, ctx: &BuildContext, burst: f32) -> WidgetNode {
    let glow = 50.0 + 90.0 * burst;
    let state_for_tap = state.clone();
    Pressable::themed(ctx, move |press| {
        let press_down = 1.0 - press * 0.03;
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .gradient(palette::train_gradient())
                    .radius(22.0)
                    .shadow(Shadow::new(
                        Color::rgba(94, 234, 212, glow as u8),
                        Offset::new(0.0, 12.0),
                        26.0,
                    )),
            )
            .padding(EdgeInsets::symmetric(18.0, 15.0))
            .child(
                Transformed::new(Transform::scale_around(
                    Offset::new(190.0, 36.0),
                    press_down,
                    press_down,
                ))
                .child(
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            neurons::spark(22.0, Color::rgb(5, 18, 16)),
                            SizedBox::width(12.0),
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .spacing(1.0)
                                .children(children![
                                    Text::new("Train U")
                                        .size(16.0)
                                        .bold()
                                        .color(Color::rgb(5, 18, 16)),
                                    Text::new("a two-minute session, right here")
                                        .size(10.5)
                                        .color(Color::rgba(5, 24, 21, 200)),
                                ]),
                            Flexible::expanded(1).child(SizedBox::shrink()),
                            Icon::new(icons::chevron_right())
                                .size(16.0)
                                .color(Color::rgba(5, 24, 21, 190)),
                        ]),
                ),
            )
            .into()
    })
    .on_tap(move || state_for_tap.train())
    .into()
}
