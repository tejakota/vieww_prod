//! The first page (Home) and the last (a new session).
//!
//! ```text
//!  [Home] [session] [session] ... [new session]
//!    ^ what should exist?           ^ swipe past the last one to get here
//!      your sessions, Saved           and describe the next thing
//! ```

use std::rc::Rc;

use vieww::prelude::*;

use super::kit::*;
use super::session::Voice;
use super::theme::Pal;
use super::thread::{composer, tone_color};
use super::Ui;

const STARTERS: [(&str, &str, &str); 3] = [
    ("Daily digest", "Fetch, summarize, email on a schedule", "Every morning at 8, fetch the top Hacker News stories, summarize them, and email me the digest."),
    ("A tested CLI tool", "Code, tests, logging and CI in place", "A command line tool that counts words in files, with tests, logging and CI."),
    ("Weekly report", "From a folder of CSV files", "Every Monday, read the CSV files in my sales folder and email me a one-page summary."),
];

/// Tall enough for a starter's title and a two-line subtitle.
const STARTER_MIN_H: f32 = 82.0;

pub fn home(ui: &Ui) -> WidgetNode {
    let ui2 = ui.clone();
    let ui = ui.clone();
    let content: WidgetNode = Live::new("home", move || {
        let p = ui.pal();
        let send: Rc<dyn Fn()> = {
            let ui = ui.clone();
            Rc::new(move || ui.start_session_from(&ui.home))
        };
        let starters: Vec<WidgetNode> = STARTERS
            .iter()
            .map(|(title, sub, text)| {
                let prompt = ui.home.prompt.clone();
                let (title, sub, text) = (title.to_string(), sub.to_string(), text.to_string());
                grow(Pressable::sensed(move |s: Sense| {
                    // One height for all three: a tap target loosens its
                    // child's constraints, so a row's Stretch cannot reach
                    // the card, and a one-line subtitle would leave its card
                    // shorter than its neighbours. The floor fits a title
                    // and two lines of subtitle.
                    Constrained::new(Constraints::new(0.0, f32::INFINITY, STARTER_MIN_H, f32::INFINITY))
                        .child(Container::new()
                        .color(p.surface.lerp(p.surface2, s.hover))
                        .radius(corner::CARD)
                        .border(Border::new(if s.hover > 0.0 { p.line_strong } else { p.line }, 1.0))
                        .padding(EdgeInsets::all(space::L))
                        .child(column(space::XS, vec![b(title.clone(), size::BODY, p.ink), t(sub.clone(), size::SMALL, p.muted)]))
                        )
                        .into()
                })
                .on_tap(move || prompt.set(TextEditingValue::new(text.clone()))))
            })
            .collect();

        let sessions = ui.sessions.get();
        let cards: Vec<WidgetNode> = sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (status, tone) = s.status();
                let share = s.done_share();
                let title = s.title();
                let go = ui.clone();
                let color = tone_color(&p, tone);
                Pressable::sensed(move |st: Sense| {
                    Container::new()
                        .width(330.0)
                        .color(p.surface)
                        .radius(corner::CARD)
                        .border(Border::new(if st.hover > 0.0 { p.line_strong } else { p.line }, 1.0))
                        .padding(EdgeInsets::all(space::L))
                        .child(stretch(space::S, vec![
                            line(title.clone(), size::LEAD, p.ink),
                            line(status.clone(), size::SMALL, color),
                            meter_bar(p.surface2, color, share, 4.0),
                        ]))
                        .into()
                })
                .on_tap(move || go.go(i + 1))
                .into()
            })
            .collect();

        let mut items = vec![
            SizedBox::height(28.0).into(),
            Text::new("What should exist?").size(size::DISPLAY).color(p.ink).weight(FontWeight::Bold).into(),
            para("Describe the outcome in your own words. Wright drafts a plan, shows what it will touch and what it costs, and runs nothing until you approve.", size::LEAD, p.muted),
            speak_hero(&ui),
            composer(&ui, &ui.home, send, "Every Monday, read the CSVs in my sales folder and email me a one-page summary", true, false, Rc::new(|| ())),
            // Stretch: the three cards stand as tall as the tallest, so a
            // shorter subtitle does not leave one card floating in the row.
            stretch(space::S, vec![
                b("OR START FROM ONE", size::CAPTION, p.muted),
                Flex::row().cross_axis_alignment(CrossAxisAlignment::Start).spacing(space::M).children(starters).into(),
            ]),
            {
                let u = ui.clone();
                row(space::S, vec![
                    t(format!("Built in: {} agents of Wright's own and {} tools a plan can hand out.", wright_agents::WORKFORCE.len(), wright_core::palette::PALETTE.len()), size::SMALL, p.muted),
                    button(&p, "See agents & tools", Look::Quiet, true, move || u.open_agents_and_tools()),
                ])
            },
        ];
        if !cards.is_empty() {
            items.push(row(space::M, vec![b("SESSIONS", size::CAPTION, p.muted), t("Swipe sideways with two fingers to move between them; pinch to see them all", size::SMALL, p.muted)]));
            items.push(wrap_rows(cards, 2));
        }
        let saved = ui.saved.get();
        if !saved.is_empty() {
            items.push(row(space::M, vec![b("SAVED", size::CAPTION, p.muted), t("Closed sessions keep their conversation, plan, history and schedules. Open one to pick up where you left off.", size::SMALL, p.muted)]));
            items.push(saved_shelf(&ui, &saved));
        }
        stretch(space::L, items)
    })
    .into();
    scroll_area(
        &ui2,
        &ui2.home_scroll,
        pad(
            EdgeInsets::only(space::XXL, space::M, space::XXL, 60.0),
            reading(720.0, content),
        ),
    )
}

/// The mic, as the main way to start (the memo's item 6): one big
/// affordance that says SPEAK FIRST. It opens the same voice pipeline
/// the composer's mic icon uses — but it is the first thing a person
/// tries, not a small button beside a keyboard they were reaching for.
/// With built-in Whisper on this machine, speaking costs nothing and
/// leaks nothing; the transcript starts the session when it lands.
fn speak_hero(ui: &Ui) -> WidgetNode {
    let p = ui.pal();
    let (u, c) = (ui.clone(), ui.home.clone());
    let listening = c.voice.get() == Voice::Listening;
    let node: WidgetNode = match c.voice.get() {
        Voice::Listening => {
            let u2 = u.clone();
            let c2 = c.clone();
            Pressable::sensed(move |s: Sense| {
                let level = u.mic_level.get();
                Container::new()
                    .width(520.0)
                    .color(p.accent_soft)
                    .radius(corner::CARD)
                    .border(Border::new(p.accent, 1.0 + s.hover))
                    .padding(EdgeInsets::all(space::L))
                    .child(row(
                        space::M,
                        vec![
                            Glyph::Mic.draw(22.0, p.accent_ink),
                            grow(column(
                                space::XS,
                                vec![
                                    b(
                                        "Listening — say what should exist.".to_string(),
                                        size::LEAD,
                                        p.ink,
                                    ),
                                    t(
                                        "Tap to stop; Wright writes it down and starts.",
                                        size::SMALL,
                                        p.muted,
                                    ),
                                ],
                            )),
                            meter_bars(&p, level, u.clock.get()),
                        ],
                    ))
                    .into()
            })
            .on_tap(move || u2.stop_voice(&c2))
            .into()
        }
        Voice::Transcribing => {
            let u = u.clone();
            Container::new()
                .width(520.0)
                .color(p.surface)
                .radius(corner::CARD)
                .border(Border::new(p.line_strong, 1.5))
                .padding(EdgeInsets::all(space::L))
                .child(row(
                    space::M,
                    vec![
                        Glyph::spinner(22.0, p.accent_ink, u.clock.get()),
                        b("Writing it down...".to_string(), size::LEAD, p.ink),
                    ],
                ))
                .into()
        }
        _ => {
            let u3 = u.clone();
            let c3 = c.clone();
            Pressable::sensed(move |s: Sense| {
            Container::new()
                .width(520.0)
                // Tinted, not outlined like the composer under it: speaking
                // is the way in this page leads with, so it should not read
                // as one more box of the same kind.
                .color(p.accent_soft.lerp(p.surface, 0.35 - 0.35 * s.hover))
                .radius(corner::CARD)
                .border(Border::new(p.accent.with_alpha((90.0 + 165.0 * s.hover) as u8), 1.0))
                .padding(EdgeInsets::all(space::L))
                .child(row(space::M, vec![
                    Glyph::Mic.draw(22.0, p.accent_ink),
                    grow(column(space::XS, vec![
                        b("Speak instead of typing.".to_string(), size::LEAD, p.ink),
                        t("Tap, say what should exist, tap again — the transcript starts the interview. Nothing leaves this machine.", size::SMALL, p.muted),
                    ])),
                ]))
                .into()
        })
        .on_tap(move || u3.start_voice(&c3))
        .into()
        }
    };
    let _ = listening;
    node
}

/// Five bars that follow the voice, sized for the hero.
fn meter_bars(p: &Pal, level: f32, clock: f32) -> WidgetNode {
    super::thread::meter(p, level, clock)
}

/// Cards in rows of `per`.
fn wrap_rows(items: Vec<WidgetNode>, per: usize) -> WidgetNode {
    let mut rows = Vec::new();
    let mut it = items.into_iter().peekable();
    while it.peek().is_some() {
        rows.push(row(space::M, it.by_ref().take(per).collect()));
    }
    column(space::M, rows)
}

/// Saved sessions as chips: tap to reopen.
pub fn saved_shelf(ui: &Ui, saved: &[std::path::PathBuf]) -> WidgetNode {
    let p = ui.pal();
    let cards: Vec<WidgetNode> = saved
        .iter()
        .take(8)
        .map(|path| {
            let sum = super::session::saved_summary(path);
            let (u, path) = (ui.clone(), path.clone());
            Pressable::sensed(move |st: Sense| {
                let mut rows = vec![row(
                    space::S,
                    vec![
                        Glyph::Restore.draw(14.0, p.accent_ink),
                        grow(line(sum.title.clone(), size::BODY, p.ink)),
                    ],
                )];
                match &sum.last {
                    Some(last) => {
                        rows.push(line(last.clone(), size::SMALL, p.muted));
                        rows.push(t(
                            format!(
                                "{} message{} · open to continue",
                                sum.messages,
                                if sum.messages == 1 { "" } else { "s" }
                            ),
                            size::CAPTION,
                            p.muted,
                        ));
                    }
                    None => rows.push(t(
                        "No conversation kept · open to continue",
                        size::SMALL,
                        p.muted,
                    )),
                }
                Container::new()
                    .width(330.0)
                    .color(p.surface.lerp(p.surface2, st.hover))
                    .radius(corner::CARD)
                    .border(Border::new(
                        if st.hover > 0.0 {
                            p.line_strong
                        } else {
                            p.line
                        },
                        1.0,
                    ))
                    .padding(EdgeInsets::all(space::L))
                    .child(stretch(space::XS, rows))
                    .into()
            })
            .on_tap(move || u.reopen(&path))
            .into()
        })
        .collect();
    wrap_rows(cards, 2)
}

/// The same, small: one chip per saved session (for the overview, where
/// the sessions themselves take the room).
pub fn saved_chips(ui: &Ui, saved: &[std::path::PathBuf]) -> WidgetNode {
    let p = ui.pal();
    let chips: Vec<WidgetNode> = saved
        .iter()
        .take(6)
        .map(|path| {
            let name = super::session::saved_summary(path).title;
            let (u, path) = (ui.clone(), path.clone());
            Pressable::sensed(move |s: Sense| {
                Container::new()
                    .color(p.surface.lerp(p.surface2, s.hover))
                    .radius(16.0)
                    .border(Border::new(p.line_strong, 1.0))
                    .padding(EdgeInsets::only(10.0, 5.0, space::M, 5.0))
                    .child(row(
                        space::S,
                        vec![
                            Glyph::Restore.draw(14.0, p.accent_ink),
                            t(name.clone(), size::SMALL, p.ink),
                        ],
                    ))
                    .into()
            })
            .on_tap(move || u.reopen(&path))
            .into()
        })
        .collect();
    row(space::S, chips)
}

/// The page past the last session: start something new.
pub fn fresh(ui: &Ui) -> WidgetNode {
    let ui = ui.clone();
    Live::new("fresh", move || {
        let p = ui.pal();
        let pull = (-ui.drag.get() / 160.0).clamp(0.0, 1.0);
        let send: Rc<dyn Fn()> = {
            let ui = ui.clone();
            Rc::new(move || ui.start_session_from(&ui.fresh))
        };
        let ring = Container::new()
            .width(72.0 + 10.0 * pull)
            .height(72.0 + 10.0 * pull)
            .radius(41.0)
            .color(p.ground.lerp(p.accent_soft, pull))
            .border(Border::new(p.line_strong.lerp(p.accent, pull), 1.5))
            .alignment(Alignment::CENTER)
            .child(Glyph::Plus.draw(26.0, p.muted.lerp(p.accent_ink, pull)));
        pad(
            EdgeInsets::only(space::XXL, 60.0, space::XXL, 40.0),
            reading(680.0, stretch(space::L, vec![
                Center::new().child(ring).into(),
                Center::new().child(Text::new("A new session").size(size::HEADLINE).color(p.ink).weight(FontWeight::Bold)).into(),
                Center::new().child(t("Describe what should exist. It gets its own folder, its own plan and its own history.", size::BODY, p.muted)).into(),
                composer(&ui, &ui.fresh, send, "Describe what should exist", true, false, Rc::new(|| ())),
            ])),
        )
    })
    .into()
}
