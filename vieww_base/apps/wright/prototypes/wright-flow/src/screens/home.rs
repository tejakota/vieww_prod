//! Wright · Flow: every screen of the revamped window, as one interactive
//! vieww app on mock data. Prototype only; the real UI is apps/wright/app.
//!
//! One self-contained file on purpose: `viewwstudio-cli` compiles a screen
//! file on its own against its own vieww, and treats a path that climbs to
//! the crate root or a parent module as a reach into the project around
//! it. So the parts
//! are sections of one namespace, in order: the window + Home/New, state
//! (and its clock), mock data, the kit (palette, components, vector
//! glyphs), the session, All sessions, Settings, and the overlays.
//!
//!   viewwstudio-cli animate src/screens/home.rs --width 1280 --height 800
//!   viewwstudio-cli web .

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use vieww::foundation::{DragDetails, ScrollEvent};
use vieww::prelude::*;
use vieww::widget::ElementState;

// ═════════════════════════════════════════════════════════════════
// HOME
// The window: top bar, the page, the status bar and the overlays — and
// the two composer pages, Home and New.



#[must_use]
pub fn screen() -> impl Widget {
    Window
}

#[derive(Debug)]
pub struct Window;

impl Widget for Window {
    fn debug_name(&self) -> &'static str {
        "WrightFlow"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn create_state(&self) -> Option<Box<dyn ElementState>> {
        Some(Box::new(FlowState::default()))
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let st: FlowState = ctx.state::<FlowState, _>(|s| s.clone()).unwrap_or_default();
        let act = Act(ctx.state_handle());
        let p = pal(st.mode, st.accent);
        let busy = st.sessions.iter().any(|s| matches!(s.stage, Stage::Running | Stage::Approval)) && !st.stopped;

        let body: WidgetNode = match st.screen {
            Screen::Home => home_page(&p, &act, &st),
            Screen::New => new_page(&p, &act, &st),
            Screen::Session(i) => session_page(&p, &act, &st, i),
            Screen::Overview => overview_page(&p, &act, &st),
            Screen::Engine => engine_page(&p, &act, &st),
        };
        // Each page change is an entrance: the new page rises into place.
        let body = appear(format!("page-{:?}-{}", st.screen, st.sessions.len()), body);

        let mut column: Vec<WidgetNode> = vec![top_bar(&p, &act, &st)];
        if st.stopped {
            column.push(stop_banner(&p, &act));
        }
        column.push(grow(body));
        column.push(status_bar(&p, &act, &st));

        let pp = p;
        let mut layers: Vec<WidgetNode> = vec![
            ColoredBox::new(p.ground).into(),
            Animated::new(if busy { 1.0 } else { 0.0 })
                .duration(ms(2400))
                .curve(Curve::SINE_IN_OUT)
                .build(move |k| Painting::new(PaintWith::new(move |b, s| aurora(&pp, b, s, k))).into())
                .into(),
            Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).children(column).into(),
        ];
        if st.attach_open {
            layers.push(attach_menu(&p, &act));
        }
        if st.settings {
            layers.push(overlay(&p, "settings", 1080.0, settings_sheet(&p, &act, &st), act.on(|s| s.settings = false)));
        }
        if st.help {
            layers.push(overlay(&p, "help", 720.0, help_sheet(&p, &act), act.on(|s| s.help = false)));
        }
        if st.receipt {
            layers.push(overlay(&p, "receipt", 680.0, receipt_sheet(&p, &act), act.on(|s| s.receipt = false)));
        }
        if let Some((msg, _)) = &st.toast {
            layers.push(toast(&p, &act, msg, st.undo.is_some()));
        }
        Stack::new().fit(StackFit::Expand).children(layers).into()
    }
}

// ── bars ─────────────────────────────────────────────────────────

fn top_bar(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let pages = st.sessions.len() + 2;
    let at = st.page_index();
    let pp = *p;
    let dots: Vec<WidgetNode> = (0..pages)
        .map(|k| {
            let on = k == at && !matches!(st.screen, Screen::Overview | Screen::Engine);
            Pressable::new(move |_| {
                Animated::new(if on { 1.0 } else { 0.0 })
                    .duration(ms(420))
                    .curve(Curve::EXPO_OUT)
                    .build(move |t| {
                        let c = Container::new().width(7.0 + 15.0 * t).height(7.0).radius(3.5);
                        pad(EdgeInsets::symmetric(2.0, 8.0), if t > 0.02 { c.gradient(pp.duo()) } else { c.color(pp.faint.with_alpha(110)) })
                    })
                    .into()
            })
            .on_tap(act.on(move |s| s.go_page(k)))
            .into()
        })
        .collect();
    let where_ = match st.screen {
        Screen::Home => "Home".to_string(),
        Screen::New => "New session".to_string(),
        Screen::Session(i) => st.sessions.get(i).map(|s| s.title.clone()).unwrap_or_default(),
        Screen::Overview => "All sessions".to_string(),
        Screen::Engine => "Engine".to_string(),
    };
    Container::new()
        .padding(EdgeInsets::symmetric(20.0, 12.0))
        .child(row(10.0, vec![
            Pressable::new({
                let pp = *p;
                move |_| Painting::sized(Size::new(22.0, 22.0), PaintWith::new(move |b, s| mark(&pp, b, s))).into()
            })
            .on_tap(act.on(|s| s.screen = Screen::Home))
            .into(),
            SizedBox::width(200.0).child(line(where_, 15.0, p.ink)).into(),
            gap(),
            row(0.0, dots),
            gap(),
            chip_button(p, Glyph::Grid, "All sessions", st.screen == Screen::Overview, act.on(|s| {
                s.screen = if s.screen == Screen::Overview { Screen::Home } else { Screen::Overview };
            })),
            chip_button(p, Glyph::Graph, "Engine", st.screen == Screen::Engine, act.on(|s| {
                s.screen = if s.screen == Screen::Engine {
                    if s.sessions.is_empty() { Screen::Home } else { Screen::Session(s.sessions.len() - 1) }
                } else {
                    Screen::Engine
                };
            })),
            icon_button(p, Glyph::Gear, "Settings", st.settings, act.on(|s| s.settings = true)),
            icon_button(p, Glyph::Help, "Gestures and shortcuts", st.help, act.on(|s| s.help = true)),
        ]))
        .into()
}

fn chip_button(p: &Pal, icon: Glyph, label: &str, on: bool, on_tap: impl Fn() + 'static) -> WidgetNode {
    let (p, label) = (*p, label.to_string());
    Pressable::sensed(move |s: Sense| {
        let c = Container::new().padding(EdgeInsets::symmetric(14.0, 8.0)).radius(999.0);
        let (c, ink) = if on { (c.gradient(p.duo()), p.on_accent) } else { (c.color(p.wash.lerp(p.edge, s.hover)).border(Border::new(p.edge, 1.0)), p.ink) };
        c.child(row(7.0, vec![icon.widget(16.0, ink), bold(label.clone(), 13.0, ink)])).into()
    })
    .on_tap(on_tap)
    .into()
}

fn stop_banner(p: &Pal, act: &Act) -> WidgetNode {
    appear(
        "stopped",
        Container::new()
            .color(p.bad.with_alpha(40))
            .border(Border::new(p.bad.with_alpha(140), 1.0))
            .padding(EdgeInsets::symmetric(20.0, 10.0))
            .child(row(12.0, vec![
                Glyph::Stop.widget(18.0, p.bad),
                bold("Everything is stopped", 14.0, p.ink),
                grow(text("Nothing runs, sends or fetches on this computer until you start again.", 13.0, p.muted)),
                button(p, "Start again", Look::Primary, act.on(|s| { s.stopped = false; s.say("Started again. Paused runs stay paused."); })),
            ]))
            .into(),
    )
}

fn status_bar(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let (where_, status, tone, spent) = match st.screen {
        Screen::Session(i) if i < st.sessions.len() => {
            let s = &st.sessions[i];
            let (text, tone, _) = s.status();
            (s.title.clone(), text.to_string(), p.tone(tone), Some(if s.run_no > 0 && s.stage == Stage::Done { 0.0041 * s.run_no as f32 } else { 0.0 }))
        }
        _ => (
            match st.screen {
                Screen::Overview => "All sessions".into(),
                Screen::Engine => "Engine".into(),
                Screen::New => "New session".into(),
                _ => "Home".into(),
            },
            format!("{} session{} · {} saved", st.sessions.len(), if st.sessions.len() == 1 { "" } else { "s" }, st.saved.len()),
            p.faint,
            None,
        ),
    };
    let pp = *p;
    let stop = Pressable::sensed(move |s: Sense| {
        Container::new()
            .padding(EdgeInsets::symmetric(10.0, 4.0))
            .radius(6.0)
            .color(pp.bad.with_alpha((235.0 * s.hover) as u8))
            .child(row(6.0, vec![Glyph::Stop.widget(12.0, pp.muted.lerp(Color::WHITE, s.hover)), text("Stop everything", 12.0, pp.muted.lerp(Color::WHITE, s.hover))]))
            .into()
    })
    .on_tap(act.on(|s| { s.stopped = true; s.say("Everything is stopped."); }));
    let flat = |label: String, icon: Glyph, f: Box<dyn Fn()>| -> WidgetNode {
        Pressable::sensed(move |s: Sense| {
            Container::new()
                .padding(EdgeInsets::symmetric(10.0, 4.0))
                .radius(6.0)
                .color(pp.wash.lerp(pp.edge, s.hover).with_alpha((255.0 * s.hover) as u8))
                .child(row(6.0, vec![icon.widget(12.0, pp.muted), text(label.clone(), 12.0, pp.muted)]))
                .into()
        })
        .on_tap(move || f())
        .into()
    };
    let mut right: Vec<WidgetNode> = Vec::new();
    if let Some(v) = spent {
        right.push(text(format!("${v:.4} today"), 12.0, p.muted));
    }
    right.push(flat(if st.mode == Mode::Dark { "Dark".into() } else { "Light".into() }, Glyph::Sun, Box::new(act.on(|s| s.mode = if s.mode == Mode::Dark { Mode::Light } else { Mode::Dark }))));
    right.push(flat("Settings".into(), Glyph::Gear, Box::new(act.on(|s| s.settings = true))));
    if !st.stopped {
        right.push(Container::new().width(1.0).height(14.0).color(p.edge).into());
        right.push(stop.into());
    }
    Container::new()
        .height(30.0)
        .color(p.ground.with_alpha(220))
        .border(Border::new(p.edge, 1.0))
        .child(
            Flex::row().cross_axis_alignment(CrossAxisAlignment::Center).spacing(12.0).children({
                let mut v: Vec<WidgetNode> = vec![
                    Container::new().height(30.0).padding(EdgeInsets::symmetric(12.0, 0.0)).gradient(Gradient::horizontal().with_stops(&[(0.0, p.a), (1.0, p.b)])).alignment(Alignment::CENTER).child(bold("wright", 12.0, p.on_accent)).into(),
                    text(where_, 12.0, p.ink),
                    dot(tone, 7.0),
                    grow(line(status, 12.0, p.muted)),
                ];
                v.extend(right);
                v.push(SizedBox::width(4.0).into());
                v
            }),
        )
        .into()
}

fn toast(p: &Pal, act: &Act, msg: &str, undo: bool) -> WidgetNode {
    let mut items = vec![text(msg.to_string(), 13.5, p.ink)];
    if undo {
        items.push(button(p, "Undo", Look::Secondary, act.on(|s| s.undo_close())));
    }
    let msg = msg.to_string();
    Align::new(Alignment::BOTTOM_CENTER)
        .child(pad(
            EdgeInsets::only(0.0, 0.0, 0.0, 46.0),
            appear(format!("toast-{msg}"), glass(p, 999.0, 0.0, pad(EdgeInsets::symmetric(18.0, 8.0), Flex::row().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Center).spacing(12.0).children(items))).shadow(Shadow::new(Color::rgba(0, 0, 0, 90), Offset::new(0.0, 10.0), 30.0)).into()),
        ))
        .into()
}

fn attach_menu(p: &Pal, act: &Act) -> WidgetNode {
    let pp = *p;
    let item = move |icon: Glyph, label: &'static str, f: Box<dyn Fn()>| -> WidgetNode {
        Pressable::sensed(move |s: Sense| {
            Container::new()
                .radius(10.0)
                .color(pp.wash.lerp(pp.edge, s.hover).with_alpha((255.0 * s.hover) as u8))
                .padding(EdgeInsets::symmetric(12.0, 10.0))
                .child(row(10.0, vec![icon.widget(18.0, pp.ink), text(label, 14.0, pp.ink)]))
                .into()
        })
        .on_tap(move || f())
        .into()
    };
    let scrim = Pressable::new(|_| SizedBox::expand().into()).on_tap(act.on(|s| s.attach_open = false));
    Stack::new()
        .fit(StackFit::Expand)
        .children(children![
            scrim,
            Positioned::new().left(250.0).top(390.0).child(appear(
                "attach",
                glass(p, 16.0, 8.0, stretch(2.0, vec![
                    item(Glyph::File, "Add files…", Box::new(act.on(|s| {
                        for f in MOCK_FILES.iter().take(2) {
                            if !s.files.iter().any(|x| x == f) {
                                s.files.push(f.to_string());
                            }
                        }
                        s.attach_open = false;
                    }))),
                    item(Glyph::Folder, "Add a folder…", Box::new(act.on(|s| {
                        if !s.files.iter().any(|x| x == "sales/") {
                            s.files.push("sales/".into());
                        }
                        s.attach_open = false;
                    }))),
                ]))
                .width(220.0)
                .shadow(Shadow::new(Color::rgba(0, 0, 0, 110), Offset::new(0.0, 16.0), 40.0))
                .into(),
            )),
        ])
        .into()
}

// ── the composer (Home and New) ──────────────────────────────────

pub fn composer(p: &Pal, act: &Act, st: &FlowState, placeholder: &str) -> WidgetNode {
    let filled = !st.prompt.is_empty();
    let pp = *p;
    let middle: WidgetNode = match st.voice {
        Voice::Listening => row(12.0, vec![
            meter(p, st.voice_t),
            grow(col(2.0, vec![bold("Listening — say what should exist.", 15.0, p.ink), text("Tap Stop and transcribe; it is written down on this machine.", 12.5, p.muted)])),
            button(p, "Stop and transcribe", Look::Secondary, act.on(|s| { s.voice = Voice::Transcribing; s.voice_t = 0.0; })),
        ]),
        Voice::Transcribing => row(12.0, vec![
            Container::new().width(120.0).child(progress(p, st.voice_t, 4.0)).into(),
            grow(text("Writing it down…", 15.0, p.muted)),
        ]),
        Voice::Idle => grow(
            TextField::text(st.prompt.clone())
                .size(17.0)
                .color(p.ink)
                .placeholder(placeholder.to_string())
                .placeholder_color(p.faint)
                .on_changed(act.text(|s, t| { s.prompt = t; s.picked = None; }))
                .on_submit(act.submit(|s, _| s.start())),
        ),
    };
    let mut parts = vec![
        icon_button(p, Glyph::Plus, "Add files or a folder", st.attach_open, act.on(|s| s.attach_open = !s.attach_open)),
        if st.voice == Voice::Idle { middle } else { grow(middle) },
    ];
    if st.voice == Voice::Idle {
        parts.push(icon_button(p, Glyph::Mic, "Speak instead of typing", false, act.on(|s| { s.voice = Voice::Listening; s.voice_t = 0.0; })));
        parts.push(
            Pressable::sensed(move |s: Sense| {
                Container::new()
                    .width(46.0)
                    .height(46.0)
                    .radius(23.0)
                    .gradient(pp.duo())
                    .shadow(Shadow::new(pp.b.with_alpha((80.0 + 90.0 * s.hover) as u8), Offset::ZERO, 20.0))
                    .child(Glyph::Arrow.widget(46.0, pp.on_accent))
                    .into()
            })
            .on_tap(act.on(|s| s.start()))
            .into(),
        );
    }
    let mut stack = vec![
        Container::new()
            .radius(24.0)
            .color(p.glass)
            .border(Border::new(if filled || st.voice != Voice::Idle { p.a.with_alpha(170) } else { p.edge }, 1.0))
            .shadow(Shadow::new(p.a.with_alpha(if filled { 80 } else { 30 }), Offset::new(0.0, 18.0), 50.0))
            .padding(EdgeInsets::only(12.0, 10.0, 10.0, 10.0))
            .child(row(12.0, parts))
            .into(),
    ];
    if !st.files.is_empty() {
        let chips: Vec<WidgetNode> = st
            .files
            .iter()
            .map(|f| {
                let name = f.clone();
                let n2 = f.clone();
                Container::new()
                    .radius(999.0)
                    .color(p.wash)
                    .border(Border::new(p.edge, 1.0))
                    .padding(EdgeInsets::only(10.0, 5.0, 6.0, 5.0))
                    .child(row(6.0, vec![
                        (if name.ends_with('/') { Glyph::Folder } else { Glyph::File }).widget(14.0, p.muted),
                        text(name, 12.5, p.ink),
                        Pressable::new({ let pp = *p; move |_| Glyph::Close.widget(16.0, pp.muted) }).on_tap(act.on(move |s| s.files.retain(|x| *x != n2))).into(),
                    ]))
                    .into()
            })
            .collect();
        stack.push(row(8.0, chips));
    }
    stretch(10.0, stack)
}

fn starters(p: &Pal, act: &Act, picked: Option<usize>) -> WidgetNode {
    let pp = *p;
    let cards: Vec<WidgetNode> = STARTERS
        .iter()
        .enumerate()
        .map(|(i, (title, sub, prompt))| {
            let (title, sub, prompt) = (title.to_string(), sub.to_string(), prompt.to_string());
            let on = picked == Some(i);
            grow(
                Pressable::sensed(move |s: Sense| {
                    let (title, sub) = (title.clone(), sub.clone());
                    Animated::new(if on { 1.0 } else { 0.45 * s.hover })
                        .duration(ms(340))
                        .curve(Curve::EXPO_OUT)
                        .build(move |k| {
                            Transformed::new(Transform::translate(Offset::new(0.0, -6.0 * k)))
                                .child(
                                    Container::new()
                                        .height(98.0)
                                        .radius(18.0)
                                        .color(pp.glass.lerp(pp.glass_hi, k))
                                        .border(Border::new(pp.edge.lerp(pp.a.with_alpha(220), k), 1.0))
                                        .shadow(Shadow::new(pp.a.with_alpha((70.0 * k) as u8), Offset::new(0.0, 14.0), 30.0))
                                        .padding(EdgeInsets::all(18.0))
                                        .child(col(6.0, vec![bold(title.clone(), 15.0, pp.ink), text(sub.clone(), 13.0, pp.muted)])),
                                )
                                .into()
                        })
                        .into()
                })
                .on_tap(act.on(move |s| { s.prompt = prompt.clone(); s.picked = Some(i); })),
            )
        })
        .collect();
    Flex::row().spacing(14.0).children(cards).into()
}

fn session_card(p: &Pal, act: &Act, i: usize, s: &Sess) -> WidgetNode {
    let (status, tone, share) = s.status();
    let (title, color, pp) = (s.title.clone(), p.tone(tone), *p);
    grow(
        Pressable::sensed(move |h: Sense| {
            Transformed::new(Transform::translate(Offset::new(0.0, -3.0 * h.hover)))
                .child(
                    glass(&pp, 18.0, 18.0, stretch(8.0, vec![line(title.clone(), 16.0, pp.ink), line(status, 13.0, color), progress(&pp, share, 4.0)]))
                        .border(Border::new(pp.edge.lerp(pp.a.with_alpha(180), h.hover), 1.0)),
                )
                .into()
        })
        .on_tap(act.on(move |s| s.open(i))),
    )
}

pub fn saved_shelf(p: &Pal, act: &Act, saved: &[Sess]) -> WidgetNode {
    let chips: Vec<WidgetNode> = saved
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let (title, pp) = (s.title.clone(), *p);
            Pressable::sensed(move |h: Sense| {
                Container::new()
                    .radius(999.0)
                    .color(pp.wash.lerp(pp.edge, h.hover))
                    .border(Border::new(pp.edge, 1.0))
                    .padding(EdgeInsets::symmetric(14.0, 7.0))
                    .child(row(8.0, vec![Glyph::Play.widget(12.0, pp.muted), text(title.clone(), 13.0, pp.ink)]))
                    .into()
            })
            .on_tap(act.on(move |s| s.reopen(k)))
            .into()
        })
        .collect();
    row(8.0, chips)
}

fn home_page(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let mut items = vec![
        row(8.0, vec![
            Container::new().width(8.0).height(8.0).radius(4.0).color(p.good).shadow(Shadow::new(p.good.with_alpha(170), Offset::ZERO, 10.0)).into(),
            text(format!("{} agents of Wright's own · {} tools a plan can hand out · runs on this machine", WORKFORCE.len(), TOOLS.len()), 13.0, p.muted),
            Pressable::new({ let pp = *p; move |_| bold("See agents & tools", 13.0, pp.live).into() }).on_tap(act.on(|s| { s.settings = true; s.tab = 1; })).into(),
        ]),
        bold("What should exist?", 54.0, p.ink),
        text("Describe the outcome in your own words. Wright drafts a plan, shows what it will touch and what it costs, and runs nothing until you approve.", 17.0, p.muted),
        SizedBox::height(6.0).into(),
        composer(p, act, st, "Describe the outcome in your own words…"),
        overline(p, "OR START FROM ONE"),
        starters(p, act, st.picked),
    ];
    if !st.sessions.is_empty() {
        items.push(row(10.0, vec![overline(p, "SESSIONS"), text("Swipe sideways with two fingers to move between them; pinch to see them all", 12.5, p.faint)]));
        let cards: Vec<WidgetNode> = st.sessions.iter().enumerate().map(|(i, s)| session_card(p, act, i, s)).collect();
        let mut rows = Vec::new();
        let mut it = cards.into_iter().peekable();
        while it.peek().is_some() {
            let mut r: Vec<WidgetNode> = it.by_ref().take(3).collect();
            while r.len() < 3 {
                r.push(gap());
            }
            rows.push(Flex::row().spacing(14.0).children(r).into());
        }
        items.push(stretch(14.0, rows));
    }
    if !st.saved.is_empty() {
        items.push(row(10.0, vec![overline(p, "SAVED"), text("Closed sessions keep their conversation, plan, history and schedules.", 12.5, p.faint)]));
        items.push(saved_shelf(p, act, &st.saved));
    }
    page(act, st, 0, 880.0, cascade("home", 16.0, items))
}

fn new_page(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    page(act, st, 1, 760.0, cascade("new", 18.0, vec![
        SizedBox::height(40.0).into(),
        Center::new().child(
            Container::new().width(76.0).height(76.0).radius(38.0).color(p.glass).border(Border::new(p.a.with_alpha(160), 1.5)).child(Glyph::Plus.widget(76.0, p.ink)),
        ).into(),
        Center::new().child(bold("A new session", 32.0, p.ink)).into(),
        Center::new().child(text("Describe what should exist. It gets its own folder, its own plan and its own history.", 15.0, p.muted)).into(),
        composer(p, act, st, "Describe what should exist"),
    ]))
}


// ═════════════════════════════════════════════════════════════════
// STATE
// The whole prototype's state, and the clock that moves it.
//
// One `ElementState` for every screen, so what is started on Home is what
// the session, All sessions, the engine and the status bar show. `tick`
// runs in the frame's animate phase: it advances a running plan (and
// stops at the approval gate), streams Wright's reply while it drafts,
// finishes a voice transcription and moves a model download along. A
// paused run simply is not advanced, so Pause and Resume are real.




#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    /// One of `sessions`.
    Session(usize),
    /// The page past the last session: describe the next thing.
    New,
    Overview,
    Engine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Wright is reading the message and drafting; the reply streams in.
    Drafting,
    /// The Confirm Gate: "This is exactly what will exist".
    Confirm,
    /// The blueprint exists; the plan card waits for Approve & run.
    Plan,
    /// Firing (or paused, see `Sess::paused`).
    Running,
    /// Stopped at the gate: the Mailer asks before it touches the world.
    Approval,
    Done,
    /// The approval was refused: nothing left the machine.
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Idle,
    Listening,
    /// Writing it down; `voice_t` counts to 1.
    Transcribing,
}

#[derive(Debug, Clone)]
pub struct Sess {
    pub title: String,
    pub request: String,
    pub stage: Stage,
    /// Run progress 0..1 (the approval gate sits at `GATE`).
    pub p: f32,
    /// How much of Wright's drafting reply has streamed, 0..1.
    pub draft: f32,
    pub paused: bool,
    pub run_no: u32,
    pub export_open: bool,
    pub exported: Option<usize>,
    pub built_open: bool,
    pub files: Vec<String>,
    /// The request for a change typed under the plan.
    pub change: Option<String>,
    pub composer: String,
}

impl Sess {
    pub fn new(request: &str, files: Vec<String>) -> Sess {
        Sess {
            title: title_for(request),
            request: request.to_string(),
            stage: Stage::Drafting,
            p: 0.0,
            draft: 0.0,
            paused: false,
            run_no: 0,
            export_open: false,
            exported: None,
            built_open: false,
            files,
            change: None,
            composer: String::new(),
        }
    }

    /// A session that already ran, for the mock history on first open.
    pub fn delivered(request: &str) -> Sess {
        let mut s = Sess::new(request, Vec::new());
        s.stage = Stage::Done;
        s.p = 1.0;
        s.draft = 1.0;
        s.run_no = 1;
        s
    }

    /// One line on where it stands, and the share done.
    pub fn status(&self) -> (&'static str, Tone, f32) {
        match self.stage {
            Stage::Drafting => ("Drafting the plan with you", Tone::Live, 0.05),
            Stage::Confirm => ("Needs you · confirm the blueprint", Tone::Wait, 0.1),
            Stage::Plan => ("Plan ready · not run yet", Tone::Quiet, 0.0),
            Stage::Running if self.paused => ("Paused", Tone::Wait, self.p),
            Stage::Running => ("Running", Tone::Live, self.p),
            Stage::Approval => ("Needs you · an approval", Tone::Wait, self.p),
            Stage::Done => ("Delivered and verified", Tone::Good, 1.0),
            Stage::Refused => ("Refused · nothing was sent", Tone::Bad, self.p),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Live,
    Wait,
    Good,
    Bad,
}

/// What a key looks like in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Missing,
    Ready,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub address: String,
    pub models: String,
    pub key: Key,
    pub yours: bool,
}

#[derive(Debug, Clone)]
pub struct FlowState {
    pub mode: Mode,
    pub accent: usize,
    pub screen: Screen,
    /// Where Settings / help / the engine go back to.
    pub sessions: Vec<Sess>,
    pub saved: Vec<Sess>,
    /// The last close, for Undo (index it came from, and the session).
    pub undo: Option<(usize, Sess)>,
    // Home and New share one composer.
    pub prompt: String,
    pub picked: Option<usize>,
    pub attach_open: bool,
    pub files: Vec<String>,
    pub voice: Voice,
    pub voice_t: f32,
    // overlays
    pub settings: bool,
    pub tab: usize,
    pub help: bool,
    pub receipt: bool,
    pub engine_kept: bool,
    pub stopped: bool,
    pub toast: Option<(String, f32)>,
    // settings
    pub providers: Vec<Provider>,
    pub editing_key: Option<usize>,
    pub key_text: String,
    pub adding_provider: bool,
    pub new_provider: (String, String),
    pub download: Option<f32>,
    pub downloaded: bool,
    pub no_key: usize,
    pub env: Vec<(String, String)>,
    pub env_new: (String, String),
    pub cert: Option<String>,
    pub per_run: String,
    pub per_day: String,
    pub schedules_on: bool,
    pub verify_all: bool,
    pub folder: String,
    pub tiers: [String; 3],
    /// Scroll offsets and their maxima, one slot per page (see `kit::scroller`).
    pub scroll: [f32; 16],
    pub scroll_max: [f32; 16],
    // the clock
    pub last: Option<Duration>,
    pub pending: bool,
}

impl Default for FlowState {
    fn default() -> Self {
        FlowState {
            mode: Mode::Dark,
            accent: 0,
            screen: Screen::Home,
            sessions: vec![Sess::delivered(HISTORY[0])],
            saved: vec![Sess::delivered(HISTORY[1])],
            undo: None,
            prompt: String::new(),
            picked: None,
            attach_open: false,
            files: Vec::new(),
            voice: Voice::Idle,
            voice_t: 0.0,
            settings: false,
            tab: 0,
            help: false,
            receipt: false,
            engine_kept: false,
            stopped: false,
            toast: None,
            providers: providers(),
            editing_key: None,
            key_text: String::new(),
            adding_provider: false,
            new_provider: (String::new(), String::new()),
            download: None,
            downloaded: false,
            no_key: 0,
            env: vec![("HTTPS_PROXY".into(), "http://proxy.example.com:3128".into())],
            env_new: (String::new(), String::new()),
            cert: None,
            per_run: "0.50".into(),
            per_day: "2.00".into(),
            schedules_on: false,
            verify_all: true,
            folder: "~/wright-projects".into(),
            tiers: ["deepseek-v4-flash".into(), "claude-haiku-5-5".into(), "claude-sonnet-5-5".into()],
            scroll: [0.0; 16],
            scroll_max: [0.0; 16],
            last: None,
            pending: false,
        }
    }
}

impl FlowState {
    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), 2.6));
    }

    /// Start a session from the shared composer (Home or New).
    pub fn start(&mut self) {
        let text = self.prompt.trim().to_string();
        if text.is_empty() {
            self.say("Describe what should exist first — or pick a starter.");
            return;
        }
        let files = std::mem::take(&mut self.files);
        self.sessions.push(Sess::new(&text, files));
        self.screen = Screen::Session(self.sessions.len() - 1);
        self.follow(self.sessions.len() - 1);
        self.prompt.clear();
        self.picked = None;
        self.attach_open = false;
    }

    pub fn current(&mut self) -> Option<&mut Sess> {
        match self.screen {
            Screen::Session(i) => self.sessions.get_mut(i),
            _ => None,
        }
    }

    pub fn close_session(&mut self, i: usize) {
        if i < self.sessions.len() {
            let s = self.sessions.remove(i);
            self.say(format!("Closed \"{}\" into Saved", s.title));
            self.undo = Some((i, s.clone()));
            self.saved.insert(0, s);
            if let Screen::Session(at) = self.screen {
                if at >= self.sessions.len() {
                    self.screen = if self.sessions.is_empty() { Screen::Home } else { Screen::Session(self.sessions.len() - 1) };
                }
            }
        }
    }

    pub fn undo_close(&mut self) {
        if let Some((i, s)) = self.undo.take() {
            if let Some(pos) = self.saved.iter().position(|x| x.title == s.title) {
                self.saved.remove(pos);
            }
            let at = i.min(self.sessions.len());
            self.sessions.insert(at, s);
            self.toast = None;
        }
    }

    pub fn reopen(&mut self, k: usize) {
        if k < self.saved.len() {
            let s = self.saved.remove(k);
            self.sessions.push(s);
            self.open(self.sessions.len() - 1);
        }
    }

    /// Page order for the dots and the swipe: Home, sessions, New.
    pub fn page_index(&self) -> usize {
        match self.screen {
            Screen::Home | Screen::Overview | Screen::Engine => 0,
            Screen::Session(i) => i + 1,
            Screen::New => self.sessions.len() + 1,
        }
    }

    pub fn go_page(&mut self, k: usize) {
        let n = self.sessions.len();
        if k == 0 {
            self.screen = Screen::Home;
        } else if k <= n {
            self.open(k - 1);
        } else {
            self.screen = Screen::New;
        }
    }

    /// Open session `i` at its newest card.
    pub fn open(&mut self, i: usize) {
        self.screen = Screen::Session(i);
        self.follow(i);
    }

    pub fn scroll_by(&mut self, key: usize, dy: f32) {
        let k = key % 16;
        self.scroll[k] = (self.scroll[k].min(self.scroll_max[k]) + dy).clamp(0.0, self.scroll_max[k]);
    }

    /// Follow session `i` to its newest card, the way the window keeps the
    /// latest of a conversation in view.
    pub fn follow(&mut self, i: usize) {
        self.scroll[(5 + i) % 16] = 1.0e9;
    }

    /// Record a page's scroll range; true only when it changed (so a
    /// layout that reports the same range does not rebuild forever).
    pub fn set_extent(&mut self, key: usize, max: f32) -> bool {
        let k = key % 16;
        if (self.scroll_max[k] - max).abs() < 0.5 {
            return false;
        }
        self.scroll_max[k] = max;
        true
    }

    pub fn animating(&self) -> bool {
        self.toast.is_some()
            || self.voice != Voice::Idle
            || self.download.is_some()
            || self.sessions.iter().any(|s| s.stage == Stage::Drafting || (s.stage == Stage::Running && !s.paused && !self.stopped))
    }

    fn advance(&mut self, dt: f32) {
        if let Some((_, left)) = &mut self.toast {
            *left -= dt;
            if *left <= 0.0 {
                self.toast = None;
                self.undo = None;
            }
        }
        if self.voice == Voice::Listening {
            self.voice_t += dt;
        }
        if self.voice == Voice::Transcribing {
            self.voice_t += dt / 1.4;
            if self.voice_t >= 1.0 {
                self.voice = Voice::Idle;
                self.voice_t = 0.0;
                self.prompt = TRANSCRIPT.to_string();
            }
        }
        if let Some(d) = &mut self.download {
            *d += dt / 4.0;
            if *d >= 1.0 {
                self.download = None;
                self.downloaded = true;
            }
        }
        let stopped = self.stopped;
        let mut moved = Vec::new();
        for (i, s) in self.sessions.iter_mut().enumerate() {
            let before = s.stage;
            match s.stage {
                Stage::Drafting => {
                    s.draft += dt / 2.8;
                    if s.draft >= 1.0 {
                        s.draft = 1.0;
                        s.stage = Stage::Confirm;
                    }
                }
                Stage::Running if !s.paused && !stopped => {
                    let was = s.p;
                    s.p = (s.p + dt / 6.0).min(1.0);
                    if was < GATE && s.p >= GATE {
                        s.p = GATE;
                        s.stage = Stage::Approval;
                    } else if s.p >= 1.0 {
                        s.stage = Stage::Done;
                    }
                }
                _ => {}
            }
            if s.stage != before {
                moved.push(i);
            }
        }
        for i in moved {
            self.follow(i);
        }
    }
}

impl ElementState for FlowState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
    fn tick(&mut self, now: Duration) -> bool {
        let dt = match self.last {
            Some(prev) => (now.saturating_sub(prev)).as_secs_f32().min(0.1),
            None => 0.0,
        };
        self.last = Some(now);
        if !self.animating() {
            return false;
        }
        self.advance(dt);
        true
    }
    fn is_animating(&self) -> bool {
        self.animating()
    }
}

/// A clonable handle handlers use to change the state.
#[derive(Clone)]
pub struct Act(pub Option<Rc<RefCell<dyn ElementState>>>);

impl Act {
    pub fn with(&self, f: impl FnOnce(&mut FlowState)) {
        if let Some(h) = &self.0 {
            if let Some(s) = h.borrow_mut().as_any_mut().downcast_mut::<FlowState>() {
                f(s);
                s.pending = true;
            }
        }
    }
    /// Change the state, rebuilding only when `f` says something changed.
    pub fn with_if(&self, f: impl FnOnce(&mut FlowState) -> bool) {
        if let Some(h) = &self.0 {
            if let Ok(mut b) = h.try_borrow_mut() {
                if let Some(s) = b.as_any_mut().downcast_mut::<FlowState>() {
                    if f(s) {
                        s.pending = true;
                    }
                }
            }
        }
    }

    /// A tap handler.
    pub fn on(&self, f: impl Fn(&mut FlowState) + 'static) -> impl Fn() + 'static {
        let me = self.clone();
        move || me.with(|s| f(s))
    }
    /// A text-field change handler.
    pub fn text(&self, f: impl Fn(&mut FlowState, String) + 'static) -> Rc<dyn Fn(vieww::prelude::TextEditingValue)> {
        let me = self.clone();
        Rc::new(move |v: vieww::prelude::TextEditingValue| me.with(|s| f(s, v.text.clone())))
    }
    pub fn submit(&self, f: impl Fn(&mut FlowState, String) + 'static) -> Rc<dyn Fn(String)> {
        let me = self.clone();
        Rc::new(move |v: String| me.with(|s| f(s, v)))
    }
}


// ═════════════════════════════════════════════════════════════════
// DATA
// Mock data, in Wright's own words (taken from the real UI and its
// agents, tools and receipt checks), so the prototype reads like the app.


/// Where the run waits for the Mailer's approval.
pub const GATE: f32 = 0.72;

pub const STARTERS: [(&str, &str, &str); 3] = [
    ("Daily digest", "Fetch, summarize, email on a schedule",
     "Every morning at 8, fetch the top Hacker News stories, summarize them, and email me the digest."),
    ("A tested CLI tool", "Code, tests, logging and CI in place",
     "A command line tool that counts words in files, with tests, logging and CI."),
    ("Weekly report", "From a folder of CSV files",
     "Every Monday, read the CSV files in my sales folder and email me a one-page summary."),
];

/// Sessions that exist before the person does anything: one open, one saved.
pub const HISTORY: [&str; 2] = [
    "Every Monday, read the CSV files in my sales folder and email me a one-page summary.",
    "A command line tool that counts words in files, with tests, logging and CI.",
];

pub const TRANSCRIPT: &str = "Every morning at 8, fetch the top Hacker News stories, summarize them, and email me the digest.";

pub const MOCK_FILES: [&str; 3] = ["sales-q3.csv", "sales-q4.csv", "brand-notes.md"];

/// A short title from a request, the way the window names sessions.
pub fn title_for(request: &str) -> String {
    let r = request.to_lowercase();
    if r.contains("hacker news") {
        "HN Daily Digest".into()
    } else if r.contains("csv") || r.contains("sales") {
        "Weekly Sales Report".into()
    } else if r.contains("count") && r.contains("words") {
        "Word Count CLI".into()
    } else {
        let mut t: String = request.split([',', '.']).next().unwrap_or(request).chars().take(28).collect();
        if request.chars().count() > 28 {
            t.push('…');
        }
        t
    }
}

/// Wright's reply while it drafts, streamed in as `draft` grows.
pub fn reply_for(title: &str) -> String {
    format!(
        "Got it: {}. I'll draft a plan with one reader and one writer, keep the internet to the one place that needs it, and ask you before anything is sent. Here's exactly what will exist:",
        match title {
            "HN Daily Digest" => "a morning digest of the top Hacker News stories, by email",
            "Weekly Sales Report" => "a one-page weekly summary of the sales folder",
            "Word Count CLI" => "a tested word-count command line tool",
            _ => "the project you described",
        }
    )
}

/// What the pending line says at each point of the draft.
pub fn drafting_line(draft: f32) -> &'static str {
    if draft < 0.25 {
        "Reading your message"
    } else if draft < 0.7 {
        "Drafting the plan"
    } else {
        "Writing the blueprint"
    }
}

/// The Confirm Gate's lines: (text, needs care).
pub fn confirm_lines(title: &str) -> Vec<(&'static str, bool)> {
    let mut v = vec![
        ("A project folder at ~/wright-projects/", false),
        ("Fetcher reads news.ycombinator.com (uses the internet)", true),
        ("Summarizer writes digest.md inside the project", false),
        ("Testwright runs the project's tests before anything leaves", false),
        ("Reviewer approves before delivery", false),
        ("Mailer sends one email to you@example.com — asks you first", true),
        ("Runs every day at 08:00 (Asia/Kolkata)", false),
        ("Never more than $0.50 a run, $2.00 a day", false),
    ];
    if title == "Weekly Sales Report" {
        v[1] = ("Reader reads ~/sales/*.csv (read-only)", false);
        v[6] = ("Runs every Monday at 09:00 (Asia/Kolkata)", false);
    }
    v
}

/// The plan's steps: (name, agent, what it does, pill, pill kind).
pub const STEPS: [(&str, &str, &str, &str, PillKind); 4] = [
    ("Collect", "Fetcher", "reads the top 30 stories", "uses the internet", PillKind::Wait),
    ("Summarize", "Summarizer", "writes digest.md", "sandboxed", PillKind::Good),
    ("Check", "Testwright · Reviewer", "tests, then a review", "added by Wright", PillKind::Quiet),
    ("Send", "Mailer", "one email to you", "asks you first", PillKind::Wait),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillKind {
    Good,
    Wait,
    Quiet,
}

/// "How it was built": the run told as events.
pub const BUILT: [(&str, &str, &str); 9] = [
    ("+0.0s", "wright", "run 1 started from blueprint a3f9c21"),
    ("+0.1s", "Shipwright", "laid the skeleton: 5 file(s), git initialized"),
    ("+0.4s", "Fetcher", "read 30 stories (http-read, 1 host)"),
    ("+1.9s", "Summarizer", "wrote artifacts/digest.md (412 words)"),
    ("+2.3s", "Testwright", "ran 6 tests: 6 passed"),
    ("+2.6s", "Reviewer", "approve — the digest matches its contract"),
    ("+2.6s", "Mailer", "asked: send one email to you@example.com"),
    ("+9.8s", "Mailer", "allowed by you; sent (smtp, 1 message)"),
    ("+9.9s", "wright", "done: 6 ran, 0 cached, cost $0.0041 · receipt signed"),
];

/// The engine's live log while a run fires.
pub const LOG: [(&str, &str); 8] = [
    ("Shipwright", "laid the skeleton: 5 file(s)"),
    ("Fetcher", "GET news.ycombinator.com/best — 200"),
    ("Fetcher", "kept 30 stories, 2 duplicates dropped"),
    ("Summarizer", "drafting digest.md"),
    ("Summarizer", "wrote 412 words"),
    ("Testwright", "6 tests passed"),
    ("Reviewer", "approve"),
    ("Mailer", "waiting for your approval"),
];

/// The receipt's checks, in `wright verify`'s own words.
pub const CHECKS: [(&str, &str); 7] = [
    ("signature", "Ed25519 signature by fbf3d43eebed — this machine's own signing key; verifies over the receipt's exact bytes"),
    ("event log", "events.jsonl still hashes to 550f1c378211 as signed"),
    ("artifacts", "digest.md hashes to 9c41e07d as recorded"),
    ("inputs", "the plan consumed no upstream inputs; nothing to check"),
    ("blueprint", "the plan is committed in this project's history as a3f9c21"),
    ("identities", "6 task-scoped identities, each delegated by the signer and revoked at its task's end"),
    ("sampling", "6 sampling settings recorded, matching the model calls in the log"),
];

/// The export menu: (label, what it is, needs a passed run).
pub const EXPORTS: [(&str, &str, bool); 7] = [
    ("Project as .zip", "The source, without logs, caches or builds", false),
    ("Python wheel", "A .whl that pip installs, with its command", true),
    ("Rust crate", "A .crate from cargo package", true),
    ("Web build", "npm install + npm run build, then dist/ as a .zip", true),
    ("Docker", "A Dockerfile running the project, tests at build time", true),
    ("systemd", "A systemd service and timer for its schedule", true),
    ("GitHub Actions", "A GitHub Actions workflow running its tests", true),
];

/// Wright's own agents: (name, what it does, when it joins).
pub const WORKFORCE: [(&str, &str, &str); 9] = [
    ("Interviewer", "Turns a user's words into a confirmed blueprint.", "joins at the DECLARE phase"),
    ("Architect", "Shapes the task graph: agents, edges, model tiers.", "joins at the COMPOSE phase"),
    ("Shipwright", "Scaffolds the materialization: the universal project skeleton.", "joins at materialization"),
    ("Testwright", "Writes and runs the checks that define 'done'.", "joins at the tests chip"),
    ("Logwright", "Instruments the firing: structured events, no log spam.", "joins at the logging chip"),
    ("Reviewer", "Reads everything agents wrote; enforces the readability contract.", "joins at the tests chip"),
    ("Firewright", "Turns a failure into a small, explained fix.", "joins when a step fails"),
    ("Operator", "Runs schedules and keeps unattended runs honest.", "joins with a schedule"),
    ("Explainer", "Tells the run's story in plain words.", "joins at the end of a run"),
];

/// Every tool a plan can grant: (name, what, effect).
pub const TOOLS: [(&str, &str, &str); 14] = [
    ("write-output", "Write the artifacts the blueprint declared", "project only"),
    ("files-read", "Read files inside the project", "project only"),
    ("files-list", "List folders inside the project", "project only"),
    ("files-write", "Write files inside the project", "project only"),
    ("edit", "Change part of a file", "project only"),
    ("apply-patch", "Apply a patch, all or nothing", "project only"),
    ("git", "Run git with an allow-listed set of flags", "sandboxed"),
    ("run-tests", "Run the project's own tests", "sandboxed"),
    ("shell", "Run a command in the sandbox", "sandboxed"),
    ("http-read", "Read a public web page", "uses the internet"),
    ("http-post", "Send data to an address", "touches the world · asks first"),
    ("browser", "Render a page and screenshot it", "uses the internet"),
    ("worker", "Hand a step to claude-code, codex, kimi-code or zcode", "sandboxed copy"),
    ("mcp-call", "Call an allow-listed tool on a pinned MCP server", "hash-pinned"),
];

pub fn providers() -> Vec<Provider> {
    vec![
        Provider { name: "anthropic".into(), address: "https://api.anthropic.com".into(), models: "claude-*".into(), key: Key::Missing, yours: false },
        Provider { name: "openai".into(), address: "https://api.openai.com/v1".into(), models: "gpt-*, o*".into(), key: Key::Missing, yours: false },
        Provider { name: "deepseek".into(), address: "https://api.deepseek.com".into(), models: "deepseek-*".into(), key: Key::Ready, yours: false },
    ]
}

pub const SHORTCUTS: [(&str, &str); 8] = [
    ("Two-finger swipe", "Swipe left or right to change page. Vertical scrolling stays in the conversation; arrow keys work too."),
    ("Swipe past the last", "The final dot opens a fresh session. The highlighted dot always follows your page."),
    ("Pinch (or O)", "Pinch in to see all sessions; pinch out to return to your page. × saves a session from the overview."),
    ("Hold Space", "Peek at the live agents and tools. E keeps them open."),
    ("Drag an approval", "Drag right to allow or left to refuse; the card previews the choice. The buttons do the same."),
    ("+ in the composer", "Attach files or a folder, or drop them on the window."),
    ("Ctrl/Cmd + Shift + T", "Reopen the last closed session."),
    ("Ctrl/Cmd + ,", "Open Settings. Esc backs out of an overlay; gestures resume when it closes."),
];

pub const TABS: [&str; 9] = [
    "Models & keys", "Agents & tools", "Defaults", "Appearance", "Network", "Fleet", "Organization", "Workspace", "This project",
];

pub const ACCENTS: [(&str, u32, u32); 4] = [
    ("Aurora", 0x7C5CFF, 0x22D3EE),
    ("Ember", 0xF43F8E, 0xF5A623),
    ("Lagoon", 0x0EA5E9, 0x34E6A0),
    ("Graphite", 0x9AA0B8, 0x5B6178),
];


// ═════════════════════════════════════════════════════════════════
// KIT
// The Flow look: palettes (dark and light), components and vector icons.




// ── palette ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub ground: Color,
    pub glass: Color,
    pub glass_hi: Color,
    pub edge: Color,
    pub ink: Color,
    pub muted: Color,
    pub faint: Color,
    pub a: Color,
    pub b: Color,
    pub good: Color,
    pub wait: Color,
    pub bad: Color,
    pub live: Color,
    pub on_accent: Color,
    pub wash: Color,
}

pub fn pal(mode: Mode, accent: usize) -> Pal {
    let (_, a, b) = ACCENTS[accent % ACCENTS.len()];
    let (a, b) = (Color::hex(a), Color::hex(b));
    match mode {
        Mode::Dark => Pal {
            dark: true,
            ground: Color::rgb(7, 8, 13),
            glass: Color::rgba(22, 24, 36, 216),
            glass_hi: Color::rgba(40, 36, 74, 236),
            edge: Color::rgba(255, 255, 255, 24),
            ink: Color::rgb(244, 245, 250),
            muted: Color::rgb(166, 171, 194),
            faint: Color::rgb(138, 143, 168),
            a,
            b,
            good: Color::rgb(52, 230, 160),
            wait: Color::rgb(245, 181, 74),
            bad: Color::rgb(240, 90, 96),
            live: b,
            on_accent: Color::rgb(7, 8, 13),
            wash: Color::rgba(255, 255, 255, 12),
        },
        Mode::Light => Pal {
            dark: false,
            ground: Color::rgb(244, 245, 250),
            glass: Color::rgba(255, 255, 255, 222),
            glass_hi: Color::rgba(236, 232, 255, 245),
            edge: Color::rgba(12, 14, 34, 26),
            ink: Color::rgb(11, 13, 23),
            muted: Color::rgb(74, 79, 102),
            faint: Color::rgb(96, 101, 126),
            a: a.lerp(Color::rgb(0, 0, 0), 0.18),
            b: b.lerp(Color::rgb(0, 0, 0), 0.32),
            good: Color::rgb(4, 120, 87),
            wait: Color::rgb(161, 98, 7),
            bad: Color::rgb(200, 30, 40),
            live: b.lerp(Color::rgb(0, 0, 0), 0.35),
            on_accent: Color::rgb(255, 255, 255),
            wash: Color::rgba(12, 14, 34, 10),
        },
    }
}

impl Pal {
    pub fn ramp(&self) -> Gradient {
        Gradient::horizontal().with_stops(&[(0.0, self.a), (0.55, self.b), (1.0, self.good)])
    }
    pub fn duo(&self) -> Gradient {
        Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, self.a), (1.0, self.b)])
    }
    pub fn tone(&self, t: Tone) -> Color {
        match t {
            Tone::Quiet => self.muted,
            Tone::Live => self.live,
            Tone::Wait => self.wait,
            Tone::Good => self.good,
            Tone::Bad => self.bad,
        }
    }
    pub fn kind(&self, k: PillKind) -> Color {
        match k {
            PillKind::Good => self.good,
            PillKind::Wait => self.wait,
            PillKind::Quiet => self.muted,
        }
    }
}

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// ── layout ───────────────────────────────────────────────────────

pub fn text(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s).size(size).color(color).into()
}

pub fn bold(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s).size(size).color(color).weight(FontWeight::Bold).into()
}

pub fn mono(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s)
        .style(TextStyle::new(size).family(vieww::foundation::FontFamily::Monospace).color(color))
        .into()
}

pub fn line(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s).size(size).color(color).max_lines(1).overflow(vieww::text::TextOverflow::Ellipsis).into()
}

pub fn col(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Start).spacing(gap).children(items).into()
}

pub fn stretch(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(gap).children(items).into()
}

pub fn row(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::row().cross_axis_alignment(CrossAxisAlignment::Center).spacing(gap).children(items).into()
}

pub fn row_top(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::row().cross_axis_alignment(CrossAxisAlignment::Start).spacing(gap).children(items).into()
}

pub fn grow(w: impl Into<WidgetNode>) -> WidgetNode {
    Flexible::expanded(1).child(w).into()
}

pub fn gap() -> WidgetNode {
    grow(SizedBox::shrink())
}

pub fn none() -> WidgetNode {
    SizedBox::shrink().into()
}

pub fn pad(insets: EdgeInsets, child: impl Into<WidgetNode>) -> WidgetNode {
    Container::new().padding(insets).child(child).into()
}

/// A scrolling region whose offset lives in the state under `key`; the
/// wheel and a drag move it, clamped to the content it reports.
pub fn scroller(act: &Act, st: &FlowState, key: usize, child: impl Into<WidgetNode>) -> WidgetNode {
    let offset = st.scroll[key % 16].min(st.scroll_max[key % 16]);
    let (a1, a2, a3) = (act.clone(), act.clone(), act.clone());
    GestureDetector::new()
        .on_scroll(move |e: ScrollEvent| a1.with(|s| s.scroll_by(key, e.delta.dy)))
        .child(
            Scrollable::vertical(offset)
                .on_drag(Rc::new(move |d: DragDetails| a2.with(|s| s.scroll_by(key, -d.delta.dy))))
                .on_extents(Rc::new(move |x: ScrollExtents| {
                    let max = (x.content - x.viewport).max(0.0);
                    a3.with_if(|s| s.set_extent(key, max));
                }))
                .child(child),
        )
        .into()
}

/// A centred page column at most `max` wide, scrolling under `key`.
pub fn page(act: &Act, st: &FlowState, key: usize, max: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    scroller(
        act,
        st,
        key,
        Align::new(Alignment::TOP_CENTER).child(
            Constrained::new(Constraints::new(0.0, max, 0.0, f32::INFINITY))
                .child(Container::new().padding(EdgeInsets::only(28.0, 8.0, 28.0, 28.0)).child(child)),
        ),
    )
}

pub fn glass(p: &Pal, radius: f32, padding: f32, child: impl Into<WidgetNode>) -> Container {
    Container::new()
        .radius(radius)
        .color(p.glass)
        .border(Border::new(p.edge, 1.0))
        .padding(EdgeInsets::all(padding))
        .child(child)
}

pub fn inset(p: &Pal, child: impl Into<WidgetNode>) -> WidgetNode {
    Container::new()
        .radius(14.0)
        .color(p.wash)
        .border(Border::new(p.edge, 1.0))
        .padding(EdgeInsets::all(14.0))
        .child(child)
        .into()
}

// ── controls ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Primary,
    Secondary,
    Quiet,
    Danger,
}

pub fn button(p: &Pal, label: &str, look: Look, on_tap: impl Fn() + 'static) -> WidgetNode {
    let (p, label) = (*p, label.to_string());
    Pressable::sensed(move |s: Sense| {
        let c = Container::new().padding(EdgeInsets::symmetric(16.0, 9.0)).radius(999.0);
        let (c, ink) = match look {
            Look::Primary => (c.gradient(p.ramp()).shadow(Shadow::new(p.b.with_alpha((50.0 + 90.0 * s.hover) as u8), Offset::ZERO, 20.0)), p.on_accent),
            Look::Secondary => (c.color(p.wash.lerp(p.edge, s.hover)).border(Border::new(p.edge, 1.0)), p.ink),
            Look::Quiet => (c.color(p.wash.with_alpha((255.0 * s.hover * 0.06) as u8)), p.muted.lerp(p.ink, s.hover)),
            Look::Danger => (c.color(p.bad.with_alpha((30.0 + 200.0 * s.hover) as u8)).border(Border::new(p.bad.with_alpha(120), 1.0)), p.bad.lerp(Color::WHITE, s.hover)),
        };
        Transformed::new(Transform::translate(Offset::new(0.0, -1.5 * s.hover)))
            .child(c.child(bold(label.clone(), 13.5, ink)))
            .into()
    })
    .on_tap(on_tap)
    .into()
}

/// A round icon button with a spoken label.
pub fn icon_button(p: &Pal, icon: Glyph, label: &str, on: bool, on_tap: impl Fn() + 'static) -> WidgetNode {
    let p = *p;
    let _ = label;
    Pressable::sensed(move |s: Sense| {
        let c = Container::new().width(38.0).height(38.0).radius(19.0);
        let c = if on { c.gradient(p.duo()) } else { c.color(p.wash.lerp(p.edge, s.hover)).border(Border::new(p.edge, 1.0)) };
        let ink = if on { p.on_accent } else { p.ink };
        c.child(Painting::new(PaintWith::new(move |b, sz| icon.draw(b, sz, ink)))).into()
    })
    .on_tap(on_tap)
    .into()
}

pub fn pill(p: &Pal, label: impl Into<String>, fg: Color) -> WidgetNode {
    let _ = p;
    Container::new()
        .padding(EdgeInsets::symmetric(10.0, 3.0))
        .radius(999.0)
        .color(fg.with_alpha(34))
        .child(bold(label.into(), 12.0, fg))
        .into()
}

pub fn overline(p: &Pal, s: &str) -> WidgetNode {
    bold(s.to_string(), 12.0, p.faint)
}

pub fn dot(c: Color, size: f32) -> WidgetNode {
    Container::new().width(size).height(size).radius(size / 2.0).color(c).into()
}

/// A text field in the Flow style.
pub fn field(p: &Pal, value: &str, placeholder: &str, size: f32, on_change: Rc<dyn Fn(TextEditingValue)>) -> WidgetNode {
    TextField::text(value.to_string())
        .size(size)
        .color(p.ink)
        .placeholder(placeholder.to_string())
        .placeholder_color(p.faint)
        .single_line()
        .on_changed(on_change)
        .into()
}

pub fn labelled_field(p: &Pal, label: &str, value: &str, placeholder: &str, on_change: Rc<dyn Fn(TextEditingValue)>) -> WidgetNode {
    stretch(6.0, vec![
        text(label.to_string(), 12.5, p.muted),
        Container::new()
            .radius(10.0)
            .color(p.wash)
            .border(Border::new(p.edge, 1.0))
            .padding(EdgeInsets::symmetric(12.0, 9.0))
            .child(field(p, value, placeholder, 14.0, on_change))
            .into(),
    ])
}

pub fn progress(p: &Pal, share: f32, height: f32) -> WidgetNode {
    let done = (share.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let mut parts: Vec<WidgetNode> = Vec::new();
    if done > 0 {
        parts.push(Flexible::expanded(done).child(Container::new().height(height).radius(height / 2.0).gradient(p.ramp())).into());
    }
    if done < 1000 {
        parts.push(Flexible::expanded(1000 - done).child(SizedBox::height(height)).into());
    }
    Container::new().height(height).radius(height / 2.0).color(p.edge).child(Flex::row().children(parts)).into()
}

pub fn toggle(p: &Pal, on: bool, on_tap: impl Fn() + 'static) -> WidgetNode {
    let p = *p;
    Pressable::new(move |_| {
        Animated::new(if on { 1.0 } else { 0.0 })
            .duration(ms(260))
            .curve(Curve::EXPO_OUT)
            .build(move |k| {
                let track = Container::new().width(44.0).height(24.0).radius(12.0);
                let track = if k > 0.5 { track.gradient(p.duo()) } else { track.color(p.edge) };
                Stack::new()
                    .children(children![
                        track,
                        Positioned::new().left(3.0 + 20.0 * k).top(3.0).child(Container::new().width(18.0).height(18.0).radius(9.0).color(if k > 0.5 { p.on_accent } else { p.ink })),
                    ])
                    .into()
            })
            .into()
    })
    .on_tap(on_tap)
    .into()
}

// ── motion ───────────────────────────────────────────────────────

pub fn phase(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

pub fn rise(t: f32, dy: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    Opacity::new(t)
        .child(Transformed::new(Transform::translate(Offset::new(0.0, dy * (1.0 - t)))).child(child))
        .into()
}

/// Rises in once, when it first appears (keyed, so it does not replay).
pub fn appear(key: impl Into<String>, child: WidgetNode) -> WidgetNode {
    Animated::new(1.0)
        .from(0.0)
        .duration(ms(560))
        .curve(Curve::EXPO_OUT)
        .key(key.into())
        .build(move |t| rise(t, 26.0, child.clone()))
        .into()
}

/// Children that rise one after another off one clock.
pub fn cascade(key: impl Into<String>, gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    let n = items.len().max(1) as f32;
    let items = Rc::new(items);
    Animated::new(1.0)
        .from(0.0)
        .duration(ms(1000))
        .curve(Curve::EXPO_OUT)
        .key(key.into())
        .build(move |t| {
            let kids: Vec<WidgetNode> = items
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let a = i as f32 / n * 0.45;
                    rise(phase(t, a, a + 0.55), 20.0, w.clone())
                })
                .collect();
            Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(gap).children(kids).into()
        })
        .into()
}

/// A sheet over everything, behind a dimmed, blurred-looking scrim.
pub fn overlay(p: &Pal, key: &str, max_w: f32, sheet: WidgetNode, on_scrim: impl Fn() + 'static) -> WidgetNode {
    let p = *p;
    let scrim = Pressable::new(move |_| Container::new().color(p.ground.with_alpha(if p.dark { 196 } else { 170 })).into()).on_tap(on_scrim);
    Animated::new(1.0)
        .from(0.0)
        .duration(ms(420))
        .curve(Curve::EXPO_OUT)
        .key(key.to_string())
        .build(move |t| {
            Opacity::new(t)
                .child(
                    Stack::new().fit(StackFit::Expand).children(children![
                        scrim.clone(),
                        Align::new(Alignment::CENTER).child(
                            Container::new().padding(EdgeInsets::all(24.0)).child(
                                Transformed::new(Transform::translate(Offset::new(0.0, 28.0 * (1.0 - t))))
                                    .child(Constrained::new(Constraints::new(0.0, max_w, 0.0, 720.0)).child(sheet.clone())),
                            ),
                        ),
                    ]),
                )
                .into()
        })
        .into()
}

// ── vector icons ─────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Plus,
    Mic,
    Arrow,
    Stop,
    Gear,
    Help,
    Close,
    Grid,
    Graph,
    Tick,
    Warn,
    File,
    Folder,
    Pause,
    Play,
    Copy,
    Sun,
}

impl Glyph {
    pub fn draw(self, book: &mut Sketchbook, s: Size, c: Color) {
        let (cx, cy) = (s.width / 2.0, s.height / 2.0);
        let o = |x: f32, y: f32| Offset::new(cx + x, cy + y);
        let w = 1.8;
        let poly = |book: &mut Sketchbook, pts: &[(f32, f32)], close: bool, width: f32| {
            let mut path = Path::new();
            path.move_to(o(pts[0].0, pts[0].1));
            for q in &pts[1..] {
                path.line_to(o(q.0, q.1));
            }
            if close {
                path.close();
            }
            book.stroke(path, c, width);
        };
        match self {
            Glyph::Plus => {
                book.line(o(-6.0, 0.0), o(6.0, 0.0), c, w);
                book.line(o(0.0, -6.0), o(0.0, 6.0), c, w);
            }
            Glyph::Mic => {
                book.stroke(Path::rounded_rect(Rect::new(cx - 3.5, cy - 9.0, cx + 3.5, cy + 3.0), 3.5), c, w);
                book.stroke(Path::arc(o(0.0, -1.0), 7.0, 0.0, std::f32::consts::PI), c, w);
                book.line(o(0.0, 6.0), o(0.0, 9.0), c, w);
            }
            Glyph::Arrow => poly(book, &[(-7.0, 0.0), (7.0, 0.0), (1.5, -6.0), (7.0, 0.0), (1.5, 6.0)], false, 2.3),
            Glyph::Stop => {
                book.stroke(Path::rounded_rect(Rect::new(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0), 1.5), c, w);
            }
            Glyph::Gear => {
                book.ring(o(0.0, 0.0), 7.0, w, c);
                book.circle(o(0.0, 0.0), 2.2, c);
                for k in 0..6 {
                    let a = k as f32 * std::f32::consts::PI / 3.0;
                    book.line(o(7.0 * a.cos(), 7.0 * a.sin()), o(9.5 * a.cos(), 9.5 * a.sin()), c, 2.4);
                }
            }
            Glyph::Help => {
                book.ring(o(0.0, 0.0), 8.5, w, c);
                book.stroke(Path::arc(o(0.0, -2.5), 3.0, std::f32::consts::PI, std::f32::consts::PI * 1.4), c, w);
                book.line(o(0.6, 0.4), o(0.0, 2.2), c, w);
                book.circle(o(0.0, 5.0), 1.2, c);
            }
            Glyph::Close => {
                book.line(o(-5.5, -5.5), o(5.5, 5.5), c, w);
                book.line(o(5.5, -5.5), o(-5.5, 5.5), c, w);
            }
            Glyph::Grid => {
                for (x, y) in [(-6.0, -6.0), (1.0, -6.0), (-6.0, 1.0), (1.0, 1.0)] {
                    book.stroke(Path::rounded_rect(Rect::new(cx + x, cy + y, cx + x + 5.0, cy + y + 5.0), 1.2), c, 1.5);
                }
            }
            Glyph::Graph => {
                book.circle(o(-6.0, 4.0), 2.2, c);
                book.circle(o(6.0, -5.0), 2.2, c);
                book.circle(o(6.0, 6.0), 2.2, c);
                book.line(o(-6.0, 4.0), o(6.0, -5.0), c, 1.5);
                book.line(o(-6.0, 4.0), o(6.0, 6.0), c, 1.5);
            }
            Glyph::Tick => poly(book, &[(-6.0, 0.5), (-2.0, 4.5), (6.5, -5.0)], false, 2.4),
            Glyph::Warn => {
                poly(book, &[(0.0, -8.0), (8.5, 7.0), (-8.5, 7.0)], true, w);
                book.line(o(0.0, -2.5), o(0.0, 2.0), c, w);
                book.circle(o(0.0, 4.6), 1.1, c);
            }
            Glyph::File => {
                poly(book, &[(-5.0, -8.0), (2.0, -8.0), (6.0, -4.0), (6.0, 8.0), (-5.0, 8.0)], true, 1.6);
                book.line(o(-2.0, 1.0), o(3.0, 1.0), c, 1.4);
                book.line(o(-2.0, 4.0), o(3.0, 4.0), c, 1.4);
            }
            Glyph::Folder => poly(book, &[(-8.0, -6.0), (-2.0, -6.0), (0.0, -3.5), (8.0, -3.5), (8.0, 7.0), (-8.0, 7.0)], true, 1.6),
            Glyph::Pause => {
                book.line(o(-3.0, -6.0), o(-3.0, 6.0), c, 2.6);
                book.line(o(3.0, -6.0), o(3.0, 6.0), c, 2.6);
            }
            Glyph::Play => {
                let mut path = Path::new();
                path.move_to(o(-4.0, -6.5));
                path.line_to(o(6.5, 0.0));
                path.line_to(o(-4.0, 6.5));
                path.close();
                book.fill(path, c);
            }
            Glyph::Copy => {
                book.stroke(Path::rounded_rect(Rect::new(cx - 6.0, cy - 3.0, cx + 3.0, cy + 7.0), 1.5), c, 1.5);
                poly(book, &[(-3.0, -3.5), (-3.0, -7.0), (6.0, -7.0), (6.0, 3.5), (3.5, 3.5)], false, 1.5);
            }
            Glyph::Sun => {
                book.ring(o(0.0, 0.0), 4.0, w, c);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::PI / 4.0;
                    book.line(o(6.5 * a.cos(), 6.5 * a.sin()), o(9.0 * a.cos(), 9.0 * a.sin()), c, 1.6);
                }
            }
        }
    }

    pub fn widget(self, size: f32, c: Color) -> WidgetNode {
        Painting::sized(Size::new(size, size), PaintWith::new(move |b, s| self.draw(b, s, c))).into()
    }
}

// ── the room and the flow ────────────────────────────────────────

fn glow(c: Color, a: u8) -> Gradient {
    Gradient::radial(Offset::new(0.5, 0.5), 0.5).with_stops(&[(0.0, c.with_alpha(a)), (1.0, c.with_alpha(0))])
}

/// The background: three soft lights and flowing ribbons; `k` leans the
/// light toward the work while something runs.
pub fn aurora(p: &Pal, book: &mut Sketchbook, size: Size, k: f32) {
    let (w, h) = (size.width, size.height);
    let s = if p.dark { 1.0 } else { 0.55 };
    let magenta = Color::rgb(244, 63, 142);
    book.rect(Rect::new(-0.15 * w, -0.35 * h, 0.55 * w, 0.55 * h), glow(p.a, (120.0 * s) as u8));
    book.rect(Rect::new((0.55 - 0.1 * k) * w, (-0.2 + 0.25 * k) * h, 1.2 * w, 0.75 * h), glow(p.b, ((70.0 + 50.0 * k) * s) as u8));
    book.rect(Rect::new(0.2 * w, 0.55 * h, 0.95 * w, 1.35 * h), glow(magenta, ((55.0 - 20.0 * k) * s) as u8));
    for (i, (y, amp, a)) in [(0.62_f32, 0.18_f32, 90_u8), (0.70, 0.12, 60), (0.78, 0.22, 40)].iter().enumerate() {
        let lift = (i as f32 + 1.0) * 0.03 * k;
        let mut path = Path::new();
        path.move_to(Offset::new(-0.05 * w, (y - lift) * h));
        path.cubic_to(Offset::new(0.3 * w, (y - amp - lift) * h), Offset::new(0.6 * w, (y + amp - lift) * h), Offset::new(1.05 * w, (y - 0.1 - lift) * h));
        let al = (*a as f32 * s) as u8;
        let ramp = Gradient::horizontal().with_stops(&[(0.0, p.a.with_alpha(0)), (0.35, p.a.with_alpha(al)), (0.7, p.b.with_alpha(al)), (1.0, p.b.with_alpha(0))]);
        book.stroke(path, ramp, 2.0 + i as f32);
    }
}

fn bez(p0: Offset, p1: Offset, p2: Offset, p3: Offset, t: f32) -> Offset {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Offset::new(a * p0.dx + b * p1.dx + c * p2.dx + d * p3.dx, a * p0.dy + b * p1.dy + c * p2.dy + d * p3.dy)
}

/// The plan's track: four nodes joined by curves, lit up to `prog`.
/// `hold` marks a node that is waiting on the person (amber).
pub fn track(p: &Pal, book: &mut Sketchbook, size: Size, prog: f32, started: bool, hold: Option<usize>) {
    let (w, h) = (size.width, size.height);
    let ys = [h * 0.42, h * 0.66, h * 0.32, h * 0.6];
    let nodes: Vec<Offset> = (0..4).map(|i| Offset::new(40.0 + (w - 80.0) * i as f32 / 3.0, ys[i])).collect();
    let mut pts = Vec::new();
    for s in 0..3 {
        let (a, b) = (nodes[s], nodes[s + 1]);
        let mid = (b.dx - a.dx) * 0.5;
        for j in 0..=40 {
            pts.push(bez(a, Offset::new(a.dx + mid, a.dy), Offset::new(b.dx - mid, b.dy), b, j as f32 / 40.0));
        }
    }
    let line = |upto: usize| {
        let mut path = Path::new();
        path.move_to(pts[0]);
        for q in &pts[1..upto.max(1)] {
            path.line_to(*q);
        }
        path
    };
    book.stroke(line(pts.len()), p.edge, 3.0);
    if started && prog > 0.001 {
        let lit = ((pts.len() - 1) as f32 * prog).round() as usize + 1;
        book.stroke(line(lit), p.ramp(), 4.0);
        book.circle(pts[lit - 1], 15.0, glow(if hold.is_some() { p.wait } else { p.b }, 210));
    }
    let blank = if p.dark { Color::rgb(27, 30, 44) } else { Color::rgb(226, 228, 238) };
    for (i, c) in nodes.iter().enumerate() {
        let at = i as f32 / 3.0;
        let on = if started { ((prog - at) * 10.0 + 1.0).clamp(0.0, 1.0) } else { 0.0 };
        let hue = if hold == Some(i) { p.wait } else { p.good };
        let on = if hold == Some(i) { 1.0 } else { on };
        if on > 0.0 {
            book.circle(*c, 18.0 + 6.0 * on, hue.with_alpha((60.0 * on) as u8));
        }
        book.circle(*c, 11.0, blank.lerp(hue, on));
        book.ring(*c, 11.0, 1.5, p.ink.with_alpha((50.0 + 150.0 * on) as u8));
    }
}

pub fn mark(p: &Pal, book: &mut Sketchbook, s: Size) {
    let (w, h) = (s.width, s.height);
    let mut path = Path::new();
    path.move_to(Offset::new(0.08 * w, 0.2 * h));
    path.line_to(Offset::new(0.3 * w, 0.82 * h));
    path.line_to(Offset::new(0.5 * w, 0.4 * h));
    path.line_to(Offset::new(0.7 * w, 0.82 * h));
    path.line_to(Offset::new(0.92 * w, 0.2 * h));
    book.stroke(path, p.ramp(), 2.6);
}

/// Five bars that move with the voice (driven by `t`, a running clock).
pub fn meter(p: &Pal, t: f32) -> WidgetNode {
    let bars: Vec<WidgetNode> = (0..5)
        .map(|i| {
            let k = ((t * 7.0 + i as f32 * 1.3).sin() * 0.5 + 0.5) * 0.8 + 0.2;
            Container::new().width(4.0).height(6.0 + 18.0 * k).radius(2.0).gradient(p.duo()).into()
        })
        .collect();
    row(3.0, bars)
}


// ═════════════════════════════════════════════════════════════════
// SESSION
// A session: the conversation, the Confirm Gate, the plan, the run (with
// Pause/Resume and the approval gate), and the result with its export
// menu, its build story and its receipt.



pub fn session_page(p: &Pal, act: &Act, st: &FlowState, i: usize) -> WidgetNode {
    let Some(s) = st.sessions.get(i) else {
        return none();
    };
    let mut items: Vec<WidgetNode> = vec![bubble(p, &s.request)];
    if !s.files.is_empty() {
        let chips: Vec<WidgetNode> = s.files.iter().map(|f| pill(p, f.clone(), p.muted)).collect();
        items.push(Align::new(Alignment::CENTER_RIGHT).child(row(6.0, chips)).into());
    }
    let reply = reply_for(&s.title);
    if s.stage == Stage::Drafting && s.change.is_none() {
        items.push(pending(p, &reply, s.draft));
    } else {
        items.push(reply_text(p, &reply));
    }
    if s.stage != Stage::Drafting || s.change.is_some() {
        items.push(appear(format!("confirm-{i}"), confirm_card(p, act, i, s)));
    }
    if let Some(change) = &s.change {
        items.push(bubble(p, change));
        if s.stage == Stage::Drafting {
            items.push(pending(p, "Noted. I'll change the blueprint and show you the difference before anything runs.", s.draft));
        } else {
            items.push(reply_text(p, "Noted. The blueprint now reflects the change; only the affected steps will run again."));
        }
    }
    if matches!(s.stage, Stage::Plan | Stage::Running | Stage::Approval | Stage::Done | Stage::Refused) {
        items.push(appear(format!("plan-{i}"), plan_card(p, act, i, s)));
    }
    if matches!(s.stage, Stage::Running | Stage::Approval | Stage::Done | Stage::Refused) {
        items.push(appear(format!("run-{i}-{}", s.run_no), run_card(p, act, i, s, st.stopped)));
    }
    if s.stage == Stage::Approval {
        items.push(appear(format!("gate-{i}-{}", s.run_no), approval(p, act, i)));
    }
    if s.stage == Stage::Refused {
        items.push(appear(format!("refused-{i}-{}", s.run_no), refused(p, act, i)));
    }
    if s.stage == Stage::Done {
        items.push(appear(format!("done-{i}-{}", s.run_no), result(p, act, i, s)));
    }
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(children![
            grow(page(act, st, 5 + i, 940.0, stretch(16.0, items))),
            Align::new(Alignment::TOP_CENTER).child(
                Constrained::new(Constraints::new(0.0, 940.0, 0.0, f32::INFINITY))
                    .child(pad(EdgeInsets::only(28.0, 0.0, 28.0, 14.0), change_composer(p, act, i, s))),
            ),
        ])
        .into()
}

fn bubble(p: &Pal, text_: &str) -> WidgetNode {
    Align::new(Alignment::CENTER_RIGHT)
        .child(
            Constrained::new(Constraints::new(0.0, 640.0, 0.0, f32::INFINITY)).child(
                Container::new()
                    .padding(EdgeInsets::symmetric(18.0, 12.0))
                    .radius(20.0)
                    .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, p.a.with_alpha(if p.dark { 150 } else { 60 })), (1.0, p.b.with_alpha(if p.dark { 90 } else { 40 }))]))
                    .child(text(text_.to_string(), 15.0, p.ink)),
            ),
        )
        .into()
}

fn reply_text(p: &Pal, t: &str) -> WidgetNode {
    Constrained::new(Constraints::new(0.0, 760.0, 0.0, f32::INFINITY)).child(text(t.to_string(), 15.5, p.ink)).into()
}

/// Wright's reply on its way: the words so far, and what it is doing.
fn pending(p: &Pal, full: &str, draft: f32) -> WidgetNode {
    let n = (full.chars().count() as f32 * draft).round() as usize;
    let shown: String = full.chars().take(n).collect();
    stretch(8.0, vec![
        reply_text(p, &format!("{shown}▍")),
        row(8.0, vec![
            Container::new().width(60.0).child(progress(p, draft, 3.0)).into(),
            text(format!("{}…  {:.0}s", drafting_line(draft), draft * 2.8), 13.0, p.muted),
        ]),
    ])
}

fn confirm_card(p: &Pal, act: &Act, i: usize, s: &Sess) -> WidgetNode {
    let lines: Vec<WidgetNode> = confirm_lines(&s.title)
        .into_iter()
        .map(|(l, care)| {
            row(10.0, vec![
                dot(if care { p.wait } else { p.good }, 7.0),
                grow(text(l.to_string(), 14.0, p.ink)),
                if care { pill(p, "needs care", p.wait) } else { none() },
            ])
        })
        .collect();
    let mut body = vec![
        row(10.0, vec![
            grow(col(3.0, vec![bold("This is exactly what will exist", 18.0, p.ink), text("Written from the blueprint by Wright, not by a model.", 12.5, p.muted)])),
            button(p, "Copy", Look::Quiet, act.on(|s| s.say("Copied the summary — 8 lines"))),
        ]),
        stretch(8.0, lines),
    ];
    if s.stage == Stage::Confirm {
        body.push(row(10.0, vec![
            button(p, "Build this", Look::Primary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.stage = Stage::Plan; } s.follow(i); })),
            button(p, "Change something", Look::Secondary, act.on(|s| s.say("Type what to change in the box below, then send."))),
        ]));
    }
    glass(p, 22.0, 20.0, stretch(14.0, body)).into()
}

fn plan_card(p: &Pal, act: &Act, i: usize, s: &Sess) -> WidgetNode {
    let steps: Vec<WidgetNode> = STEPS
        .iter()
        .enumerate()
        .map(|(k, (name, agent, what, tag, kind))| {
            row(12.0, vec![
                Container::new().width(26.0).height(26.0).radius(13.0).color(p.a.with_alpha(40)).alignment(Alignment::CENTER).child(bold(format!("{}", k + 1), 12.5, p.ink)).into(),
                grow(col(2.0, vec![bold(format!("{name} · {agent}"), 14.0, p.ink), text(what.to_string(), 12.5, p.muted)])),
                pill(p, *tag, p.kind(*kind)),
            ])
        })
        .collect();
    let mut body = vec![
        row(10.0, vec![
            grow(col(3.0, vec![
                row(8.0, vec![bold("The plan", 20.0, p.ink), pill(p, "blueprint a3f9c21", p.muted)]),
                text("4 steps · never more than $0.50 a run, $2.00 a day", 13.0, p.muted),
            ])),
            button(p, "See the agents", Look::Quiet, act.on(|s| s.screen = Screen::Engine)),
            button(p, "Agents & tools", Look::Quiet, act.on(|s| { s.settings = true; s.tab = 1; })),
        ]),
        stretch(10.0, steps),
    ];
    if s.stage == Stage::Plan {
        body.push(inset(p, row(10.0, vec![Glyph::Warn.widget(18.0, p.wait), grow(text("Worth a look before you approve: the Mailer sends one email, and it asks you first.", 13.0, p.ink))])));
        body.push(row(10.0, vec![
            button(p, "Approve & run", Look::Primary, act.on(move |s| {
                if let Some(x) = s.sessions.get_mut(i) {
                    x.stage = Stage::Running;
                    x.p = 0.0;
                    x.paused = false;
                    x.run_no += 1;
                }
                s.follow(i);
            })),
            button(p, "Change something", Look::Secondary, act.on(|s| s.say("Type what to change in the box below, then send."))),
        ]));
    }
    glass(p, 22.0, 20.0, stretch(14.0, body)).into()
}

fn run_card(p: &Pal, act: &Act, i: usize, s: &Sess, stopped: bool) -> WidgetNode {
    let at_gate = s.stage == Stage::Approval;
    let done = s.stage == Stage::Done;
    let (status, tone, _) = s.status();
    let label = if stopped && s.stage == Stage::Running { "Stopped".to_string() } else { format!("{status} · {:.0}%", s.p * 100.0) };
    let spent = 0.0041 * s.p;
    let mut head: Vec<WidgetNode> = vec![
        grow(col(3.0, vec![
            bold(format!("Run {}", s.run_no), 20.0, p.ink),
            text(format!("${spent:.4} so far · {}", if done { "6 ran, 0 cached" } else { "firing in the sandbox" }), 13.0, p.muted),
        ])),
        row(8.0, vec![dot(p.tone(tone), 8.0), bold(label, 13.5, p.tone(tone))]),
    ];
    if s.stage == Stage::Running {
        head.push(if s.paused {
            button(p, "Resume", Look::Primary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.paused = false; } s.say("Resumed."); }))
        } else {
            button(p, "Pause", Look::Secondary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.paused = true; } s.say("Paused after the step in progress."); }))
        });
    }
    head.push(button(p, "Watch the agents", Look::Quiet, act.on(|s| s.screen = Screen::Engine)));
    let pp = *p;
    let prog = s.p;
    let hold = if at_gate { Some(3) } else { None };
    glass(p, 22.0, 20.0, stretch(12.0, vec![
        row(12.0, head),
        SizedBox::height(104.0).child(Painting::new(PaintWith::new(move |b, sz| track(&pp, b, sz, prog, true, hold)))).into(),
        step_labels(p, s),
    ]))
    .into()
}

fn step_labels(p: &Pal, s: &Sess) -> WidgetNode {
    let labels: Vec<WidgetNode> = STEPS
        .iter()
        .enumerate()
        .map(|(k, (name, agent, ..))| {
            let at = k as f32 / 3.0;
            let prev = if k == 0 { -1.0 } else { (k - 1) as f32 / 3.0 };
            let (state, c) = if k == 3 && s.stage == Stage::Approval {
                ("asks first", p.wait)
            } else if k == 3 && s.stage == Stage::Refused {
                ("skipped", p.bad)
            } else if s.p >= at - 0.01 && (k < 3 || s.stage == Stage::Done) {
                ("done", p.good)
            } else if s.p > prev {
                (if s.paused { "paused" } else { "working" }, if s.paused { p.wait } else { p.live })
            } else {
                ("waiting", p.faint)
            };
            grow(
                Flex::column()
                    .main_axis_size(MainAxisSize::Min)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(2.0)
                    .children(children![bold(name.to_string(), 14.0, p.ink), bold(state, 12.0, c), text(agent.to_string(), 12.0, p.faint)]),
            )
        })
        .collect();
    let _ = GATE;
    Flex::row().children(labels).into()
}

fn approval(p: &Pal, act: &Act, i: usize) -> WidgetNode {
    Container::new()
        .radius(20.0)
        .gradient(Gradient::horizontal().with_stops(&[(0.0, p.wait.with_alpha(46)), (1.0, p.glass)]))
        .border(Border::new(p.wait.with_alpha(130), 1.0))
        .padding(EdgeInsets::all(18.0))
        .child(row(14.0, vec![
            Glyph::Warn.widget(26.0, p.wait),
            grow(col(4.0, vec![
                bold("Mailer is asking before it touches the world", 15.0, p.ink),
                text("One email to you@example.com with the digest (412 words). Checks passed: Testwright ✓  Reviewer ✓", 13.0, p.muted),
                text("Drag the card right to allow, left to refuse — or use the buttons.", 12.0, p.faint),
            ])),
            button(p, "Don't allow", Look::Danger, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.stage = Stage::Refused; } s.follow(i); s.say("Refused. Nothing was sent."); })),
            button(p, "Allow", Look::Primary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.stage = Stage::Running; x.p = GATE + 0.001; } s.follow(i); })),
        ]))
        .into()
}

fn refused(p: &Pal, act: &Act, i: usize) -> WidgetNode {
    glass(p, 22.0, 20.0, row(12.0, vec![
        Glyph::Close.widget(22.0, p.bad),
        grow(col(3.0, vec![bold("Not sent", 18.0, p.ink), text("You refused the Mailer. Nothing left this machine; the digest is in the project folder.", 13.0, p.muted)])),
        button(p, "Run again", Look::Primary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.stage = Stage::Running; x.p = 0.0; x.run_no += 1; } })),
        button(p, "Open the folder", Look::Secondary, act.on(|s| s.say("Opened ~/wright-projects/hn-daily-digest"))),
    ]))
    .border(Border::new(p.bad.with_alpha(110), 1.0))
    .into()
}

fn result(p: &Pal, act: &Act, i: usize, s: &Sess) -> WidgetNode {
    let mut body = vec![
        row(12.0, vec![
            Container::new().width(38.0).height(38.0).radius(19.0).color(p.good.with_alpha(40)).shadow(Shadow::new(p.good.with_alpha(90), Offset::ZERO, 18.0)).child(Glyph::Tick.widget(38.0, p.good)).into(),
            grow(col(2.0, vec![
                bold("Delivered and verified", 20.0, p.ink),
                text(format!("run {} · done: 6 ran, 0 cached (saved $0.0000), cost $0.0041", s.run_no), 13.0, p.muted),
            ])),
        ]),
        row(8.0, vec![pill(p, "6 tests passed", p.good), pill(p, "Review: approve", p.good), pill(p, "Receipt saved", p.muted)]),
        row(10.0, vec![
            button(p, "Run again", Look::Secondary, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.stage = Stage::Running; x.p = 0.0; x.run_no += 1; x.export_open = false; x.built_open = false; } })),
            button(p, "Export", if s.export_open { Look::Primary } else { Look::Secondary }, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.export_open = !x.export_open; } s.follow(i); })),
            button(p, "Open the folder", Look::Secondary, act.on(|s| s.say("Opened ~/wright-projects/hn-daily-digest"))),
            gap(),
            button(p, "See the receipt", Look::Primary, act.on(|s| s.receipt = true)),
            button(p, if s.built_open { "Hide how it was built" } else { "How it was built" }, Look::Quiet, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.built_open = !x.built_open; } s.follow(i); })),
        ]),
    ];
    if let Some(k) = s.exported {
        body.push(inset(p, row(10.0, vec![
            Glyph::Tick.widget(18.0, p.good),
            grow(text(format!("{} exported to ~/wright-projects/hn-daily-digest/dist · signed into receipt.json", EXPORTS[k].0), 13.0, p.ink)),
            button(p, "Show in folder", Look::Quiet, act.on(|s| s.say("Opened the dist folder"))),
        ])));
    }
    if s.export_open {
        let items: Vec<WidgetNode> = EXPORTS
            .iter()
            .enumerate()
            .map(|(k, (label, what, needs_pass))| {
                let pp = *p;
                let allowed = !needs_pass || s.stage == Stage::Done;
                let (label, what) = (label.to_string(), what.to_string());
                Pressable::sensed(move |h: Sense| {
                    Container::new()
                        .radius(12.0)
                        .color(pp.wash.lerp(pp.edge, h.hover))
                        .padding(EdgeInsets::symmetric(14.0, 10.0))
                        .child(row(12.0, vec![
                            Glyph::File.widget(18.0, if allowed { pp.ink } else { pp.faint }),
                            grow(col(2.0, vec![bold(label.clone(), 14.0, if allowed { pp.ink } else { pp.faint }), text(what.clone(), 12.5, pp.muted)])),
                        ]))
                        .into()
                })
                .on_tap(act.on(move |s| {
                    if let Some(x) = s.sessions.get_mut(i) {
                        x.exported = Some(k);
                        x.export_open = false;
                    }
                }))
                .into()
            })
            .collect();
        body.push(appear("export-menu", stretch(6.0, items)));
    }
    if s.built_open {
        let rows: Vec<WidgetNode> = BUILT
            .iter()
            .map(|(t, who, what)| row(12.0, vec![SizedBox::width(48.0).child(mono(t.to_string(), 12.0, p.faint)).into(), SizedBox::width(96.0).child(bold(who.to_string(), 12.5, p.live)).into(), grow(text(what.to_string(), 13.0, p.ink))]))
            .collect();
        body.push(appear("built", inset(p, stretch(6.0, rows))));
    }
    glass(p, 22.0, 20.0, stretch(14.0, body)).border(Border::new(p.good.with_alpha(100), 1.0)).into()
}

fn change_composer(p: &Pal, act: &Act, i: usize, s: &Sess) -> WidgetNode {
    let pp = *p;
    glass(p, 22.0, 8.0, row(10.0, vec![
        icon_button(p, Glyph::Plus, "Add files", false, act.on(move |s| {
            if let Some(x) = s.sessions.get_mut(i) {
                if !x.files.iter().any(|f| f == "brand-notes.md") {
                    x.files.push("brand-notes.md".into());
                }
            }
        })),
        grow(
            TextField::text(s.composer.clone())
                .size(15.0)
                .color(p.ink)
                .placeholder("Ask for a change, e.g. make it weekly")
                .placeholder_color(p.faint)
                .single_line()
                .on_changed(act.text(move |s, t| { if let Some(x) = s.sessions.get_mut(i) { x.composer = t; } }))
                .on_submit(act.submit(move |s, _| send_change(s, i))),
        ),
        icon_button(p, Glyph::Mic, "Speak", false, act.on(move |s| { if let Some(x) = s.sessions.get_mut(i) { x.composer = "Make it weekly, on Mondays at 9".into(); } })),
        Pressable::sensed(move |h: Sense| {
            Container::new().width(42.0).height(42.0).radius(21.0).gradient(pp.duo()).shadow(Shadow::new(pp.b.with_alpha((60.0 + 90.0 * h.hover) as u8), Offset::ZERO, 16.0)).child(Glyph::Arrow.widget(42.0, pp.on_accent)).into()
        })
        .on_tap(act.on(move |s| send_change(s, i)))
        .into(),
    ]))
    .into()
}

fn send_change(s: &mut FlowState, i: usize) {
    if let Some(x) = s.sessions.get_mut(i) {
        let t = x.composer.trim().to_string();
        if t.is_empty() {
            s.say("Type what to change first.");
            return;
        }
        x.change = Some(t);
        x.composer.clear();
        x.stage = Stage::Drafting;
        x.draft = 0.0;
        s.follow(i);
    }
}


// ═════════════════════════════════════════════════════════════════
// OVERVIEW
// All sessions: every page as a card, × closes one into Saved (with
// Undo), and the Saved shelf reopens one.




pub fn overview_page(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let mut cards: Vec<WidgetNode> = vec![card(p, "Home", pill(p, "Home", p.muted), "What should exist?", 0.0, None, act.on(|s| s.screen = Screen::Home), None)];
    for (i, s) in st.sessions.iter().enumerate() {
        let (status, tone, share) = s.status();
        cards.push(card(
            p,
            &s.title,
            pill(p, status, p.tone(tone)),
            &s.request,
            share,
            Some(s.stage),
            act.on(move |s| s.open(i)),
            Some(Box::new(act.on(move |s| s.close_session(i)))),
        ));
    }
    cards.push(card(p, "New session", pill(p, "+ New", p.live), "Describe what should exist", 0.0, None, act.on(|s| s.screen = Screen::New), None));

    let n = cards.len();
    let per = 3usize;
    let cards = Rc::new(cards);
    let grid = Animated::new(1.0)
        .from(0.0)
        .duration(ms(900))
        .curve(Curve::EXPO_OUT)
        .key(format!("overview-{n}"))
        .build(move |t| {
            let mut rows: Vec<WidgetNode> = Vec::new();
            for r in 0..n.div_ceil(per) {
                let mut kids: Vec<WidgetNode> = Vec::new();
                for c in 0..per {
                    let i = r * per + c;
                    if let Some(w) = cards.get(i) {
                        let a = i as f32 / n as f32 * 0.4;
                        let k = phase(t, a, a + 0.6);
                        kids.push(grow(Opacity::new(k).child(Transformed::new(Transform::scale_around(Offset::new(170.0, 120.0), 0.9 + 0.1 * k, 0.9 + 0.1 * k)).child(w.clone()))));
                    } else {
                        kids.push(gap());
                    }
                }
                rows.push(Flex::row().spacing(20.0).cross_axis_alignment(CrossAxisAlignment::Start).children(kids).into());
            }
            stretch(24.0, rows)
        });
    let mut items = vec![
        Center::new().child(text("Click a session to open it · × closes it into Saved · Esc goes back", 13.0, p.muted)).into(),
        SizedBox::height(20.0).into(),
        grid.into(),
    ];
    if !st.saved.is_empty() {
        items.push(SizedBox::height(16.0).into());
        items.push(Center::new().child(row(12.0, vec![overline(p, "SAVED"), saved_shelf(p, act, &st.saved)])).into());
    }
    page(act, st, 2, 1160.0, stretch(0.0, items))
}

#[allow(clippy::too_many_arguments)]
fn card(
    p: &Pal,
    title: &str,
    badge: WidgetNode,
    preview: &str,
    share: f32,
    stage: Option<Stage>,
    open: impl Fn() + 'static,
    close: Option<Box<dyn Fn()>>,
) -> WidgetNode {
    let (pp, title, preview) = (*p, title.to_string(), preview.to_string());
    let body = Pressable::sensed(move |h: Sense| {
        let pp2 = pp;
        let mini: WidgetNode = match stage {
            Some(_) => SizedBox::height(46.0).child(Painting::new(PaintWith::new(move |b, s| track(&pp2, b, s, share, true, None)))).into(),
            None => SizedBox::height(46.0).into(),
        };
        Transformed::new(Transform::translate(Offset::new(0.0, -5.0 * h.hover)))
            .child(
                Container::new()
                    .height(220.0)
                    .radius(20.0)
                    .color(pp.glass)
                    .border(Border::new(pp.edge.lerp(pp.a.with_alpha(200), h.hover), 1.0))
                    .shadow(Shadow::new(pp.a.with_alpha((60.0 * h.hover) as u8), Offset::new(0.0, 18.0), 36.0))
                    .padding(EdgeInsets::all(18.0))
                    .child(Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(10.0).children(vec![
                        Align::new(Alignment::CENTER_LEFT).child(badge.clone()).into(),
                        Text::new(preview.clone()).size(13.5).color(pp.muted).max_lines(3).into(),
                        gap(),
                        mini,
                        progress(&pp, share, 4.0),
                    ])),
            )
            .into()
    })
    .on_tap(open);
    let mut caption = vec![grow(line(title, 14.0, p.ink))];
    if let Some(f) = close {
        let pp = *p;
        caption.push(
            Pressable::sensed(move |h: Sense| {
                Container::new().width(28.0).height(28.0).radius(14.0).color(pp.wash.lerp(pp.edge, h.hover)).child(Glyph::Close.widget(28.0, pp.muted.lerp(pp.ink, h.hover))).into()
            })
            .on_tap(move || f())
            .into(),
        );
    }
    stretch(10.0, vec![body.into(), row(8.0, caption)])
}


// ═════════════════════════════════════════════════════════════════
// SETTINGS
// Settings: all nine tabs of the real window, working on mock data.



pub fn settings_sheet(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let pp = *p;
    let tabs: Vec<WidgetNode> = TABS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let on = st.tab == i;
            let name = name.to_string();
            Pressable::sensed(move |h: Sense| {
                let name = name.clone();
                Animated::new(if on { 1.0 } else { 0.0 })
                    .duration(ms(280))
                    .curve(Curve::EXPO_OUT)
                    .build(move |k| {
                        Container::new()
                            .padding(EdgeInsets::symmetric(14.0, 9.0))
                            .radius(10.0)
                            .color(pp.wash.with_alpha((255.0 * h.hover * 0.5) as u8).lerp(pp.a.with_alpha(if pp.dark { 70 } else { 36 }), k))
                            .child(text(name.clone(), 14.0, pp.muted.lerp(pp.ink, k.max(h.hover))))
                            .into()
                    })
                    .into()
            })
            .on_tap(act.on(move |s| s.tab = i))
            .into()
        })
        .collect();
    let content: WidgetNode = match st.tab {
        0 => models(p, act, st),
        1 => agents(p),
        2 => defaults(p, act, st),
        3 => appearance(p, act, st),
        4 => network(p, act, st),
        5 => fleet(p, act, st),
        6 => organization(p, act, st),
        7 => workspace(p, act, st),
        _ => this_project(p, act, st),
    };
    let content = appear(format!("tab-{}", st.tab), content);
    glass(p, 26.0, 22.0, stretch(16.0, vec![
        row(10.0, vec![
            Glyph::Gear.widget(22.0, p.live),
            grow(bold("Settings", 22.0, p.ink)),
            text("Esc closes", 12.5, p.faint),
            icon_button(p, Glyph::Close, "Close settings", false, act.on(|s| s.settings = false)),
        ]),
        SizedBox::height(560.0)
            .child(Flex::row().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(24.0).children(children![
                SizedBox::width(200.0).child(stretch(2.0, tabs)),
                grow(scroller(act, st, 4, pad(EdgeInsets::only(0.0, 0.0, 12.0, 0.0), content))),
            ]))
            .into(),
    ]))
    .into()
}

fn section(p: &Pal, title: &str, sub: &str) -> WidgetNode {
    col(4.0, vec![bold(title.to_string(), 17.0, p.ink), text(sub.to_string(), 13.5, p.muted)])
}

fn segmented(p: &Pal, options: &[&str], at: usize, act: &Act, set: fn(&mut FlowState, usize)) -> WidgetNode {
    let items: Vec<WidgetNode> = options
        .iter()
        .enumerate()
        .map(|(i, o)| button(p, o, if i == at { Look::Primary } else { Look::Secondary }, act.on(move |s| set(s, i))))
        .collect();
    row(8.0, items)
}

fn notice(p: &Pal, c: Color, msg: &str) -> WidgetNode {
    Container::new()
        .radius(10.0)
        .color(c.with_alpha(26))
        .border(Border::new(c.with_alpha(100), 1.0))
        .padding(EdgeInsets::symmetric(12.0, 9.0))
        .child(row_top(10.0, vec![pad(EdgeInsets::only(0.0, 5.0, 0.0, 0.0), dot(c, 6.0)), grow(text(msg.to_string(), 13.0, p.ink))]))
        .into()
}

// ── Models & keys ────────────────────────────────────────────────

fn models(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let qwen_state: WidgetNode = if let Some(d) = st.download {
        stretch(6.0, vec![text(format!("Downloading the built-in model qwen3-1.7b (first use, once): {:.0}% of 1.83 GB", d * 100.0), 12.5, p.muted), progress(p, d, 5.0)])
    } else {
        none()
    };
    let qwen_action: WidgetNode = if st.downloaded {
        pill(p, "Ready", p.good)
    } else if st.download.is_some() {
        button(p, "Pause", Look::Secondary, act.on(|s| { s.download = None; s.say("Download paused — Resume download picks up where it stopped."); }))
    } else {
        button(p, "Download now", Look::Secondary, act.on(|s| s.download = Some(0.0)))
    };
    let mut v = vec![
        section(p, "Built-in models", "Two small open models Wright runs on this machine: no key, no account, nothing leaves it."),
        inset(p, stretch(8.0, vec![
            row(10.0, vec![
                grow(col(4.0, vec![
                    row(8.0, vec![bold("qwen3-1.7b", 15.0, p.ink), pill(p, if st.downloaded { "On this machine" } else { "Downloads on first use" }, p.live)]),
                    text("Qwen3 1.7B (8-bit): writes, plans and calls tools · 1.83 GB · Apache-2.0 · sha256 061b54daade0…", 12.5, p.muted),
                ])),
                qwen_action,
            ]),
            qwen_state,
        ])),
        inset(p, row(10.0, vec![
            grow(col(4.0, vec![
                row(8.0, vec![bold("whisper-base", 15.0, p.ink), pill(p, "Downloads on first use", p.live)]),
                text("Whisper base: speech to text in 99 languages, on this machine · 148 MB · MIT", 12.5, p.muted),
            ])),
            button(p, "Download now", Look::Secondary, act.on(|s| s.say("whisper-base will download with the next voice message."))),
        ])),
        row(10.0, vec![text("When a model has no key:", 13.5, p.ink), segmented(p, &["Use the built-in model", "Stop and ask for a key"], st.no_key, act, |s, i| s.no_key = i)]),
        SizedBox::height(4.0).into(),
        row(10.0, vec![
            grow(section(p, "Model providers", "Agents use the first provider whose models match. Keys live in your system's keychain; Wright never writes them to a file.")),
            button(p, "Add provider", Look::Secondary, act.on(|s| s.adding_provider = true)),
        ]),
    ];
    if st.adding_provider {
        v.push(inset(p, stretch(10.0, vec![
            bold("Add a provider", 15.0, p.ink),
            row(10.0, vec![
                grow(labelled_field(p, "Name", &st.new_provider.0, "together", act.text(|s, t| s.new_provider.0 = t))),
                grow(labelled_field(p, "Address", &st.new_provider.1, "https://api.together.xyz/v1", act.text(|s, t| s.new_provider.1 = t))),
            ]),
            row(10.0, vec![
                button(p, "Add provider", Look::Primary, act.on(|s| {
                    let (name, addr) = (s.new_provider.0.trim().to_string(), s.new_provider.1.trim().to_string());
                    if name.is_empty() || addr.is_empty() {
                        s.say("A provider needs a name and an address.");
                        return;
                    }
                    s.providers.push(Provider { name, address: addr, models: "*".into(), key: Key::Missing, yours: true });
                    s.new_provider = (String::new(), String::new());
                    s.adding_provider = false;
                })),
                button(p, "Cancel", Look::Quiet, act.on(|s| s.adding_provider = false)),
            ]),
        ])));
    }
    for (i, pr) in st.providers.iter().enumerate() {
        let mut block = vec![
            row(10.0, vec![
                bold(pr.name.clone(), 15.0, p.ink),
                if pr.key == Key::Ready { pill(p, "Key ready", p.good) } else { pill(p, "Key missing", p.wait) },
                if pr.yours { pill(p, "yours", p.live) } else { none() },
                gap(),
                button(p, if pr.key == Key::Ready { "Replace key" } else { "Set key" }, Look::Secondary, act.on(move |s| { s.editing_key = Some(i); s.key_text.clear(); })),
                if pr.yours { button(p, "Remove", Look::Quiet, act.on(move |s| { s.providers.remove(i); })) } else { none() },
            ]),
            mono(format!("{}  ·  {}", pr.address, pr.models), 12.0, p.muted),
        ];
        if pr.key == Key::Missing {
            block.push(notice(p, p.wait, &format!("keychain:wright/{} cannot be read: the keychain tool (secret-tool) is not installed — install it, or set WRIGHT_SECRET_WRIGHT_{}", pr.name, pr.name.to_uppercase())));
        }
        if st.editing_key == Some(i) {
            block.push(row(10.0, vec![
                grow(
                    Container::new().radius(10.0).color(p.wash).border(Border::new(p.a.with_alpha(160), 1.0)).padding(EdgeInsets::symmetric(12.0, 9.0))
                        .child(TextField::text(st.key_text.clone()).size(14.0).color(p.ink).placeholder("Paste the key — sk-…").placeholder_color(p.faint).single_line().on_changed(act.text(|s, t| s.key_text = t))),
                ),
                button(p, "Save to keychain", Look::Primary, act.on(move |s| {
                    if let Some(x) = s.providers.get_mut(i) {
                        x.key = Key::Ready;
                        let msg = format!("Saved keychain:wright/{} to the keychain.", x.name);
                        s.editing_key = None;
                        s.key_text.clear();
                        s.say(msg);
                    }
                })),
                button(p, "Cancel", Look::Quiet, act.on(|s| s.editing_key = None)),
            ]));
        }
        v.push(inset(p, stretch(8.0, block)));
    }
    stretch(14.0, v)
}

// ── Agents & tools ───────────────────────────────────────────────

fn agents(p: &Pal) -> WidgetNode {
    let mut v = vec![section(p, "Wright's own agents", "Built in, always there. You never write these: Wright brings each one in when a plan needs it, and every one follows a fixed contract. Your project's agents are the ones your plan names; they appear on the plan card and in the Engine view (hold Space).")];
    for (name, what, joins) in WORKFORCE {
        v.push(inset(p, row(10.0, vec![grow(col(3.0, vec![bold(name, 15.0, p.ink), text(what, 13.0, p.muted)])), pill(p, joins, p.muted)])));
    }
    v.push(SizedBox::height(6.0).into());
    v.push(section(p, "Tools", "Every tool a plan can grant. An agent gets only the tools its plan names; the gate checks each call."));
    for (name, what, effect) in TOOLS {
        let c = if effect.contains("asks") || effect.contains("internet") { p.wait } else if effect.contains("project") { p.good } else { p.muted };
        v.push(row(10.0, vec![SizedBox::width(120.0).child(mono(name, 13.0, p.ink)).into(), grow(text(what, 13.0, p.muted)), pill(p, effect, c)]));
    }
    stretch(10.0, v)
}

// ── Defaults ─────────────────────────────────────────────────────

fn defaults(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    stretch(14.0, vec![
        section(p, "New projects", "What a new blueprint starts with. A project can change any of these in This project."),
        row(12.0, vec![
            grow(labelled_field(p, "The per-run limit ($)", &st.per_run, "0.50", act.text(|s, t| s.per_run = t))),
            grow(labelled_field(p, "The daily limit ($)", &st.per_day, "2.00", act.text(|s, t| s.per_day = t))),
        ]),
        row(12.0, vec![
            grow(labelled_field(p, "Interview model", "qwen3-1.7b (built in)", "", act.text(|_, _| {}))),
            grow(labelled_field(p, "Voice model", "whisper-base (built in)", "", act.text(|_, _| {}))),
        ]),
        row(10.0, vec![
            button(p, "Save defaults", Look::Primary, act.on(|s| { let m = format!("Saved: never more than ${} a run, ${} a day for new projects.", s.per_run, s.per_day); s.say(m); })),
        ]),
    ])
}

// ── Appearance ───────────────────────────────────────────────────

fn appearance(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let swatches: Vec<WidgetNode> = ACCENTS
        .iter()
        .enumerate()
        .map(|(i, (name, a, b))| {
            let on = st.accent == i;
            let (pp, a, b) = (*p, Color::hex(*a), Color::hex(*b));
            Pressable::new(move |_| {
                col(6.0, vec![
                    Container::new()
                        .width(72.0)
                        .height(72.0)
                        .radius(20.0)
                        .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, a), (1.0, b)]))
                        .border(Border::new(if on { pp.ink } else { Color::TRANSPARENT }, 3.0))
                        .into(),
                    text(*name, 12.5, if on { pp.ink } else { pp.muted }),
                ])
            })
            .on_tap(act.on(move |s| s.accent = i))
            .into()
        })
        .collect();
    stretch(16.0, vec![
        section(p, "Theme", "The window follows the theme you pick here; the status bar flips it too."),
        segmented(p, &["Dark", "Light"], if st.mode == Mode::Dark { 0 } else { 1 }, act, |s, i| s.mode = if i == 0 { Mode::Dark } else { Mode::Light }),
        section(p, "Accent colour", "The accent drives the flow, the buttons and the light in the room. Any colour reads in both themes."),
        row(14.0, swatches),
    ])
}

// ── Network ──────────────────────────────────────────────────────

fn network(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let mut env_rows: Vec<WidgetNode> = st
        .env
        .iter()
        .enumerate()
        .map(|(i, (k, v))| row(10.0, vec![SizedBox::width(150.0).child(mono(k.clone(), 13.0, p.ink)).into(), grow(mono(v.clone(), 13.0, p.muted)), button(p, "Remove", Look::Quiet, act.on(move |s| { s.env.remove(i); }))]))
        .collect();
    if env_rows.is_empty() {
        env_rows.push(text("No variables yet.", 13.0, p.faint));
    }
    stretch(14.0, vec![
        section(p, "Certificates", "If your organization inspects HTTPS (common on office laptops), add its root certificate so Wright can reach model providers and downloads. It is trusted in addition to this computer's own certificates."),
        row(10.0, vec![
            grow(labelled_field(p, "Certificate file (paste a path, or choose one)", st.cert.as_deref().unwrap_or(""), "/path/to/company-root.pem", act.text(|s, t| s.cert = Some(t)))),
            pad(EdgeInsets::only(0.0, 20.0, 0.0, 0.0), button(p, "Choose file…", Look::Secondary, act.on(|s| s.cert = Some("/etc/ssl/company-root.pem".into())))),
        ]),
        text(if st.cert.is_some() { "Chosen. Save to start using it." } else { "No extra certificates. This computer's own trust store is used." }, 12.5, p.muted),
        row(10.0, vec![
            button(p, "Save certificates", Look::Primary, act.on(|s| { if s.cert.is_some() { s.say("Saved. New connections trust the certificate."); } else { s.say("Choose a certificate file first."); } })),
            button(p, "Remove", Look::Quiet, act.on(|s| s.cert = None)),
        ]),
        SizedBox::height(4.0).into(),
        section(p, "Environment variables", "For Wright and every program it runs: a proxy, a package mirror, a model mirror. Values whose names say they are secret are hidden."),
        row(10.0, vec![
            grow(labelled_field(p, "Name", &st.env_new.0, "NO_PROXY", act.text(|s, t| s.env_new.0 = t))),
            grow(labelled_field(p, "Value", &st.env_new.1, "localhost,127.0.0.1", act.text(|s, t| s.env_new.1 = t))),
            pad(EdgeInsets::only(0.0, 20.0, 0.0, 0.0), button(p, "Add variable", Look::Primary, act.on(|s| {
                let k = s.env_new.0.trim().to_string();
                if k.is_empty() {
                    s.say("A variable needs a name.");
                    return;
                }
                let v = s.env_new.1.trim().to_string();
                s.env.push((k, v));
                s.env_new = (String::new(), String::new());
            }))),
        ]),
        section(p, "In effect now", "What Wright sees in this session. Passwords inside proxy addresses are hidden."),
        inset(p, stretch(8.0, env_rows)),
    ])
}

// ── Fleet ────────────────────────────────────────────────────────

fn fleet(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    stretch(14.0, vec![
        section(p, "Background schedules", "Everything that runs on its own, on this computer."),
        inset(p, stretch(10.0, vec![
            row(10.0, vec![grow(col(2.0, vec![bold("Weekly Sales Report", 14.0, p.ink), text("Every Monday at 09:00 · last run: delivered and verified", 12.5, p.muted)])), pill(p, if st.schedules_on { "Runs on its schedule." } else { "Waiting for the background runner" }, if st.schedules_on { p.good } else { p.wait })]),
            row(10.0, vec![grow(col(2.0, vec![bold("HN Daily Digest", 14.0, p.ink), text("Every day at 08:00 · Never run yet.", 12.5, p.muted)])), pill(p, "Not run yet", p.muted)]),
        ])),
        row(12.0, vec![toggle(p, st.schedules_on, act.on(|s| s.schedules_on = !s.schedules_on)), grow(text("Run schedules in the background — a login service starts at sign-in; unattended runs still stop at every approval.", 13.5, p.ink))]),
    ])
}

// ── Organization ─────────────────────────────────────────────────

fn organization(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    stretch(14.0, vec![
        section(p, "Your organization's rules", "A signed policy file can limit which tools, providers and destinations plans may use."),
        notice(p, p.muted, "No policy to check it against. Plans follow this computer's settings."),
        row(12.0, vec![toggle(p, st.verify_all, act.on(|s| s.verify_all = !s.verify_all)), grow(text("Verify every receipt when the window opens", 13.5, p.ink))]),
        section(p, "Every receipt on this computer", "Checked with the same seven checks as wright verify."),
        inset(p, stretch(8.0, vec![
            row(10.0, vec![grow(text("Weekly Sales Report · run 1 · 2026-11-09 09:00", 13.0, p.ink)), pill(p, "verified", p.good)]),
            row(10.0, vec![grow(text("Word Count CLI · run 1 · 2026-11-06 16:42", 13.0, p.ink)), pill(p, "verified", p.good)]),
        ])),
    ])
}

// ── Workspace ────────────────────────────────────────────────────

fn workspace(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    stretch(14.0, vec![
        section(p, "Projects folder", "Where a new session's folder is made."),
        row(10.0, vec![
            grow(labelled_field(p, "Folder", &st.folder, "~/wright-projects", act.text(|s, t| s.folder = t))),
            pad(EdgeInsets::only(0.0, 20.0, 0.0, 0.0), button(p, "Save", Look::Primary, act.on(|s| { let m = format!("New projects go in {}", s.folder); s.say(m); }))),
        ]),
        section(p, "Wright's own records", "Run logs, receipts and caches live outside your projects (~/.wright/projects/…), so a sandboxed agent can never touch them."),
        button(p, "Show in folder", Look::Secondary, act.on(|s| s.say("Opened ~/.wright/projects"))),
    ])
}

// ── This project ─────────────────────────────────────────────────

fn this_project(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    stretch(14.0, vec![
        section(p, "This project", "Model tiers and limits for the open session's blueprint. Saving changes the blueprint; nothing re-runs because of a budget."),
        row(12.0, vec![
            grow(labelled_field(p, "Everyday work: prefer", &st.tiers[0], "deepseek-v4-flash", act.text(|s, t| s.tiers[0] = t))),
            grow(labelled_field(p, "Helpers: prefer", &st.tiers[1], "claude-haiku-5-5", act.text(|s, t| s.tiers[1] = t))),
        ]),
        labelled_field(p, "Careful work (review, delivery): prefer", &st.tiers[2], "claude-sonnet-5-5", act.text(|s, t| s.tiers[2] = t)),
        row(12.0, vec![
            grow(labelled_field(p, "Per run ($)", &st.per_run, "0.50", act.text(|s, t| s.per_run = t))),
            grow(labelled_field(p, "Per day ($)", &st.per_day, "2.00", act.text(|s, t| s.per_day = t))),
        ]),
        row(10.0, vec![button(p, "Save to the blueprint", Look::Primary, act.on(|s| s.say("Saved to the blueprint as commit b71e04d.")))]),
    ])
}


// ═════════════════════════════════════════════════════════════════
// OVERLAYS
// The engine (live agents, tools and log), the shortcuts sheet and the
// receipt.



/// The engine page: the running session's agents as a live graph, and the
/// log underneath.
pub fn engine_page(p: &Pal, act: &Act, st: &FlowState) -> WidgetNode {
    let idx = st
        .sessions
        .iter()
        .rposition(|s| matches!(s.stage, Stage::Running | Stage::Approval))
        .or_else(|| st.sessions.len().checked_sub(1));
    let Some(i) = idx else {
        return page(act, st, 3, 900.0, col(10.0, vec![bold("Nothing is running", 22.0, p.ink), text("Start a session on Home; its agents appear here as they work.", 14.0, p.muted)]));
    };
    let s = &st.sessions[i];
    let started = s.run_no > 0;
    let state_of = |k: usize| -> (&'static str, Color) {
        let at = k as f32 / 3.0;
        if !started {
            ("Waiting", p.faint)
        } else if k == 3 && s.stage == Stage::Approval {
            ("Asks first", p.wait)
        } else if k == 3 && s.stage == Stage::Refused {
            ("Refused", p.bad)
        } else if s.p >= at - 0.01 && (k < 3 || s.stage == Stage::Done) {
            ("Done", p.good)
        } else if s.p > at - 1.0 / 3.0 {
            (if s.paused { "Paused" } else { "Working" }, if s.paused { p.wait } else { p.live })
        } else {
            ("Waiting", p.faint)
        }
    };
    let tools = [["http-read"], ["files-write"], ["run-tests"], ["http-post"]];
    let tiers = ["built-in · no model", "helper · deepseek-v4-flash", "helper · claude-haiku-5-5", "delivers · claude-haiku-5-5"];
    let cards: Vec<WidgetNode> = STEPS
        .iter()
        .enumerate()
        .map(|(k, (_, agent, what, ..))| {
            let (label, c) = state_of(k);
            let live = label == "Working" || label == "Asks first";
            grow(
                Container::new()
                    .radius(18.0)
                    .color(p.glass)
                    .border(Border::new(if live { c.with_alpha(200) } else { p.edge }, 1.0))
                    .shadow(Shadow::new(c.with_alpha(if live { 80 } else { 0 }), Offset::ZERO, 26.0))
                    .padding(EdgeInsets::all(16.0))
                    .child(stretch(8.0, vec![
                        row(8.0, vec![grow(bold(agent.to_string(), 15.0, p.ink)), pill(p, label, c)]),
                        text(tiers[k], 12.0, p.faint),
                        text(what.to_string(), 13.0, p.muted),
                        row(6.0, tools[k].iter().map(|t| pill(p, *t, if *t == "http-post" { p.wait } else { p.muted })).collect()),
                    ])),
            )
        })
        .collect();
    let shown = if !started { 0 } else if s.stage == Stage::Done { LOG.len() } else { ((s.p / GATE) * (LOG.len() as f32)).ceil().min(LOG.len() as f32) as usize };
    let mut log: Vec<WidgetNode> = LOG
        .iter()
        .take(shown)
        .enumerate()
        .map(|(n, (who, what))| row(14.0, vec![SizedBox::width(52.0).child(mono(format!("+{:.1}s", n as f32 * 1.1), 12.0, p.faint)).into(), SizedBox::width(110.0).child(bold(who.to_string(), 12.5, p.live)).into(), grow(text(what.to_string(), 13.0, p.ink))]))
        .collect();
    if s.stage == Stage::Done {
        log.push(row(14.0, vec![SizedBox::width(52.0).child(mono("+9.9s", 12.0, p.faint)).into(), SizedBox::width(110.0).child(bold("wright", 12.5, p.good)).into(), grow(text("done: 6 ran, 0 cached, cost $0.0041", 13.0, p.ink))]));
    }
    if log.is_empty() {
        log.push(text("Not run yet — Approve & run on the plan, and the log fills here.", 13.0, p.muted));
    }
    page(act, st, 3, 1180.0, stretch(18.0, vec![
        row(12.0, vec![
            grow(col(3.0, vec![
                bold(s.title.clone(), 24.0, p.ink),
                text(format!("Run {} · ${:.4} of $0.50 · 3.6k tokens in · 0.2k out", s.run_no.max(1), 0.0041 * s.p), 13.0, p.muted),
            ])),
            button(p, "Open the session", Look::Secondary, act.on(move |s| s.open(i))),
            button(p, "Hide", Look::Quiet, act.on(move |s| s.open(i))),
        ]),
        Flex::row().spacing(14.0).cross_axis_alignment(CrossAxisAlignment::Start).children(cards).into(),
        glass(p, 18.0, 16.0, stretch(8.0, {
            let mut v = vec![row(8.0, vec![pill(p, "LIVE", if started && !matches!(s.stage, Stage::Done | Stage::Refused) { p.live } else { p.faint }), bold("Live log", 14.0, p.ink)])];
            v.extend(log);
            v
        }))
        .into(),
    ]))
}

pub fn help_sheet(p: &Pal, act: &Act) -> WidgetNode {
    let rows: Vec<WidgetNode> = SHORTCUTS
        .iter()
        .map(|(k, v)| row_top(16.0, vec![SizedBox::width(190.0).child(bold(k.to_string(), 14.0, p.ink)).into(), grow(text(v.to_string(), 13.5, p.muted))]))
        .collect();
    glass(p, 24.0, 24.0, stretch(16.0, vec![
        row(10.0, vec![grow(bold("Gestures and shortcuts", 22.0, p.ink)), icon_button(p, Glyph::Close, "Close", false, act.on(|s| s.help = false))]),
        stretch(14.0, rows),
        row(10.0, vec![
            button(p, "Agents & tools", Look::Secondary, act.on(|s| { s.help = false; s.settings = true; s.tab = 1; })),
            button(p, "Replay the opening", Look::Quiet, act.on(|s| { s.help = false; s.say("The opening replays when the window next starts."); })),
            gap(),
            button(p, "Done", Look::Primary, act.on(|s| s.help = false)),
        ]),
    ]))
    .into()
}

pub fn receipt_sheet(p: &Pal, act: &Act) -> WidgetNode {
    let checks: Vec<WidgetNode> = CHECKS
        .iter()
        .map(|(name, what)| {
            Container::new()
                .radius(12.0)
                .color(p.wash)
                .padding(EdgeInsets::symmetric(12.0, 10.0))
                .child(row_top(10.0, vec![Glyph::Tick.widget(18.0, p.good), grow(col(2.0, vec![bold(name.to_string(), 14.0, p.ink), text(what.to_string(), 12.5, p.muted)]))]))
                .into()
        })
        .collect();
    glass(p, 24.0, 24.0, stretch(12.0, vec![
        row(10.0, vec![
            Glyph::Tick.widget(24.0, p.good),
            grow(bold("The receipt · run 1", 22.0, p.ink)),
            pill(p, "verified", p.good),
        ]),
        text("Every check over the signed receipt, in verify's own words:", 13.0, p.muted),
        cascade("receipt", 8.0, checks),
        Container::new()
            .radius(12.0)
            .color(p.good.with_alpha(26))
            .border(Border::new(p.good.with_alpha(90), 1.0))
            .padding(EdgeInsets::symmetric(14.0, 12.0))
            .child(bold("verified: every check passed. This receipt is evidence of what ran, what it touched and what it cost.", 13.5, p.good))
            .into(),
        row(10.0, vec![
            button(p, "Copy the report", Look::Secondary, act.on(|s| s.say("Copied the report"))),
            gap(),
            button(p, "Done", Look::Primary, act.on(|s| s.receipt = false)),
        ]),
    ]))
    .into()
}
