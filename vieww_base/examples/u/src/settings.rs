//! Settings — the form that shapes the persona.
//!
//! Reached by the gear on home, left by the back button. Identity (a name),
//! persona (a voice and a decision style), training (a rhythm, feedback,
//! calm), and privacy (a lock, and a network that cannot be turned on
//! because there is none).
//!
//! The form is **controlled throughout**: every control is handed its value
//! from a signal and reports the change it was asked for, so nothing here
//! holds a second copy of anything. The screen scrolls with a real
//! `ScrollController`, so a fling survives the screen being covered and
//! returned to, and the soft keyboard pushing `view_insets` moves the content
//! rather than hiding it.

use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Color, TextEditingValue};

use crate::neurons;
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
        let name = state.name.get();

        // Dimmer than home: the form, not the biology, is the subject here.
        let field = if calm { 0.12 } else { 0.4 };

        let scroll = state.scroll.clone();
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
                                                    Padding::new(EdgeInsets::symmetric(0.0, 6.0,))
                                                        .child(form(&state, &name))
                                                ),
                                        ),
                                    ]),
                            ),
                        ),
                    ),
                ),
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
                    Text::new("shape who U becomes").size(10.5).color(MUTED),
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

/// The form itself: four cards and the line that signs them. Every write
/// goes through the state, so every change is an event in the log — the
/// form restores through the same fold the answers do.
fn form(state: &AppState, name: &TextEditingValue) -> WidgetNode {
    let tone = state.tone.get();
    let style = state.style.get();
    let daily = state.daily.get();
    let haptics = state.haptics.get();
    let calm = state.calm.get();
    let biometric = state.biometric.get();

    let name_write = state.clone();
    let tone_pick = state.clone();
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
            // ------------------------------------------------- identity
            section(
                "IDENTITY",
                vec![
                    setting_row(
                        "What should U call you?",
                        "it greets you, and signs its advice"
                    ),
                    SizedBox::height(10.0).into(),
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .color(Color::rgba(9, 13, 24, 235))
                                .radius(14.0)
                                .border(Border::new(EDGE, 1.0)),
                        )
                        .padding(EdgeInsets::symmetric(14.0, 5.0))
                        .child(
                            TextField::new(name.clone())
                                .placeholder("Your name")
                                .placeholder_color(MUTED)
                                .color(INK)
                                .single_line()
                                .on_changed(Rc::new(move |next| name_write.set_name(next))),
                        )
                        .into(),
                ],
            ),
            // --------------------------------------------------- persona
            section(
                "PERSONA",
                vec![
                    setting_row("Voice", "how U speaks when it answers"),
                    SizedBox::height(10.0).into(),
                    SegmentedControl::new(["Direct", "Warm", "Analytical"], tone)
                        .on_selected(Rc::new(move |index| tone_pick.set_tone(index)))
                        .into(),
                    SizedBox::height(16.0).into(),
                    setting_row("Decision style", "how U weighs a choice you bring it"),
                    SizedBox::height(10.0).into(),
                    choice(
                        "Fast and intuitive",
                        "gut calls, short debates",
                        style == 0,
                        Rc::new(move || style_fast.set_style(0)),
                    ),
                    SizedBox::height(8.0).into(),
                    choice(
                        "Deliberate and thorough",
                        "weighs every option twice",
                        style == 1,
                        Rc::new(move || style_slow.set_style(1)),
                    ),
                ],
            ),
            // -------------------------------------------------- training
            section(
                "TRAINING",
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
                    SizedBox::height(14.0).into(),
                    switch_row(
                        "Haptic feedback",
                        "a pulse when a memory lands",
                        haptics,
                        Rc::new(move |on| haptics_flip.set_haptics(on)),
                    ),
                    SizedBox::height(8.0).into(),
                    switch_row(
                        "Calm animations",
                        "dim the neuron field and its pulses",
                        calm,
                        Rc::new(move |on| calm_flip.set_calm(on)),
                    ),
                ],
            ),
            // --------------------------------------------------- privacy
            section(
                "PRIVACY",
                vec![
                    switch_row(
                        "Biometric lock",
                        "ask for your fingerprint to open U",
                        biometric,
                        Rc::new(move |on| biometric_flip.set_biometric(on)),
                    ),
                    SizedBox::height(8.0).into(),
                    // Locked on: no handler is the disabled state, which is the
                    // honest rendering of a guarantee rather than a preference.
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
                            Switch::new(true),
                        ])
                        .into(),
                    SizedBox::height(12.0).into(),
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Container::new()
                                .color(MEMBRANE.with_alpha(120))
                                .radius(3.0)
                                .size(6.0, 6.0),
                            SizedBox::width(7.0),
                            Text::new("Storage encrypted at rest").size(11.0).color(INK),
                            Flexible::expanded(1).child(SizedBox::shrink()),
                            Chip::new("SQLCipher · AES-256").selected(true),
                        ])
                        .into(),
                ],
            ),
            // ---------------------------------------------------- the act
            SizedBox::height(4.0),
            Text::new("U · on-device persona · research prototype")
                .size(10.0)
                .color(FAINT)
                .align(TextAlign::Center),
        ])
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

/// A setting's own label line.
fn setting_row(head: &str, caption: &str) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(1.0)
        .children(children![
            Text::new(head).size(13.0).bold().color(INK),
            Text::new(caption).size(10.5).color(MUTED),
        ])
        .into()
}

/// One decision-style choice: a radio and its text, the whole row tappable —
/// the radio alone is a 20-point target and a preference row is a fat one.
fn choice(head: &str, caption: &str, selected: bool, on_pick: Rc<dyn Fn()>) -> WidgetNode {
    let head = head.to_owned();
    let caption = caption.to_owned();
    let for_radio = on_pick.clone();
    let for_tap = on_pick;
    Pressable::new(move |_press| {
        Flex::row()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(11.0)
            .children(children![
                Radio::new(selected).on_selected({
                    let for_radio = for_radio.clone();
                    Rc::new(move |()| for_radio())
                }),
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .spacing(1.0)
                    .children(children![
                        Text::new(head.clone()).size(12.5).color(INK),
                        Text::new(caption.clone()).size(10.5).color(MUTED),
                    ]),
            ])
            .into()
    })
    .on_tap(move || for_tap())
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
