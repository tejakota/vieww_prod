//! The New Decision workspace — the plan's §2.3, as a screen.
//!
//! A person brings any situation, in their own words, with no category
//! chosen and no template filled: one free-form field, three optional ones,
//! and the one primary action. What comes back is the §2.4 workspace —
//! understanding, options (theirs and U's, marked), priorities, trade-offs,
//! unknowns, assumptions, the perspective, and one next step — never a chat
//! transcript, and never a number U made up.
//!
//! # The shape of the flow
//!
//! Assessing runs the deterministic engine: instant, so there is no
//! progress state to fake — §3's own rule is "do not imply unsupported
//! activity", and the honest version of that with an instant engine is to
//! have nothing to show. The one clarification U is allowed (§2.5) is asked
//! at the top of the result, with its answer folded back into the input and
//! the whole thing re-read: one question at a time, never re-asking what
//! was already said.
//!
//! # The layout
//!
//! The header and cards follow the studio's own language — the same card
//! fill, hairline rims, numbered violet labels — because the workspace is
//! the third room of the same house. It scrolls like the studio does; the
//! content is naturally sized, so the canvas is exactly as tall as what the
//! person has brought and what U had to say about it.

use std::cell::Cell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Axis, Color, FontWeight, Offset, Path, Size, TextStyle};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::palette::{self, AXON, CARD, EDGE, INK, MUTED, SOMA, SYNAPSE};
use crate::stage;
use crate::{take, AppState, Shared};

/// Where a glyph's baseline sits below its line box's top — Inter's own
/// ascent, the same constant the other screens place text by.
const ASCENT: f32 = 0.969;

/// A text box top for a baseline at `baseline`.
fn top_of(baseline: f32, size: f32) -> f32 {
    baseline - ASCENT * size
}

#[derive(Debug)]
pub struct WorkspaceScreen {
    pub state: AppState,
    pub shared: Shared,
}

#[widget]
impl WorkspaceScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();
        let t = state.time.get();

        let scroll = state.work.clone();

        let mut column = Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(14.0)
            .children(children![
                header(&state, ctx),
                situation_card(&state),
                assess_button(&self.shared, ctx),
                optional_card(&state),
            ]);

        // The verdict of the last assess, when it refused rather than
        // decided: said plainly, in the amber that means "you can fix this".
        if let Some(error) = state.assess_error.get() {
            column = column.push(error_line(&error));
        }

        if let Some(assessment) = state.assessment.get() {
            // The one question first — §2.5: ask only what materially
            // changes the assessment, one at a time.
            if let Some(question) = &assessment.follow_up_question {
                column = column.push(follow_up_card(
                    &self.shared,
                    &state,
                    question,
                ));
            }
            column = column
                .push(section(
                    "01  /  UNDERSTANDING",
                    prose(&assessment.understanding, INK),
                ))
                .push(options_card(&assessment))
                .push(list_section(
                    "03  /  WHAT MATTERS HERE",
                    &assessment.relevant_priorities,
                    AXON,
                ))
                .push(list_section(
                    "04  /  TRADE-OFFS",
                    &assessment.tradeoffs,
                    SYNAPSE,
                ))
                .push(list_section(
                    "05  /  WHAT'S MISSING",
                    &assessment.unknowns,
                    SOMA,
                ))
                .push(list_section(
                    "06  /  ASSUMED",
                    &assessment.assumptions,
                    MUTED,
                ))
                .push(section(
                    "07  /  U'S READ",
                    prose(&assessment.assessment, Color::rgb(233, 229, 245)),
                ))
                .push(next_step_card(&assessment))
                .push(feedback_row(&self.shared, &state, ctx));

            // The confidence, as the word it is, under the read itself.
            let _ = t;
        }

        let scroller = Scrollable::vertical(scroll.offset())
            .key("u-workspace-scroll")
            .on_drag(scroll.on_drag())
            .on_drag_end(scroll.on_drag_end())
            .on_extents(scroll.on_extents())
            .child(Padding::new(EdgeInsets::symmetric(0.0, 4.0)).child(column));

        // The way back: the mirror of the swipe that opened settings.
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

// ---- the header --------------------------------------------------------------

/// Back chevron, the title, and the one-line brief — the studio's own marks.
fn header(state: &AppState, ctx: &BuildContext) -> WidgetNode {
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

    SizedBox::from_size(Size::new(420.0, 76.0)).child(
        Stack::new()
            .fit(StackFit::Expand)
            .children(children![
                place(24.0, 28.0).child(back),
                place(82.0, top_of(48.0, 20.0)).child(Text::new("New decision").style(
                    TextStyle::new(20.0)
                        .weight(FontWeight::Bold)
                        .letter_spacing(-0.6)
                        .color(Color::rgb(240, 242, 250)),
                )),
                place(82.0, top_of(63.0, 9.5)).child(Text::new("BRING ANY SITUATION").style(
                    TextStyle::new(9.5)
                        .letter_spacing(0.8)
                        .color(Color::rgb(127, 140, 164)),
                )),
            ]),
    )
    .into()
}

/// A `Positioned` at the design's own coordinates.
fn place(left: f32, top: f32) -> Positioned {
    Positioned::new().left(left).top(top)
}

/// The back chevron — the studio's own stroke.
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

// ---- the input ---------------------------------------------------------------

/// The situation itself: one free-form field, the plan's example as its
/// whisper. Multiline, because situations are paragraphs.
fn situation_card(state: &AppState) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(CARD)
                .radius(21.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(10.0)
                .children(children![
                    label("THE SITUATION"),
                    SizedBox::height(132.0).child(
                        TextField::new(state.situation.get())
                            .size(12.5)
                            .color(INK)
                            .placeholder("I'm considering moving to another city, but I'm not \
                                          sure whether the change is worth it.")
                            .placeholder_color(Color::rgb(112, 126, 152))
                            .on_changed(Rc::new({
                                let state = state.clone();
                                move |next| state.set_situation(next)
                            })),
                    ),
                ]),
        )
        .into()
}

/// The optional structure, offered never demanded (§2.3): options one per
/// line, what matters, and the constraints.
fn optional_card(state: &AppState) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(CARD)
                .radius(21.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(12.0)
                .children(children![
                    optional_field(
                        state,
                        "OPTIONS — ONE PER LINE",
                        "stay · move · a third thing",
                        state.choices.get(),
                        |state, next| state.set_choices(next),
                    ),
                    optional_field(
                        state,
                        "WHAT MATTERS — MOST IMPORTANT FIRST",
                        "growth · the people close to you",
                        state.wants.get(),
                        |state, next| state.set_wants(next),
                    ),
                    optional_field(
                        state,
                        "CONSTRAINTS",
                        "budget · the lease until June",
                        state.bounds.get(),
                        |state, next| state.set_bounds(next),
                    ),
                ]),
        )
        .into()
}

/// One optional field: its label and a single-line editor.
fn optional_field(
    state: &AppState,
    title: &'static str,
    whisper: &'static str,
    value: vieww_foundation::TextEditingValue,
    write: impl Fn(&AppState, vieww_foundation::TextEditingValue) + 'static,
) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(5.0)
        .children(children![
            Text::new(title).style(
                TextStyle::new(8.5)
                    .weight(FontWeight::Bold)
                    .letter_spacing(1.5)
                    .color(Color::rgb(112, 126, 152)),
            ),
            TextField::new(value)
                .size(11.5)
                .color(INK)
                .placeholder(whisper)
                .placeholder_color(Color::rgb(96, 110, 136))
                .single_line()
                .on_changed(Rc::new({
                    let state = state.clone();
                    move |next| write(&state, next)
                })),
        ])
        .into()
}

/// The one primary action — the action gradient, as a bar this time.
fn assess_button(shared: &Shared, ctx: &BuildContext) -> WidgetNode {
    let shared_for_assess = shared.clone();
    Pressable::themed(ctx, move |press| {
        let sink = 1.0 - press * 0.04;
        SizedBox::height(48.0).child(
            Align::new(Alignment::CENTER).child(
                Transformed::new(Transform::scale_around(
                    Offset::new(186.0, 24.0),
                    sink,
                    sink,
                ))
                .child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .gradient(palette::action_gradient())
                                .radius(24.0)
                                .shadow(Shadow::new(
                                    Color::rgba(0, 0, 0, 122),
                                    Offset::new(0.0, 6.0),
                                    9.0,
                                )),
                        )
                        .width(372.0)
                        .height(48.0)
                        .alignment(Alignment::CENTER)
                        .child(
                            Text::new("Assess this").style(
                                TextStyle::new(15.0)
                                    .weight(FontWeight::Medium)
                                    .color(Color::rgb(255, 255, 255)),
                            )
                            .align(TextAlign::Center),
                        ),
                ),
            ),
        )
        .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_assess);
        state.assess();
    })
    .into()
}

/// A refusal, said plainly.
fn error_line(error: &str) -> WidgetNode {
    Text::new(error)
        .size(10.5)
        .color(SOMA)
        .align(TextAlign::Center)
        .into()
}

// ---- the result --------------------------------------------------------------

/// The one question, with its answer folded back in — §2.5's loop, one
/// question at a time.
fn follow_up_card(shared: &Shared, state: &AppState, question: &str) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgb(23, 22, 42))
                .radius(18.0)
                .border(Border::new(AXON.with_alpha(56), 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(10.0)
                .children(children![
                    label("ONE QUESTION"),
                    Text::new(question.to_owned())
                        .style(
                            TextStyle::new(13.0)
                                .weight(FontWeight::Medium)
                                .color(Color::rgb(226, 220, 245)),
                        )
                        .align(TextAlign::Center),
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .spacing(10.0)
                        .children(children![
                            Flexible::expanded(1).child(
                                TextField::new(state.follow_up.get())
                                    .size(11.5)
                                    .color(INK)
                                    .placeholder("Your answer — or leave it")
                                    .placeholder_color(Color::rgb(96, 110, 136))
                                    .single_line()
                                    .on_changed(Rc::new({
                                        let state = state.clone();
                                        move |next| state.set_follow_up(next)
                                    })),
                            ),
                            answer_button(shared),
                        ]),
                ]),
        )
        .into()
}

/// The answer button: folds the answer into the constraints and re-reads.
fn answer_button(shared: &Shared) -> WidgetNode {
    let shared_for_answer = shared.clone();
    Pressable::new(move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(42, 33, 68, (230.0 + press * 25.0) as u8))
                    .radius(13.0)
                    .border(Border::new(AXON.with_alpha((90.0 + press * 80.0) as u8), 1.0)),
            )
            .height(26.0)
            .padding(EdgeInsets::symmetric(14.0, 0.0))
            .alignment(Alignment::CENTER)
            .child(
                Text::new("FOLD IT IN").style(
                    TextStyle::new(9.5)
                        .weight(FontWeight::Bold)
                        .letter_spacing(1.0)
                        .color(Color::rgb(209, 194, 255)),
                ),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_answer);
        state.fold_answer();
    })
    .into()
}

/// The options, theirs and U's, told apart by mark — never by guess.
fn options_card(assessment: &Rc<u_decision::Assessment>) -> WidgetNode {
    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(8.0)
        .push(label("02  /  THE OPTIONS"));
    for option in &assessment.options {
        column = column.push(option_row(option));
    }
    card(column).into()
}

/// One option and where it came from.
fn option_row(option: &u_decision::Option_) -> WidgetNode {
    let (chip, chip_color) = match option.origin {
        u_decision::Origin::Theirs => ("YOURS", AXON),
        u_decision::Origin::Suggested => ("U'S — A SUGGESTION", SYNAPSE),
    };
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(10.0)
        .children(children![
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .color(chip_color.with_alpha(26))
                        .radius(8.0)
                        .border(Border::new(chip_color.with_alpha(90), 1.0)),
                )
                .padding(EdgeInsets::symmetric(7.0, 3.0))
                .child(
                    Text::new(chip).style(
                        TextStyle::new(7.5)
                            .weight(FontWeight::Bold)
                            .letter_spacing(1.0)
                            .color(chip_color),
                    ),
                ),
            Flexible::expanded(1).child(
                Text::new(option.label.clone())
                    .size(11.5)
                    .color(Color::rgb(226, 231, 242)),
            ),
        ])
        .into()
}

/// A list section: the label, then one line per entry, each with a marker
/// in the section's own colour — never a bullet that means nothing.
fn list_section(title: &str, entries: &[String], marker: Color) -> WidgetNode {
    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(7.0)
        .push(label(title));
    for entry in entries {
        column = column.push(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(9.0)
                .children(children![
                    marker_dot(marker),
                    Flexible::expanded(1).child(
                        Text::new(entry.clone())
                            .size(11.0)
                            .color(Color::rgb(203, 211, 224)),
                    ),
                ]),
        );
    }
    // An honest empty state: nothing here means nothing here.
    if entries.is_empty() {
        column = column.push(
            Text::new("None — and that is a real state, not a gap")
                .size(10.5)
                .color(Color::rgb(112, 126, 152)),
        );
    }
    card(column).into()
}

/// The marker: a four-point dot in the section's colour.
fn marker_dot(color: Color) -> Painting {
    Painting::sized(
        Size::square(12.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.circle(c, 2.0, color);
        }),
    )
}

/// A prose section: the label, then the paragraph.
fn section(title: &str, body: WidgetNode) -> WidgetNode {
    let column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(9.0)
        .push(label(title))
        .push(body);
    card(column).into()
}

/// The next step and the confidence word — the part the person acts on.
fn next_step_card(assessment: &Rc<u_decision::Assessment>) -> WidgetNode {
    let confidence = match assessment.confidence {
        u_decision::Confidence::High => "READ WITH CONFIDENCE",
        u_decision::Confidence::Medium => "READ WITH CARE",
        u_decision::Confidence::Low => "A THIN READ — SAY MORE AND IT SHARPENS",
    };
    let column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(9.0)
        .push(label("08  /  THE NEXT STEP"))
        .push(
            Text::new(assessment.next_step.clone())
                .style(
                    TextStyle::new(13.0)
                        .weight(FontWeight::Medium)
                        .color(Color::rgb(220, 250, 244)),
                )
                .align(TextAlign::Center),
        )
        .push(
            Text::new(confidence).style(
                TextStyle::new(8.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(1.6)
                    .color(Color::rgb(108, 230, 213)),
            )
            .align(TextAlign::Center),
        );
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgb(18, 43, 40))
                .radius(21.0)
                .border(Border::new(SYNAPSE.with_alpha(45), 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(column)
        .into()
}

/// The feedback row: the person's word for the assessment, recorded as
/// memory and taught to nothing (§7.3).
fn feedback_row(shared: &Shared, state: &AppState, ctx: &BuildContext) -> WidgetNode {
    if state.feedback_given.get() {
        return Text::new("Noted — thank you.")
            .size(10.5)
            .color(Color::rgb(129, 144, 168))
            .align(TextAlign::Center)
            .into();
    }

    let shared_for_helpful = shared.clone();
    let helpful = Pressable::themed(ctx, move |press| {
        pill_button("HELPFUL", SYNAPSE, press)
    })
    .on_tap(move || {
        let state = take(&shared_for_helpful);
        state.give_feedback(true);
    });

    let shared_for_not = shared.clone();
    let not_quite = Pressable::themed(ctx, move |press| {
        pill_button("NOT QUITE", AXON, press)
    })
    .on_tap(move || {
        let state = take(&shared_for_not);
        state.give_feedback(false);
    });

    Flex::row()
        .main_axis_alignment(MainAxisAlignment::Center)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(12.0)
        .children(children![helpful, not_quite])
        .into()
}

/// One feedback pill.
fn pill_button(text: &'static str, tint: Color, press: f32) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(tint.with_alpha((20.0 + press * 18.0) as u8))
                .radius(14.0)
                .border(Border::new(tint.with_alpha((70.0 + press * 60.0) as u8), 1.0)),
        )
        .height(28.0)
        .padding(EdgeInsets::symmetric(16.0, 0.0))
        .alignment(Alignment::CENTER)
        .child(
            Text::new(text).style(
                TextStyle::new(9.5)
                    .weight(FontWeight::Bold)
                    .letter_spacing(1.2)
                    .color(tint),
            ),
        )
        .into()
}

// ---- the shared parts --------------------------------------------------------

/// A card of the studio's own make.
fn card(child: impl Into<WidgetNode>) -> Container {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(CARD)
                .radius(21.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(16.0))
        .child(child)
}

/// A section's label — the studio's numbered, violet, letterspaced caps.
fn label(title: &str) -> WidgetNode {
    Text::new(title.to_owned()).style(
        TextStyle::new(9.0)
            .weight(FontWeight::Bold)
            .letter_spacing(2.0)
            .color(Color::rgb(155, 139, 210)),
    )
    .into()
}

/// One paragraph of prose at the workspace's reading size.
fn prose(text: &str, color: Color) -> WidgetNode {
    Text::new(text.to_owned()).style(
        TextStyle::new(11.5)
            .weight(FontWeight::Medium)
            .color(color)
            .line_height(1.55),
    )
    .into()
}
