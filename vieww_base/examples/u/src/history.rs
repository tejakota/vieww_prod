//! History — the room where saved decisions live, the plan's §2.1 third
//! area, as a screen at last.
//!
//! Reached from home's shortcuts, two doors into one room: HISTORY is every
//! decision the memory holds, SAVED is the ones the person marked kept.
//! Both lists are the store's own records, read at build time, newest
//! first — the room never caches what the memory can say.
//!
//! # The layout is the studio's own language
//!
//! The header and cards follow the workspace's make — the same card fill,
//! hairline rims, violet caps — because this is the fourth room of the
//! same house. The reference's "Your decisions" screen provides the shape:
//! the title, the brief, one card per decision with a tag line under its
//! own words, and a New decision bar at the bottom.
//!
//! Tapping a row brings the decision back to the workspace with the fields
//! the person actually typed — pick up where you left off, literally: the
//! workspace shows their words and the slate cleared of the last read, so
//! nothing pretends the old assessment is this decision's.

use std::cell::Cell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Axis, Color, FontWeight, Offset, Path, Size, TextStyle};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::palette::{self, AXON, CARD, EDGE, INK, MUTED};
use crate::stage;
use crate::{take, workspace_route, AppState, Shared};

/// Where a glyph's baseline sits below its line box's top — Inter's own
/// ascent, the same constant the other screens place text by.
const ASCENT: f32 = 0.969;

/// A text box top for a baseline at `baseline`.
fn top_of(baseline: f32, size: f32) -> f32 {
    baseline - ASCENT * size
}

#[derive(Debug)]
pub struct HistoryScreen {
    pub state: AppState,
    pub shared: Shared,
    /// The SAVED door: only the marked decisions.
    pub saved_only: bool,
}

#[widget]
impl HistoryScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();

        // The store's own records, read now — honest the moment a decision
        // lands or a save is made, and never cached anywhere else.
        let engine = state.engine.borrow();
        let saved_ids = engine
            .saved_decisions()
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| record.id)
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        let records = if self.saved_only {
            engine.saved_decisions()
        } else {
            engine.decisions()
        }
        .unwrap_or_default();
        let is_saved = |id: u64| saved_ids.contains(&id);
        drop(engine);

        let mut column = Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(12.0)
            .children(children![header(&state, self.saved_only, ctx)]);

        if records.is_empty() {
            column = column.push(empty_card(self.saved_only));
        } else {
            for record in &records {
                column = column.push(row(
                    &self.shared,
                    ctx,
                    record,
                    if self.saved_only {
                        false
                    } else {
                        is_saved(record.id)
                    },
                ));
            }
            column = column.push(new_decision_bar(&self.shared, ctx));
        }

        let scroller = Scrollable::vertical(state.hist.offset())
            .key("u-history-scroll")
            .on_drag(state.hist.on_drag())
            .on_drag_end(state.hist.on_drag_end())
            .on_extents(state.hist.on_extents())
            .child(Padding::new(EdgeInsets::symmetric(0.0, 4.0)).child(column));

        // The way back: the same swipe every pushed room leaves by.
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

/// The title and the one-line brief, the studio's own marks — and the
/// reference's own words for the brief: "A LITTLE CLARITY, OVER TIME".
fn header(state: &AppState, saved_only: bool, ctx: &BuildContext) -> WidgetNode {
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

    let (title, brief) = if saved_only {
        ("Saved decisions", "KEPT CLOSE")
    } else {
        ("Your decisions", "A LITTLE CLARITY, OVER TIME")
    };

    SizedBox::from_size(Size::new(420.0, 76.0))
        .child(Stack::new().fit(StackFit::Expand).children(children![
            place(24.0, 28.0).child(back),
            place(82.0, top_of(48.0, 20.0)).child(Text::new(title).style(
                TextStyle::new(20.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(-0.6)
                    .color(Color::rgb(240, 242, 250)),
            )),
            place(82.0, top_of(63.0, 9.5)).child(Text::new(brief).style(
                TextStyle::new(9.5).letter_spacing(0.8).color(Color::rgb(127, 140, 164)),
            )),
        ]))
        .into()
}

/// One decision: its own words, the tag and the day it happened, the saved
/// mark when the full list shows one, and the chevron that says "this
/// opens".
fn row(
    shared: &Shared,
    ctx: &BuildContext,
    record: &u_memory::DecisionRecord,
    saved: bool,
) -> WidgetNode {
    // The card is one line at 12 px in the same run home's strip allows:
    // 46 glyphs is the honest ceiling, and an ellipsis says the rest is in
    // the record, not gone.
    let shown: String = if record.situation.chars().count() > 46 {
        let cut: String = record.situation.chars().take(46).collect();
        format!("{cut}…")
    } else {
        record.situation.clone()
    };

    let tag = record
        .tags
        .first()
        .cloned()
        .unwrap_or_else(|| "decision".to_owned());
    let meta = format!("{tag} · {}", when(record.id));

    let shared_for_open = shared.clone();
    // The handlers are `'static`: the record's own words travel as a clone,
    // not as a borrow into them.
    let record_for_open = record.clone();
    let card = Pressable::themed(ctx, move |press| {
        // Built inside: the press builder runs on every press, so nothing
        // here is moved out of a capture.
        let chip: WidgetNode = if saved {
            saved_chip()
        } else {
            SizedBox::shrink().into()
        };
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(18, 27, 45, (255.0 - press * 20.0) as u8))
                    .radius(21.0)
                    .border(Border::new(
                        EDGE.with_alpha((19.0 + press * 110.0) as u8),
                        1.0,
                    )),
            )
            .padding(EdgeInsets::all(14.0))
            .child(
                Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(12.0)
                    .children(children![
                        tag_disc(&tag),
                        Flexible::expanded(1).child(
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .spacing(4.0)
                                .children(children![
                                    Text::new(shown.clone()).style(
                                        TextStyle::new(12.0)
                                            .weight(FontWeight::Medium)
                                            .color(Color::rgb(220, 227, 241)),
                                    ),
                                    Flex::row()
                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                        .spacing(8.0)
                                        .children(children![
                                            Text::new(meta.clone()).size(9.5).color(MUTED),
                                            chip,
                                        ]),
                                ]),
                        ),
                        Icon::new(icons::chevron_right()).size(16.0).color(MUTED),
                    ]),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_open);
        state.open_decision(&record_for_open);
        state.nav.push(workspace_route(shared_for_open.clone()));
    });

    card.into()
}

/// The round mark before a decision — the inferred tag's initial, in the
/// persona's own violet.
fn tag_disc(tag: &str) -> WidgetNode {
    let initial = tag.chars().next().unwrap_or('d').to_uppercase().to_string();
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(Color::rgb(48, 38, 74))
                .radius(13.0),
        )
        .size(26.0, 26.0)
        .alignment(Alignment::CENTER)
        .child(
            Text::new(initial)
                .style(
                    TextStyle::new(11.0)
                        .weight(FontWeight::Bold)
                        .color(Color::rgb(185, 167, 250)),
                )
                .align(TextAlign::Center),
        )
        .into()
}

/// The kept mark — said once, quietly.
fn saved_chip() -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(AXON.with_alpha(26))
                .radius(8.0)
                .border(Border::new(AXON.with_alpha(90), 1.0)),
        )
        .padding(EdgeInsets::symmetric(6.0, 3.0))
        .child(
            Text::new("SAVED")
                .style(
                    TextStyle::new(7.5)
                        .weight(FontWeight::Bold)
                        .letter_spacing(1.0)
                        .color(AXON),
                )
                .align(TextAlign::Center),
        )
        .into()
}

/// The honest empty room — nothing here is nothing here.
fn empty_card(saved_only: bool) -> WidgetNode {
    let (line, whisper) = if saved_only {
        (
            "Nothing saved yet",
            "A decision's workspace has the save that keeps it here.",
        )
    } else {
        (
            "No decisions yet",
            "Home's New decision brings U the first one.",
        )
    };
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(CARD)
                .radius(21.0)
                .border(Border::new(EDGE, 1.0)),
        )
        .padding(EdgeInsets::all(24.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(6.0)
                .children(children![
                    Text::new(line)
                        .style(TextStyle::new(14.0).weight(FontWeight::Medium).color(INK),)
                        .align(TextAlign::Center),
                    Text::new(whisper)
                        .style(TextStyle::new(10.5).color(MUTED).line_height(1.5))
                        .align(TextAlign::Center),
                ]),
        )
        .into()
}

/// The reference's own bottom bar: the one action this room also offers.
fn new_decision_bar(shared: &Shared, ctx: &BuildContext) -> WidgetNode {
    let shared_for_work = shared.clone();
    Pressable::themed(ctx, move |press| {
        let sink = 1.0 - press * 0.04;
        SizedBox::height(48.0)
            .child(
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
                                Text::new("New decision")
                                    .style(
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
        let state = take(&shared_for_work);
        state.nav.push(workspace_route(shared_for_work.clone()));
    })
    .into()
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

/// When a decision happened, in the words the reference uses: today,
/// yesterday, N days ago. The id is milliseconds since the epoch, which is
/// the one date fact this app can produce without a timezone library.
fn when(id: u64) -> String {
    let days = u_core::now().saturating_sub(id) / 86_400_000;
    match days {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        n => format!("{n} days ago"),
    }
}

/// A `Positioned` at the design's own coordinates.
fn place(left: f32, top: f32) -> Positioned {
    Positioned::new().left(left).top(top)
}
