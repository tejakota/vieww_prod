//! Wright · Flow — every screen of the revamped window, as one interactive
//! vieww app. Prototype only: the real UI lives in `apps/wright/app/src/ui`.
//!
//! Screens: Home → a session (plan as a flow, run, approval, result) → the
//! receipt; All sessions; Settings; the engine, live. They share one state,
//! so what you start on Home is what the other screens show.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use vieww::prelude::*;
use vieww::widget::ElementState;

// ── palette: rich contrast on a deep ground ──────────────────────

const GROUND: Color = Color::rgb(7, 8, 13);
const GLASS: Color = Color::rgba(22, 24, 36, 214);
const EDGE: Color = Color::rgba(255, 255, 255, 24);
const INK: Color = Color::rgb(244, 245, 250);
const MUTED: Color = Color::rgb(166, 171, 194);
const FAINT: Color = Color::rgb(138, 143, 168);
const VIOLET: Color = Color::rgb(124, 92, 255);
const CYAN: Color = Color::rgb(34, 211, 238);
const MAGENTA: Color = Color::rgb(244, 63, 142);
const MINT: Color = Color::rgb(52, 230, 160);
const AMBER: Color = Color::rgb(245, 181, 74);
const RED: Color = Color::rgb(229, 72, 77);

fn ramp() -> Gradient {
    Gradient::horizontal().with_stops(&[(0.0, VIOLET), (0.55, CYAN), (1.0, MINT)])
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// ── state ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Session,
    Overview,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Plan,
    /// Travelling to the approval point.
    ToGate,
    /// Allowed: travelling to the end.
    ToEnd,
}

#[derive(Debug)]
struct FlowState {
    screen: Screen,
    back: Screen,
    picked: Option<usize>,
    session: Option<usize>,
    run: Run,
    receipt: bool,
    engine: bool,
    tab: usize,
    pending: bool,
}

impl Default for FlowState {
    fn default() -> Self {
        FlowState {
            screen: Screen::Home,
            back: Screen::Home,
            picked: None,
            session: None,
            run: Run::Plan,
            receipt: false,
            engine: false,
            tab: 0,
            pending: false,
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
}

/// A cheap, clonable way for handlers to change the state.
#[derive(Clone)]
struct Act(Option<Rc<RefCell<dyn ElementState>>>);

impl Act {
    fn with(&self, f: impl FnOnce(&mut FlowState)) {
        if let Some(h) = &self.0 {
            if let Some(s) = h.borrow_mut().as_any_mut().downcast_mut::<FlowState>() {
                f(s);
                s.pending = true;
            }
        }
    }
    fn on(&self, f: impl Fn(&mut FlowState) + 'static) -> impl Fn() + 'static {
        let me = self.clone();
        move || me.with(|s| f(s))
    }
}

const STARTERS: [(&str, &str, &str); 3] = [
    ("Daily digest", "Fetch · summarize · email, every morning",
     "Every morning at 8, fetch the top Hacker News stories, summarize them, and email me the digest."),
    ("Tested CLI tool", "Code, tests, logging and CI in place",
     "A command line tool that counts words in files, with tests, logging and CI."),
    ("Weekly report", "From a folder of CSV files",
     "Every Monday, read the CSV files in my sales folder and email me a one-page summary."),
];

const STEPS: [(&str, &str); 4] = [
    ("Collect", "Fetcher"),
    ("Summarize", "Summarizer"),
    ("Check", "Testwright · Reviewer"),
    ("Send", "Mailer"),
];

/// Where the run waits for the Mailer's approval.
const GATE: f32 = 0.7;

const TABS: [&str; 6] = ["Models & keys", "Agents & tools", "Defaults", "Appearance", "Network", "This project"];

// ── the app ──────────────────────────────────────────────────────

#[must_use]
pub fn screen() -> impl Widget {
    Home
}

#[derive(Debug)]
pub struct Home;

impl Widget for Home {
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
        let snap = ctx
            .state::<FlowState, _>(|s| (s.screen, s.picked, s.session, s.run, s.receipt, s.engine, s.tab))
            .unwrap_or((Screen::Home, None, None, Run::Plan, false, false, 0));
        let (screen, picked, session, run, receipt, engine, tab) = snap;
        let act = Act(ctx.state_handle());
        let busy = run != Run::Plan;

        let body: WidgetNode = match screen {
            Screen::Home => home(&act, picked, session),
            Screen::Session => session_page(&act, session.unwrap_or(0), run),
            Screen::Overview => overview(&act, session, run),
            Screen::Settings => settings(&act, tab),
        };
        // A new screen rises into place; keyed by screen so each change
        // is a fresh entrance rather than a cross-fade of two layouts.
        let body: WidgetNode = Animated::new(1.0)
            .from(0.0)
            .duration(ms(650))
            .curve(Curve::EXPO_OUT)
            .key(format!("{screen:?}"))
            .build(move |t| rise(t, 26.0, body.clone()))
            .into();

        let mut layers: Vec<WidgetNode> = vec![
            ColoredBox::new(GROUND).into(),
            Animated::new(if busy { 1.0 } else { 0.0 })
                .duration(ms(2400))
                .curve(Curve::SINE_IN_OUT)
                .build(|k| Painting::new(PaintWith::new(move |b, s| aurora(b, s, k))).into())
                .into(),
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(children![
                    top_bar(&act, screen, session),
                    Flexible::expanded(1).child(body),
                    status_bar(&act, screen, session, run),
                ])
                .into(),
        ];
        if engine {
            layers.push(overlay(engine_sheet(&act, run), "engine"));
        }
        if receipt {
            layers.push(overlay(receipt_sheet(&act), "receipt"));
        }
        Stack::new().fit(StackFit::Expand).children(layers).into()
    }
}

// ── motion helpers ───────────────────────────────────────────────

fn phase(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

fn rise(t: f32, dy: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    Opacity::new(t)
        .child(Transformed::new(Transform::translate(Offset::new(0.0, dy * (1.0 - t)))).child(child))
        .into()
}

/// Children that rise one after another off one clock.
fn cascade(items: Vec<WidgetNode>, gap: f32, key: &str) -> WidgetNode {
    let n = items.len().max(1) as f32;
    let items = Rc::new(items);
    Animated::new(1.0)
        .from(0.0)
        .duration(ms(1100))
        .curve(Curve::EXPO_OUT)
        .key(format!("cascade-{key}"))
        .build(move |t| {
            let kids: Vec<WidgetNode> = items
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let a = i as f32 / n * 0.45;
                    rise(phase(t, a, a + 0.55), 22.0, w.clone())
                })
                .collect();
            Flex::column()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(gap)
                .children(kids)
                .into()
        })
        .into()
}

fn overlay(sheet: WidgetNode, key: &str) -> WidgetNode {
    Animated::new(1.0)
        .from(0.0)
        .duration(ms(450))
        .curve(Curve::EXPO_OUT)
        .key(key.to_string())
        .build(move |t| {
            Opacity::new(t)
                .child(
                    Container::new()
                        .color(Color::rgba(7, 8, 13, 190))
                        .alignment(Alignment::CENTER)
                        .padding(EdgeInsets::all(24.0))
                        .child(
                            Transformed::new(Transform::scale_around(Offset::new(400.0, 300.0), 0.96 + 0.04 * t, 0.96 + 0.04 * t))
                                .child(Transformed::new(Transform::translate(Offset::new(0.0, 24.0 * (1.0 - t)))).child(sheet.clone())),
                        ),
                )
                .into()
        })
        .into()
}

// ── shared pieces ────────────────────────────────────────────────

fn text(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s).size(size).color(color).into()
}

fn bold(s: impl Into<String>, size: f32, color: Color) -> WidgetNode {
    Text::new(s).size(size).color(color).weight(FontWeight::Bold).into()
}

fn col(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Start).spacing(gap).children(items).into()
}

fn stretch(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(gap).children(items).into()
}

fn row(gap: f32, items: Vec<WidgetNode>) -> WidgetNode {
    Flex::row().cross_axis_alignment(CrossAxisAlignment::Center).spacing(gap).children(items).into()
}

fn grow(w: impl Into<WidgetNode>) -> WidgetNode {
    Flexible::expanded(1).child(w).into()
}

fn gap() -> WidgetNode {
    grow(SizedBox::shrink())
}

fn glass(radius: f32, pad: f32, child: impl Into<WidgetNode>) -> Container {
    Container::new().radius(radius).color(GLASS).border(Border::new(EDGE, 1.0)).padding(EdgeInsets::all(pad)).child(child)
}

/// A centred reading column, at most `max` wide.
fn column_of(max: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    Align::new(Alignment::TOP_CENTER)
        .child(Constrained::new(Constraints::new(0.0, max, 0.0, f32::INFINITY)).child(Container::new().padding(EdgeInsets::symmetric(28.0, 12.0)).child(child)))
        .into()
}

/// A pill button: `primary` draws it with the flow ramp and a glow.
fn pill(label: &str, primary: bool, on_tap: impl Fn() + 'static) -> WidgetNode {
    let label = label.to_string();
    Pressable::sensed(move |s: Sense| {
        let c = Container::new().padding(EdgeInsets::symmetric(18.0, 10.0)).radius(999.0);
        let c = if primary {
            c.gradient(ramp()).shadow(Shadow::new(CYAN.with_alpha((70.0 + 90.0 * s.hover) as u8), Offset::ZERO, 22.0))
        } else {
            c.color(Color::rgba(255, 255, 255, (14.0 + 20.0 * s.hover) as u8)).border(Border::new(EDGE, 1.0))
        };
        Transformed::new(Transform::translate(Offset::new(0.0, -1.5 * s.hover)))
            .child(c.child(bold(label.clone(), 14.0, if primary { GROUND } else { INK })))
            .into()
    })
    .on_tap(on_tap)
    .into()
}

fn badge(label: &str, fg: Color) -> WidgetNode {
    Container::new()
        .padding(EdgeInsets::symmetric(10.0, 3.0))
        .radius(999.0)
        .color(fg.with_alpha(36))
        .child(bold(label.to_string(), 12.0, fg))
        .into()
}

fn overline(s: &str) -> WidgetNode {
    bold(s.to_string(), 12.0, FAINT)
}

fn progress_bar(share: f32, height: f32) -> WidgetNode {
    let done = (share.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let mut parts: Vec<WidgetNode> = Vec::new();
    if done > 0 {
        parts.push(Flexible::expanded(done).child(Container::new().height(height).radius(height / 2.0).gradient(ramp())).into());
    }
    if done < 1000 {
        parts.push(Flexible::expanded(1000 - done).child(SizedBox::height(height)).into());
    }
    Container::new().height(height).radius(height / 2.0).color(Color::rgba(255, 255, 255, 20)).child(Flex::row().children(parts)).into()
}

fn status_of(run: Run, p: Option<f32>) -> (&'static str, Color, f32) {
    match run {
        Run::Plan => ("Plan ready", MUTED, 0.0),
        Run::ToGate => match p {
            Some(v) if v >= GATE - 0.001 => ("Waiting for you", AMBER, v),
            _ => ("Running", CYAN, p.unwrap_or(0.35)),
        },
        Run::ToEnd => match p {
            Some(v) if v < 0.999 => ("Running", CYAN, v),
            _ => ("Delivered · run 1", MINT, 1.0),
        },
    }
}

fn session_title(i: usize) -> &'static str {
    ["HN Daily Digest", "Word Count CLI", "Weekly Sales Report"][i % 3]
}

// ── top and bottom bars ──────────────────────────────────────────

fn top_bar(act: &Act, screen: Screen, session: Option<usize>) -> WidgetNode {
    let pages = 1 + usize::from(session.is_some());
    let at = usize::from(screen == Screen::Session);
    let dots: Vec<WidgetNode> = (0..pages)
        .map(|i| {
            Animated::new(if i == at { 1.0 } else { 0.0 })
                .duration(ms(450))
                .curve(Curve::EXPO_OUT)
                .build(|k| {
                    let c = Container::new().width(6.0 + 16.0 * k).height(6.0).radius(3.0);
                    if k > 0.01 { c.gradient(ramp()).into() } else { c.color(Color::rgba(255, 255, 255, 64)).into() }
                })
                .into()
        })
        .collect();
    Container::new()
        .padding(EdgeInsets::symmetric(28.0, 16.0))
        .child(row(10.0, vec![
            Pressable::new(|_| row(10.0, vec![
                Painting::sized(Size::new(24.0, 24.0), PaintWith::new(mark)).into(),
                bold("wright", 18.0, INK),
            ]))
            .on_tap(act.on(|s| { s.screen = Screen::Home; s.engine = false; s.receipt = false; }))
            .into(),
            gap(),
            row(6.0, dots),
            gap(),
            pill("All sessions", screen == Screen::Overview, act.on(|s| { s.screen = Screen::Overview; s.engine = false; })),
            pill("Engine", false, act.on(|s| s.engine = !s.engine)),
            pill("Settings", screen == Screen::Settings, act.on(|s| {
                if s.screen != Screen::Settings { s.back = s.screen; }
                s.screen = Screen::Settings;
                s.engine = false;
            })),
        ]))
        .into()
}

fn status_bar(act: &Act, screen: Screen, session: Option<usize>, run: Run) -> WidgetNode {
    let (where_, status, tone) = match (screen, session) {
        (Screen::Session, Some(i)) => {
            let (st, c, _) = status_of(run, None);
            (session_title(i).to_string(), st.to_string(), c)
        }
        (Screen::Overview, _) => ("All sessions".into(), format!("{} session(s)", usize::from(session.is_some())), FAINT),
        (Screen::Settings, _) => ("Settings".into(), "Models, agents, defaults, appearance".into(), FAINT),
        _ => ("Home".into(), format!("{} session(s) · 0 saved", usize::from(session.is_some())), FAINT),
    };
    let stop = Pressable::sensed(|s: Sense| {
        Container::new()
            .padding(EdgeInsets::symmetric(10.0, 4.0))
            .radius(6.0)
            .color(RED.with_alpha((230.0 * s.hover) as u8))
            .child(text("Stop everything", 12.0, MUTED.lerp(INK, s.hover)))
            .into()
    })
    .on_tap(act.on(|s| s.run = Run::Plan));
    Container::new()
        .height(28.0)
        .color(Color::rgba(7, 8, 13, 200))
        .border(Border::new(Color::rgba(255, 255, 255, 18), 1.0))
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(12.0)
                .children(children![
                    Container::new().height(28.0).padding(EdgeInsets::symmetric(12.0, 0.0)).gradient(Gradient::horizontal().with_stops(&[(0.0, VIOLET), (1.0, CYAN)])).alignment(Alignment::CENTER).child(bold("wright", 12.0, GROUND)),
                    text(where_, 12.0, INK),
                    Container::new().width(7.0).height(7.0).radius(3.5).color(tone),
                    grow(text(status, 12.0, MUTED)),
                    Container::new().width(1.0).height(14.0).color(Color::rgba(255, 255, 255, 38)),
                    stop,
                    SizedBox::width(6.0),
                ]),
        )
        .into()
}

// ── Home ─────────────────────────────────────────────────────────

fn home(act: &Act, picked: Option<usize>, session: Option<usize>) -> WidgetNode {
    let prompt = picked.map(|i| STARTERS[i].2.to_string());
    let filled = prompt.is_some();
    let send = {
        let act = act.clone();
        move || act.with(|s| {
            if let Some(i) = s.picked {
                s.session = Some(i);
                s.run = Run::Plan;
                s.screen = Screen::Session;
                s.picked = None;
            }
        })
    };
    let composer = Container::new()
        .radius(24.0)
        .color(GLASS)
        .border(Border::new(if filled { VIOLET.with_alpha(170) } else { EDGE }, 1.0))
        .shadow(Shadow::new(VIOLET.with_alpha(if filled { 80 } else { 30 }), Offset::new(0.0, 18.0), 50.0))
        .padding(EdgeInsets::only(22.0, 12.0, 12.0, 12.0))
        .child(row(12.0, vec![
            grow(text(prompt.unwrap_or_else(|| "Describe the outcome in your own words…".into()), 17.0, if filled { INK } else { FAINT })),
            Painting::sized(Size::new(44.0, 44.0), PaintWith::new(mic)).into(),
            Pressable::sensed(|s: Sense| {
                Container::new()
                    .width(48.0)
                    .height(48.0)
                    .radius(24.0)
                    .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, VIOLET), (1.0, CYAN)]))
                    .shadow(Shadow::new(CYAN.with_alpha((90.0 + 90.0 * s.hover) as u8), Offset::ZERO, 20.0))
                    .child(Painting::new(PaintWith::new(arrow)))
                    .into()
            })
            .on_tap(send)
            .into(),
        ]));
    let starters: Vec<WidgetNode> = STARTERS
        .iter()
        .enumerate()
        .map(|(i, (title, sub, _))| {
            let (title, sub) = (title.to_string(), sub.to_string());
            let on = picked == Some(i);
            grow(
                Pressable::sensed(move |s: Sense| {
                    let (title, sub) = (title.clone(), sub.clone());
                    Animated::new(if on { 1.0 } else { 0.45 * s.hover })
                        .duration(ms(340))
                        .curve(Curve::EXPO_OUT)
                        .build(move |k| starter_card(&title, &sub, k))
                        .into()
                })
                .on_tap(act.on(move |s| s.picked = Some(i))),
            )
        })
        .collect();
    let mut items = vec![
        row(8.0, vec![
            Container::new().width(8.0).height(8.0).radius(4.0).color(MINT).shadow(Shadow::new(MINT.with_alpha(170), Offset::ZERO, 10.0)).into(),
            text("9 agents · 14 tools · runs on this machine", 13.0, MUTED),
        ]),
        bold("What should exist?", 56.0, INK),
        text("Wright drafts a plan, shows what it will touch and what it costs — and runs nothing until you approve.", 17.0, MUTED),
        SizedBox::height(8.0).into(),
        composer.into(),
        overline("OR START FROM ONE"),
        Flex::row().spacing(14.0).children(starters).into(),
    ];
    if let Some(i) = session {
        let open = act.on(|s| s.screen = Screen::Session);
        items.push(overline("SESSIONS"));
        items.push(
            Pressable::sensed(move |s: Sense| {
                Transformed::new(Transform::translate(Offset::new(0.0, -3.0 * s.hover)))
                    .child(glass(18.0, 18.0, stretch(8.0, vec![
                        bold(session_title(i), 16.0, INK),
                        text("Open it to run the plan", 13.0, MUTED),
                    ])).width(320.0))
                    .into()
            })
            .on_tap(open)
            .into(),
        );
    }
    column_of(860.0, cascade(items, 16.0, "home"))
}

fn starter_card(title: &str, sub: &str, k: f32) -> WidgetNode {
    Transformed::new(Transform::translate(Offset::new(0.0, -6.0 * k)))
        .child(
            Container::new()
                .height(100.0)
                .radius(18.0)
                .color(GLASS.lerp(Color::rgba(40, 36, 74, 235), k))
                .border(Border::new(EDGE.lerp(VIOLET.with_alpha(220), k), 1.0))
                .shadow(Shadow::new(VIOLET.with_alpha((70.0 * k) as u8), Offset::new(0.0, 14.0), 30.0))
                .padding(EdgeInsets::all(18.0))
                .child(col(6.0, vec![bold(title.to_string(), 15.0, INK), text(sub.to_string(), 13.0, MUTED)])),
        )
        .into()
}

// ── a session ────────────────────────────────────────────────────

fn session_page(act: &Act, i: usize, run: Run) -> WidgetNode {
    let target = match run {
        Run::Plan => 0.0,
        Run::ToGate => GATE,
        Run::ToEnd => 1.0,
    };
    let act2 = act.clone();
    let flow = Animated::new(target)
        .duration(ms(if run == Run::ToEnd { 1800 } else { 3000 }))
        .curve(if run == Run::ToEnd { Curve::EASE_OUT } else { Curve::SINE_IN_OUT })
        .key(format!("run-{i}"))
        .build(move |p| run_body(&act2, i, run, p));
    let bubble = Container::new()
        .padding(EdgeInsets::symmetric(18.0, 12.0))
        .radius(20.0)
        .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, VIOLET.with_alpha(150)), (1.0, CYAN.with_alpha(90))]))
        .child(text(STARTERS[i % 3].2, 15.0, INK));
    let composer = glass(22.0, 10.0, row(10.0, vec![
        SizedBox::width(8.0).into(),
        grow(text("Ask for a change, e.g. make it weekly", 15.0, FAINT)),
        Container::new().width(44.0).height(44.0).radius(22.0).gradient(ramp()).child(Painting::new(PaintWith::new(arrow))).into(),
    ]));
    column_of(900.0, stretch(16.0, vec![
        Align::new(Alignment::CENTER_RIGHT).child(Constrained::new(Constraints::new(0.0, 640.0, 0.0, f32::INFINITY)).child(bubble)).into(),
        flow.into(),
        composer.into(),
    ]))
}

/// Everything that depends on how far the run has got: the flow, the step
/// states, the approval card at the gate, and the result at the end.
fn run_body(act: &Act, i: usize, run: Run, p: f32) -> WidgetNode {
    let at_gate = run == Run::ToGate && p >= GATE - 0.001;
    let done = run == Run::ToEnd && p >= 0.999;
    let (status, tone, _) = status_of(run, Some(p));
    let head_right: WidgetNode = match run {
        Run::Plan => pill("Run plan", true, act.on(|s| s.run = Run::ToGate)),
        _ if done => badge("Delivered ✓", MINT),
        _ => row(8.0, vec![Container::new().width(8.0).height(8.0).radius(4.0).color(tone).into(), bold(format!("{status} · {}%", (p * 100.0).round()), 14.0, tone)]),
    };
    let plan = glass(24.0, 22.0, stretch(14.0, vec![
        row(14.0, vec![
            grow(col(4.0, vec![
                bold(format!("The plan · {}", session_title(i)), 22.0, INK),
                text("4 steps · touches your inbox once · about $0.004 · nothing runs until you say so", 13.0, MUTED),
            ])),
            head_right,
        ]),
        SizedBox::height(110.0).child(Painting::new(PaintWith::new(move |b, s| track(b, s, p, run != Run::Plan)))).into(),
        step_labels(p, run, at_gate),
    ]));
    let mut items: Vec<WidgetNode> = vec![plan.into()];
    if at_gate {
        items.push(appear("gate", approval(act)));
    }
    if done {
        items.push(appear("done", result(act)));
    }
    stretch(16.0, items)
}

fn appear(key: &str, child: WidgetNode) -> WidgetNode {
    Animated::new(1.0).from(0.0).duration(ms(520)).curve(Curve::EXPO_OUT).key(key.to_string()).build(move |t| rise(t, 30.0, child.clone())).into()
}

fn step_labels(p: f32, run: Run, at_gate: bool) -> WidgetNode {
    let n = STEPS.len();
    let labels: Vec<WidgetNode> = STEPS
        .iter()
        .enumerate()
        .map(|(i, (name, agent))| {
            let at = i as f32 / (n - 1) as f32;
            let prev = if i == 0 { -1.0 } else { (i - 1) as f32 / (n - 1) as f32 };
            let (state, c) = if run == Run::Plan {
                ("waiting", FAINT)
            } else if i == 3 && at_gate {
                ("asks first", AMBER)
            } else if p >= at - 0.01 {
                ("done", MINT)
            } else if p > prev {
                ("working", CYAN)
            } else {
                ("waiting", FAINT)
            };
            grow(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(2.0)
                    .children(children![bold(name.to_string(), 14.0, INK), bold(state, 12.0, c), text(agent.to_string(), 12.0, FAINT)]),
            )
        })
        .collect();
    Flex::row().children(labels).into()
}

fn approval(act: &Act) -> WidgetNode {
    Container::new()
        .radius(20.0)
        .gradient(Gradient::horizontal().with_stops(&[(0.0, AMBER.with_alpha(46)), (1.0, GLASS)]))
        .border(Border::new(AMBER.with_alpha(120), 1.0))
        .padding(EdgeInsets::all(18.0))
        .child(row(14.0, vec![
            Painting::sized(Size::new(24.0, 24.0), PaintWith::new(warn)).into(),
            grow(col(4.0, vec![
                bold("Mailer is asking before it touches the world", 15.0, INK),
                text("One email to you@example.com with the digest. Checks passed: Testwright ✓  Reviewer ✓", 13.0, Color::rgb(217, 201, 168)),
            ])),
            pill("Don't", false, act.on(|s| s.run = Run::Plan)),
            pill("Allow", true, act.on(|s| s.run = Run::ToEnd)),
        ]))
        .into()
}

fn result(act: &Act) -> WidgetNode {
    glass(24.0, 22.0, stretch(14.0, vec![
        row(12.0, vec![
            Container::new().width(36.0).height(36.0).radius(18.0).color(MINT.with_alpha(40)).shadow(Shadow::new(MINT.with_alpha(90), Offset::ZERO, 18.0)).child(Painting::new(PaintWith::new(tick))).into(),
            grow(col(2.0, vec![
                bold("Delivered and verified", 20.0, INK),
                text("done: 4 ran, 0 cached · cost $0.0041 · the digest is in your inbox", 13.0, MUTED),
            ])),
        ]),
        row(10.0, vec![
            pill("See the receipt", true, act.on(|s| s.receipt = true)),
            pill("Run again", false, act.on(|s| s.run = Run::ToGate)),
            pill("Export", false, || {}),
            pill("Open the folder", false, || {}),
        ]),
    ]))
    .border(Border::new(MINT.with_alpha(100), 1.0))
    .into()
}

// ── All sessions ─────────────────────────────────────────────────

fn overview(act: &Act, session: Option<usize>, run: Run) -> WidgetNode {
    let mut cards: Vec<WidgetNode> = vec![mini_card("Home", "What should exist?", badge("Home", MUTED), 0.0, act.on(|s| s.screen = Screen::Home))];
    if let Some(i) = session {
        let (st, c, share) = status_of(run, None);
        cards.push(mini_card(session_title(i), STARTERS[i % 3].2, badge(st, c), share, act.on(|s| s.screen = Screen::Session)));
    }
    cards.push(mini_card("New session", "Describe what should exist", badge("+ New", CYAN), 0.0, act.on(|s| s.screen = Screen::Home)));
    let n = cards.len();
    let cards = Rc::new(cards);
    let grid = Animated::new(1.0).from(0.0).duration(ms(900)).curve(Curve::EXPO_OUT).key("overview".to_string()).build(move |t| {
        let kids: Vec<WidgetNode> = cards
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let a = i as f32 / n as f32 * 0.4;
                let k = phase(t, a, a + 0.6);
                grow(Opacity::new(k).child(Transformed::new(Transform::translate(Offset::new(0.0, 40.0 * (1.0 - k)))).child(c.clone())))
            })
            .collect();
        Flex::row().spacing(20.0).cross_axis_alignment(CrossAxisAlignment::Start).children(kids).into()
    });
    column_of(1120.0, stretch(24.0, vec![
        Center::new().child(text("Click a session to open it", 13.0, MUTED)).into(),
        SizedBox::height(40.0).into(),
        grid.into(),
    ]))
}

fn mini_card(title: &str, preview: &str, badge: WidgetNode, share: f32, on_tap: impl Fn() + 'static) -> WidgetNode {
    let (title, preview) = (title.to_string(), preview.to_string());
    Pressable::sensed(move |s: Sense| {
        stretch(10.0, vec![
            Transformed::new(Transform::translate(Offset::new(0.0, -5.0 * s.hover)))
                .child(
                    Container::new()
                        .height(210.0)
                        .radius(20.0)
                        .color(GLASS)
                        .border(Border::new(EDGE.lerp(VIOLET.with_alpha(200), s.hover), 1.0))
                        .shadow(Shadow::new(VIOLET.with_alpha((60.0 * s.hover) as u8), Offset::new(0.0, 18.0), 36.0))
                        .padding(EdgeInsets::all(18.0))
                        .child(Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(12.0).children(vec![
                            Align::new(Alignment::CENTER_LEFT).child(badge.clone()).into(),
                            text(preview.clone(), 14.0, Color::rgb(215, 218, 232)),
                            gap(),
                            progress_bar(share, 4.0),
                        ])),
                )
                .into(),
            bold(title.clone(), 14.0, INK),
        ])
    })
    .on_tap(on_tap)
    .into()
}

// ── Settings ─────────────────────────────────────────────────────

fn settings(act: &Act, tab: usize) -> WidgetNode {
    let tabs: Vec<WidgetNode> = TABS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let on = tab == i;
            let name = name.to_string();
            Pressable::sensed(move |s: Sense| {
                Animated::new(if on { 1.0 } else { 0.0 }).duration(ms(300)).curve(Curve::EXPO_OUT).build({
                    let name = name.clone();
                    let hover = s.hover;
                    move |k| {
                        Container::new()
                            .padding(EdgeInsets::symmetric(14.0, 10.0))
                            .radius(10.0)
                            .color(Color::rgba(255, 255, 255, (12.0 * hover) as u8).lerp(VIOLET.with_alpha(70), k))
                            .child(text(name.clone(), 14.0, MUTED.lerp(INK, k.max(hover))))
                            .into()
                    }
                })
                .into()
            })
            .on_tap(act.on(move |s| s.tab = i))
            .into()
        })
        .collect();
    let mut nav = vec![bold("Settings", 22.0, INK), SizedBox::height(8.0).into()];
    nav.extend(tabs);
    let content: WidgetNode = match tab {
        0 => models_tab(),
        3 => appearance_tab(),
        _ => col(10.0, vec![bold(TABS[tab], 18.0, INK), text("This section keeps its current controls; the prototype shows the frame they sit in.", 14.0, MUTED)]),
    };
    let content = Animated::new(1.0).from(0.0).duration(ms(420)).curve(Curve::EXPO_OUT).key(format!("tab-{tab}")).build(move |t| rise(t, 14.0, content.clone()));
    column_of(1080.0, glass(26.0, 24.0, Flex::row().cross_axis_alignment(CrossAxisAlignment::Start).spacing(28.0).children(children![
        SizedBox::width(220.0).child(stretch(4.0, nav)),
        grow(content),
    ])))
}

fn models_tab() -> WidgetNode {
    let model = |name: &str, what: &str| -> WidgetNode {
        Container::new()
            .radius(16.0)
            .color(Color::rgba(255, 255, 255, 10))
            .border(Border::new(EDGE, 1.0))
            .padding(EdgeInsets::all(16.0))
            .child(row(14.0, vec![
                grow(col(4.0, vec![row(8.0, vec![bold(name.to_string(), 15.0, INK), badge("Downloads on first use", CYAN)]), text(what.to_string(), 13.0, MUTED)])),
                pill("Download now", false, || {}),
            ]))
            .into()
    };
    stretch(14.0, vec![
        bold("Built-in models", 18.0, INK),
        text("Two small open models Wright runs on this machine: no key, no account, nothing leaves it.", 14.0, MUTED),
        model("qwen3-1.7b", "Writes, plans and calls tools · 1.83 GB · Apache-2.0"),
        model("whisper-base", "Speech to text in 99 languages · 148 MB · MIT"),
        SizedBox::height(6.0).into(),
        bold("Model providers", 18.0, INK),
        Container::new()
            .radius(16.0)
            .color(Color::rgba(255, 255, 255, 10))
            .border(Border::new(EDGE, 1.0))
            .padding(EdgeInsets::all(16.0))
            .child(stretch(10.0, vec![
                row(10.0, vec![bold("anthropic", 15.0, INK), badge("Key missing", AMBER), gap(), pill("Set key", false, || {})]),
                Container::new()
                    .radius(10.0)
                    .color(AMBER.with_alpha(26))
                    .border(Border::new(AMBER.with_alpha(90), 1.0))
                    .padding(EdgeInsets::symmetric(12.0, 10.0))
                    .child(text("keychain:wright/anthropic cannot be read: the keychain tool (secret-tool) is not installed — install it, or set WRIGHT_SECRET_WRIGHT_ANTHROPIC", 13.0, Color::rgb(236, 227, 208)))
                    .into(),
            ]))
            .into(),
    ])
}

fn appearance_tab() -> WidgetNode {
    let swatch = |a: Color, b: Color, on: bool| -> WidgetNode {
        Container::new()
            .width(64.0)
            .height(64.0)
            .radius(18.0)
            .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, a), (1.0, b)]))
            .border(Border::new(if on { INK } else { Color::TRANSPARENT }, 3.0))
            .into()
    };
    col(14.0, vec![
        bold("Appearance", 18.0, INK),
        text("The accent drives the flow, the buttons and the light in the room.", 14.0, MUTED),
        row(12.0, vec![swatch(VIOLET, CYAN, true), swatch(MAGENTA, AMBER, false), swatch(Color::rgb(14, 165, 233), MINT, false), swatch(Color::rgb(201, 204, 216), Color::rgb(107, 112, 136), false)]),
    ])
}

// ── overlays ─────────────────────────────────────────────────────

fn engine_sheet(act: &Act, run: Run) -> WidgetNode {
    let agents = [
        ("Interviewer", "Turns what you said into a blueprint", "read · ask", false),
        ("Fetcher", "Collects the stories", "http-read", run == Run::ToGate),
        ("Summarizer", "Writes the digest", "files · edit", run == Run::ToGate),
        ("Testwright", "Runs the project's tests", "run-tests · shell (sandboxed)", false),
        ("Reviewer", "Approves before anything leaves", "read · diff", false),
        ("Mailer", "Sends, only after you allow it", "smtp (asks first)", run == Run::ToEnd),
    ];
    let cell = |(name, role, tools, on): (&str, &str, &str, bool)| -> WidgetNode {
        grow(
            Container::new()
                .radius(16.0)
                .color(Color::rgba(255, 255, 255, 10))
                .border(Border::new(if on { CYAN.with_alpha(170) } else { EDGE }, 1.0))
                .shadow(Shadow::new(CYAN.with_alpha(if on { 70 } else { 0 }), Offset::ZERO, 24.0))
                .padding(EdgeInsets::all(16.0))
                .child(col(6.0, vec![
                    row(8.0, vec![Container::new().width(8.0).height(8.0).radius(4.0).color(if on { CYAN } else { Color::rgba(255, 255, 255, 80) }).into(), bold(name.to_string(), 15.0, INK)]),
                    text(role.to_string(), 13.0, MUTED),
                    text(tools.to_string(), 12.0, FAINT),
                ])),
        )
    };
    let rows: Vec<WidgetNode> = agents.chunks(3).map(|c| Flex::row().spacing(14.0).children(c.iter().map(|a| cell(*a)).collect::<Vec<_>>()).into()).collect();
    Constrained::new(Constraints::new(0.0, 960.0, 0.0, f32::INFINITY))
        .child(glass(26.0, 26.0, stretch(16.0, {
            let mut v = vec![row(12.0, vec![grow(bold("The engine, live", 22.0, INK)), pill("Close", false, act.on(|s| s.engine = false))])];
            v.extend(rows);
            v
        })))
        .into()
}

fn receipt_sheet(act: &Act) -> WidgetNode {
    let checks = [
        "The log is whole: 41 events, none rewritten",
        "Every input hash matches the store",
        "The plan that ran is the plan you approved",
        "Permissions held: one email, one recipient",
        "Testwright ran before delivery",
        "Reviewer approved before delivery",
        "The signature is this machine's key",
    ];
    let lines: Vec<WidgetNode> = checks
        .iter()
        .map(|c| {
            Container::new()
                .radius(12.0)
                .color(Color::rgba(255, 255, 255, 10))
                .padding(EdgeInsets::symmetric(12.0, 10.0))
                .child(row(10.0, vec![Painting::sized(Size::new(18.0, 18.0), PaintWith::new(tick)).into(), text(c.to_string(), 14.0, INK)]))
                .into()
        })
        .collect();
    let mut v = vec![row(12.0, vec![
        grow(col(3.0, vec![bold("The receipt · run 1", 22.0, INK), text("Signed on this machine; wright verify reads the same store.", 13.0, MUTED)])),
        pill("Close", false, act.on(|s| s.receipt = false)),
    ])];
    v.push(cascade(lines, 8.0, "receipt"));
    v.push(
        Container::new()
            .radius(12.0)
            .color(MINT.with_alpha(26))
            .border(Border::new(MINT.with_alpha(90), 1.0))
            .padding(EdgeInsets::symmetric(14.0, 12.0))
            .child(bold("Verdict: verified — 7 of 7 checks hold", 14.0, MINT))
            .into(),
    );
    Constrained::new(Constraints::new(0.0, 640.0, 0.0, f32::INFINITY)).child(glass(26.0, 26.0, stretch(12.0, v))).into()
}

// ── vector drawing ───────────────────────────────────────────────

fn bez(p0: Offset, p1: Offset, p2: Offset, p3: Offset, t: f32) -> Offset {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Offset::new(a * p0.dx + b * p1.dx + c * p2.dx + d * p3.dx, a * p0.dy + b * p1.dy + c * p2.dy + d * p3.dy)
}

fn glow(c: Color, a: u8) -> Gradient {
    Gradient::radial(Offset::new(0.5, 0.5), 0.5).with_stops(&[(0.0, c.with_alpha(a)), (1.0, c.with_alpha(0))])
}

fn aurora(book: &mut Sketchbook, size: Size, k: f32) {
    let (w, h) = (size.width, size.height);
    book.rect(Rect::new(-0.15 * w, -0.35 * h, 0.55 * w, 0.55 * h), glow(VIOLET, 120));
    book.rect(Rect::new((0.55 - 0.1 * k) * w, (-0.2 + 0.25 * k) * h, 1.2 * w, 0.75 * h), glow(CYAN, (70.0 + 50.0 * k) as u8));
    book.rect(Rect::new(0.2 * w, 0.55 * h, 0.95 * w, 1.35 * h), glow(MAGENTA, (55.0 - 20.0 * k) as u8));
    for (i, (y, amp, a)) in [(0.62_f32, 0.18_f32, 90_u8), (0.70, 0.12, 60), (0.78, 0.22, 40)].iter().enumerate() {
        let lift = (i as f32 + 1.0) * 0.03 * k;
        let mut path = Path::new();
        path.move_to(Offset::new(-0.05 * w, (y - lift) * h));
        path.cubic_to(Offset::new(0.3 * w, (y - amp - lift) * h), Offset::new(0.6 * w, (y + amp - lift) * h), Offset::new(1.05 * w, (y - 0.1 - lift) * h));
        let ramp = Gradient::horizontal().with_stops(&[(0.0, VIOLET.with_alpha(0)), (0.35, VIOLET.with_alpha(*a)), (0.7, CYAN.with_alpha(*a)), (1.0, CYAN.with_alpha(0))]);
        book.stroke(path, ramp, 2.0 + i as f32);
    }
}

fn track(book: &mut Sketchbook, size: Size, p: f32, started: bool) {
    let (w, h) = (size.width, size.height);
    let n = STEPS.len();
    let ys = [h * 0.42, h * 0.66, h * 0.32, h * 0.6];
    let nodes: Vec<Offset> = (0..n).map(|i| Offset::new(40.0 + (w - 80.0) * i as f32 / (n - 1) as f32, ys[i])).collect();
    let mut pts = Vec::new();
    for s in 0..n - 1 {
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
    book.stroke(line(pts.len()), Color::rgba(255, 255, 255, 30), 3.0);
    if started && p > 0.001 {
        let lit = ((pts.len() - 1) as f32 * p).round() as usize + 1;
        book.stroke(line(lit), ramp(), 4.0);
        book.circle(pts[lit - 1], 16.0, glow(CYAN, 210));
    }
    for (i, c) in nodes.iter().enumerate() {
        let at = i as f32 / (n - 1) as f32;
        let on = if started { ((p - at) * 10.0 + 1.0).clamp(0.0, 1.0) } else { 0.0 };
        if on > 0.0 {
            book.circle(*c, 18.0 + 6.0 * on, MINT.with_alpha((60.0 * on) as u8));
        }
        book.circle(*c, 11.0, Color::rgb(27, 30, 44).lerp(MINT, on));
        book.ring(*c, 11.0, 1.5, Color::rgba(255, 255, 255, (70.0 + 150.0 * on) as u8));
    }
}

fn mark(book: &mut Sketchbook, s: Size) {
    let (w, h) = (s.width, s.height);
    let mut path = Path::new();
    path.move_to(Offset::new(0.08 * w, 0.2 * h));
    path.line_to(Offset::new(0.3 * w, 0.82 * h));
    path.line_to(Offset::new(0.5 * w, 0.4 * h));
    path.line_to(Offset::new(0.7 * w, 0.82 * h));
    path.line_to(Offset::new(0.92 * w, 0.2 * h));
    book.stroke(path, ramp(), 2.6);
}

fn mic(book: &mut Sketchbook, s: Size) {
    let c = Offset::new(s.width / 2.0, s.height / 2.0);
    book.circle(c, s.width / 2.0, Color::rgba(255, 255, 255, 16));
    book.rrect(Rect::new(c.dx - 4.0, c.dy - 9.0, c.dx + 4.0, c.dy + 3.0), 4.0, MUTED);
    book.stroke(Path::arc(Offset::new(c.dx, c.dy - 1.0), 7.5, 0.0, std::f32::consts::PI), MUTED, 1.6);
    book.line(Offset::new(c.dx, c.dy + 6.5), Offset::new(c.dx, c.dy + 10.0), MUTED, 1.6);
}

fn arrow(book: &mut Sketchbook, s: Size) {
    let (cx, cy) = (s.width / 2.0, s.height / 2.0);
    let mut path = Path::new();
    path.move_to(Offset::new(cx - 7.0, cy));
    path.line_to(Offset::new(cx + 7.0, cy));
    path.move_to(Offset::new(cx + 1.5, cy - 6.0));
    path.line_to(Offset::new(cx + 7.0, cy));
    path.line_to(Offset::new(cx + 1.5, cy + 6.0));
    book.stroke(path, GROUND, 2.4);
}

fn tick(book: &mut Sketchbook, s: Size) {
    let (w, h) = (s.width, s.height);
    let mut path = Path::new();
    path.move_to(Offset::new(0.25 * w, 0.52 * h));
    path.line_to(Offset::new(0.43 * w, 0.7 * h));
    path.line_to(Offset::new(0.76 * w, 0.32 * h));
    book.stroke(path, MINT, 2.4);
}

fn warn(book: &mut Sketchbook, s: Size) {
    let (w, h) = (s.width, s.height);
    let mut path = Path::new();
    path.move_to(Offset::new(0.5 * w, 0.1 * h));
    path.line_to(Offset::new(0.95 * w, 0.88 * h));
    path.line_to(Offset::new(0.05 * w, 0.88 * h));
    path.close();
    book.stroke(path, AMBER, 2.0);
    book.line(Offset::new(0.5 * w, 0.4 * h), Offset::new(0.5 * w, 0.62 * h), AMBER, 2.0);
    book.circle(Offset::new(0.5 * w, 0.75 * h), 1.4, AMBER);
}
