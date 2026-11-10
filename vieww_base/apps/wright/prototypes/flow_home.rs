//! Wright · Flow — a prototype of the revamped Home, for `viewwstudio-cli`.
//!
//! Not part of the app: a design sketch in real vieww code, so the look and
//! the motion can be judged before the real UI (app/src/ui) is changed.
//!
//!   viewwstudio-cli render  flow_home.rs --width 1280 --height 800
//!   viewwstudio-cli animate flow_home.rs --width 1280 --height 800 \
//!       --tap "Daily digest" --do 'tap "Run plan" @1.6s' --duration 5s
//!
//! The direction: a deep, near-black ground with vector aurora light and
//! flowing ribbons behind the content; one high-contrast question; a glass
//! composer; and the plan drawn as a flow — nodes joined by curves that a
//! gradient runs along as the work moves, instead of a list of checkmarks.

use vieww::prelude::*;
use vieww::widget::ElementState;

// ── the palette: rich contrast on a deep ground ──────────────────

const GROUND: Color = Color::rgb(7, 8, 13);
const GLASS: Color = Color::rgba(22, 24, 36, 210);
const GLASS_EDGE: Color = Color::rgba(255, 255, 255, 26);
const INK: Color = Color::rgb(244, 245, 250);
const MUTED: Color = Color::rgb(148, 153, 176);
const FAINT: Color = Color::rgb(92, 97, 122);
const VIOLET: Color = Color::rgb(124, 92, 255);
const CYAN: Color = Color::rgb(34, 211, 238);
const MAGENTA: Color = Color::rgb(244, 63, 142);
const MINT: Color = Color::rgb(52, 230, 160);

/// The accent ramp everything active is drawn with.
fn flow_gradient() -> Gradient {
    Gradient::horizontal().with_stops(&[(0.0, VIOLET), (0.55, CYAN), (1.0, MINT)])
}

// ── state ────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Flow {
    /// Which starter was picked (it fills the composer and opens the plan).
    picked: Option<usize>,
    running: bool,
    pending: bool,
}

impl ElementState for Flow {
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

type Handle = Option<std::rc::Rc<std::cell::RefCell<dyn ElementState>>>;

fn update(handle: &Handle, f: impl FnOnce(&mut Flow)) {
    if let Some(h) = handle {
        if let Some(s) = h.borrow_mut().as_any_mut().downcast_mut::<Flow>() {
            f(s);
            s.pending = true;
        }
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

const STEPS: [&str; 4] = ["Collect", "Summarize", "Check", "Send"];

// ── the screen ───────────────────────────────────────────────────

#[derive(Debug)]
pub struct FlowHome;

impl Widget for FlowHome {
    fn debug_name(&self) -> &'static str {
        "FlowHome"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn create_state(&self) -> Option<Box<dyn ElementState>> {
        Some(Box::new(Flow::default()))
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let (picked, running) = ctx
            .state::<Flow, _>(|s| (s.picked, s.running))
            .unwrap_or((None, false));
        let handle = ctx.state_handle();

        // One entrance clock for the page; each block takes its own phase
        // of it, which staggers them without a timer per block.
        let content = Animated::new(1.0)
            .from(0.0)
            .duration(std::time::Duration::from_millis(1400))
            .curve(Curve::EXPO_OUT)
            .build(move |t| page(t, picked, running, handle.clone()));

        Stack::new()
            .fit(StackFit::Expand)
            .children(children![
                ColoredBox::new(GROUND),
                // The light moves when the plan runs: the aurora leans toward
                // the flow, so the whole room reacts, not one widget.
                Animated::new(if running { 1.0 } else { 0.0 })
                    .duration(std::time::Duration::from_millis(2600))
                    .curve(Curve::SINE_IN_OUT)
                    .build(|k| Painting::new(PaintWith::new(move |book, size| aurora(book, size, k))).into()),
                content,
            ])
            .into()
    }
}

/// `t` (0..1) remapped so `a..b` becomes 0..1: one block's slice of the
/// entrance.
fn phase(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

/// Rise into place: fade in while travelling up `dy` pixels.
fn rise(t: f32, dy: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    Opacity::new(t)
        .child(Transformed::new(Transform::translate(Offset::new(0.0, dy * (1.0 - t)))).child(child))
        .into()
}

fn page(t: f32, picked: Option<usize>, running: bool, handle: Handle) -> WidgetNode {
    let prompt = picked
        .map(|i| STARTERS[i].2.to_string())
        .unwrap_or_else(|| "Describe the outcome in your own words…".into());
    let mut column: Vec<WidgetNode> = vec![
        top_bar(),
        SizedBox::height(36.0).into(),
        rise(phase(t, 0.0, 0.5), 24.0, eyebrow()),
        SizedBox::height(14.0).into(),
        rise(
            phase(t, 0.05, 0.6),
            28.0,
            Text::new("What should exist?").size(52.0).weight(FontWeight::Bold).color(INK),
        ),
        SizedBox::height(12.0).into(),
        rise(
            phase(t, 0.12, 0.7),
            28.0,
            Text::new("Wright drafts a plan, shows what it will touch and what it costs — and runs nothing until you approve.")
                .size(17.0)
                .color(MUTED),
        ),
        SizedBox::height(30.0).into(),
        rise(phase(t, 0.2, 0.8), 32.0, composer(prompt, picked.is_some())),
        SizedBox::height(22.0).into(),
        rise(phase(t, 0.3, 0.9), 32.0, starters(picked, handle.clone())),
    ];
    if picked.is_some() {
        column.push(SizedBox::height(28.0).into());
        column.push(
            Animated::new(1.0)
                .from(0.0)
                .duration(std::time::Duration::from_millis(700))
                .curve(Curve::EXPO_OUT)
                .build(move |k| rise(k, 40.0, plan(running, handle.clone())))
                .into(),
        );
    }
    Align::new(Alignment::TOP_CENTER)
        .child(
            Constrained::new(Constraints::new(0.0, 820.0, 0.0, f32::INFINITY)).child(
                Container::new().padding(EdgeInsets::symmetric(32.0, 20.0)).child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .children(column),
                ),
            ),
        )
        .into()
}

// ── pieces ───────────────────────────────────────────────────────

fn top_bar() -> WidgetNode {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(10.0)
        .children(children![
            Painting::sized(Size::new(22.0, 22.0), PaintWith::new(|book, s| mark(book, s))),
            Text::new("wright").size(16.0).weight(FontWeight::Bold).color(INK),
            Flexible::expanded(1).child(SizedBox::shrink()),
            pill("All sessions", false),
            pill("Engine", false),
        ])
        .into()
}

fn pill(label: &str, on: bool) -> WidgetNode {
    let label = label.to_string();
    Pressable::sensed(move |s: Sense| {
        Container::new()
            .padding(EdgeInsets::symmetric(14.0, 7.0))
            .radius(999.0)
            .color(if on { VIOLET } else { Color::rgba(255, 255, 255, (10.0 + 18.0 * s.hover) as u8) })
            .border(Border::new(GLASS_EDGE, 1.0))
            .child(Text::new(label.clone()).size(13.0).color(INK))
            .into()
    })
    .into()
}

fn eyebrow() -> WidgetNode {
    Flex::row()
        .main_axis_size(MainAxisSize::Min)
        .spacing(8.0)
        .children(children![
            Container::new()
                .width(8.0)
                .height(8.0)
                .radius(4.0)
                .color(MINT)
                .shadow(Shadow::new(MINT.with_alpha(160), Offset::ZERO, 10.0)),
            Text::new("9 agents · 14 tools · runs on this machine").size(13.0).color(MUTED),
        ])
        .into()
}

fn composer(prompt: String, filled: bool) -> WidgetNode {
    Container::new()
        .radius(22.0)
        .color(GLASS)
        .border(Border::new(if filled { VIOLET.with_alpha(150) } else { GLASS_EDGE }, 1.0))
        .shadow(Shadow::new(VIOLET.with_alpha(if filled { 70 } else { 30 }), Offset::new(0.0, 18.0), 48.0))
        .padding(EdgeInsets::only(22.0, 18.0, 14.0, 18.0))
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(14.0)
                .children(children![
                    Flexible::expanded(1).child(
                        Text::new(prompt).size(17.0).color(if filled { INK } else { FAINT })
                    ),
                    Painting::sized(Size::new(40.0, 40.0), PaintWith::new(|book, s| mic(book, s))),
                    Container::new()
                        .width(46.0)
                        .height(46.0)
                        .radius(23.0)
                        .gradient(Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0)).with_stops(&[(0.0, VIOLET), (1.0, CYAN)]))
                        .shadow(Shadow::new(CYAN.with_alpha(110), Offset::ZERO, 18.0))
                        .child(Painting::new(PaintWith::new(|book, s| arrow(book, s)))),
                ]),
        )
        .into()
}

fn starters(picked: Option<usize>, handle: Handle) -> WidgetNode {
    let cards: Vec<WidgetNode> = STARTERS
        .iter()
        .enumerate()
        .map(|(i, (title, sub, _))| {
            let (title, sub) = (title.to_string(), sub.to_string());
            let on = picked == Some(i);
            let h = handle.clone();
            Flexible::expanded(1)
                .child(
                    Pressable::sensed(move |s: Sense| {
                        Animated::new(if on { 1.0 } else { s.hover * 0.5 })
                            .duration(std::time::Duration::from_millis(320))
                            .curve(Curve::EXPO_OUT)
                            .build({
                                let (title, sub) = (title.clone(), sub.clone());
                                move |k| starter_card(&title, &sub, k)
                            })
                            .into()
                    })
                    .on_tap(move || update(&h, |f| { f.picked = Some(i); f.running = false; })),
                )
                .into()
        })
        .collect();
    Flex::row().spacing(14.0).children(cards).into()
}

/// A starter, `k` = 0 resting … 1 picked: the edge picks up the flow ramp
/// and the card lifts toward the light.
fn starter_card(title: &str, sub: &str, k: f32) -> WidgetNode {
    Transformed::new(Transform::translate(Offset::new(0.0, -6.0 * k)))
        .child(
            Container::new()
                .height(96.0)
                .radius(18.0)
                .color(GLASS.lerp(Color::rgba(40, 36, 74, 230), k))
                .border(Border::new(GLASS_EDGE.lerp(VIOLET.with_alpha(200), k), 1.0))
                .shadow(Shadow::new(VIOLET.with_alpha((60.0 * k) as u8), Offset::new(0.0, 14.0), 30.0))
                .padding(EdgeInsets::all(18.0))
                .child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .spacing(6.0)
                        .children(children![
                            Text::new(title.to_string()).size(15.0).weight(FontWeight::Bold).color(INK),
                            Text::new(sub.to_string()).size(13.0).color(MUTED),
                        ]),
                ),
        )
        .into()
}

/// The plan as a flow: four nodes on a curved track, and — once it runs —
/// a gradient that travels the track, lighting each node as it arrives.
fn plan(running: bool, handle: Handle) -> WidgetNode {
    let h = handle.clone();
    let run = Pressable::sensed(move |s: Sense| {
        Container::new()
            .padding(EdgeInsets::symmetric(18.0, 10.0))
            .radius(999.0)
            .gradient(flow_gradient())
            .shadow(Shadow::new(CYAN.with_alpha((70.0 + 80.0 * s.hover) as u8), Offset::ZERO, 20.0))
            .child(
                Animated::new(if running { 1.0 } else { 0.0 })
                    .duration(std::time::Duration::from_millis(2800))
                    .curve(Curve::SINE_IN_OUT)
                    .build(|p| {
                        let label = if p >= 0.999 { "Delivered ✓" } else if p > 0.0 { "Running…" } else { "Run plan" };
                        Text::new(label).size(14.0).weight(FontWeight::Bold).color(GROUND).into()
                    }),
            )
            .into()
    })
    .on_tap(move || update(&h, |f| f.running = true));
    Container::new()
        .radius(24.0)
        .color(GLASS)
        .border(Border::new(GLASS_EDGE, 1.0))
        .padding(EdgeInsets::all(22.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(14.0)
                .children(children![
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            Flexible::expanded(1).child(
                                Flex::column()
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .spacing(4.0)
                                    .children(children![
                                        Text::new("The plan").size(20.0).weight(FontWeight::Bold).color(INK),
                                        Text::new("4 steps · touches your inbox once · about $0.004").size(13.0).color(MUTED),
                                    ])
                            ),
                            run,
                        ]),
                    Animated::new(if running { 1.0 } else { 0.0 })
                        .duration(std::time::Duration::from_millis(2800))
                        .curve(Curve::SINE_IN_OUT)
                        .build(|p| {
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .children(children![
                                    SizedBox::height(110.0).child(Painting::new(PaintWith::new(move |book, size| track(book, size, p)))),
                                    step_labels(p),
                                ])
                                .into()
                        }),
                ]),
        )
        .into()
}

/// The step names under the track, each saying where it stands: waiting,
/// working (while the flow is between it and the next node) or done.
fn step_labels(p: f32) -> WidgetNode {
    let n = STEPS.len();
    let labels: Vec<WidgetNode> = STEPS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let at = i as f32 / (n - 1) as f32;
            let (state, color) = if p >= at - 0.02 && p > 0.001 {
                ("done", MINT)
            } else if p > 0.001 && p >= at - 1.0 / (n - 1) as f32 {
                ("working", CYAN)
            } else {
                ("waiting", FAINT)
            };
            Flexible::expanded(1)
                .child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .spacing(2.0)
                        .children(children![
                            Text::new(name.to_string()).size(14.0).weight(FontWeight::Bold).color(INK),
                            Text::new(state).size(12.0).color(color),
                        ]),
                )
                .into()
        })
        .collect();
    Flex::row().children(labels).into()
}

// ── vector drawing ───────────────────────────────────────────────

/// A point on a cubic bezier.
fn bez(p0: Offset, p1: Offset, p2: Offset, p3: Offset, t: f32) -> Offset {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Offset::new(
        a * p0.dx + b * p1.dx + c * p2.dx + d * p3.dx,
        a * p0.dy + b * p1.dy + c * p2.dy + d * p3.dy,
    )
}

/// The background: three soft radial lights and two flowing ribbons. `k`
/// (0..1) leans the light toward the flow while a plan runs.
fn aurora(book: &mut Sketchbook, size: Size, k: f32) {
    let (w, h) = (size.width, size.height);
    let glow = |c: Color, a: u8| Gradient::radial(Offset::new(0.5, 0.5), 0.5).with_stops(&[(0.0, c.with_alpha(a)), (1.0, c.with_alpha(0))]);
    book.rect(Rect::new(-0.15 * w, -0.35 * h, 0.55 * w, 0.55 * h), glow(VIOLET, 120));
    book.rect(Rect::new((0.55 - 0.1 * k) * w, (-0.2 + 0.25 * k) * h, 1.2 * w, 0.75 * h), glow(CYAN, (70.0 + 50.0 * k) as u8));
    book.rect(Rect::new(0.2 * w, 0.55 * h, 0.95 * w, 1.35 * h), glow(MAGENTA, (55.0 - 20.0 * k) as u8));
    // Ribbons: long cubic curves across the room, stroked with the ramp.
    for (i, (y, amp, a)) in [(0.62_f32, 0.18_f32, 90_u8), (0.70, 0.12, 60), (0.78, 0.22, 40)].iter().enumerate() {
        let lift = (i as f32 + 1.0) * 0.03 * k;
        let mut path = Path::new();
        path.move_to(Offset::new(-0.05 * w, (y - lift) * h));
        path.cubic_to(
            Offset::new(0.3 * w, (y - amp - lift) * h),
            Offset::new(0.6 * w, (y + amp - lift) * h),
            Offset::new(1.05 * w, (y - 0.1 - lift) * h),
        );
        let ramp = Gradient::horizontal().with_stops(&[
            (0.0, VIOLET.with_alpha(0)),
            (0.35, VIOLET.with_alpha(*a)),
            (0.7, CYAN.with_alpha(*a)),
            (1.0, CYAN.with_alpha(0)),
        ]);
        book.stroke(path, ramp, 2.0 + i as f32);
    }
}

/// The plan's track: nodes joined by curves; `p` (0..1) is how far the
/// work has travelled.
fn track(book: &mut Sketchbook, size: Size, p: f32) {
    let (w, h) = (size.width, size.height);
    let n = STEPS.len();
    let xs: Vec<f32> = (0..n).map(|i| 40.0 + (w - 80.0) * i as f32 / (n - 1) as f32).collect();
    let ys = [h * 0.42, h * 0.62, h * 0.38, h * 0.58];
    let nodes: Vec<Offset> = xs.iter().zip(ys).map(|(x, y)| Offset::new(*x, y)).collect();
    // Sample the whole track once, as one polyline.
    let mut pts = Vec::new();
    for s in 0..n - 1 {
        let (a, b) = (nodes[s], nodes[s + 1]);
        let mid = (b.dx - a.dx) * 0.5;
        let (c1, c2) = (Offset::new(a.dx + mid, a.dy), Offset::new(b.dx - mid, b.dy));
        for j in 0..=40 {
            pts.push(bez(a, c1, c2, b, j as f32 / 40.0));
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
    let lit = ((pts.len() - 1) as f32 * p).round() as usize + 1;
    if p > 0.001 {
        book.stroke(line(lit), flow_gradient(), 4.0);
        let head = pts[lit - 1];
        book.circle(head, 14.0, Gradient::radial(Offset::new(0.5, 0.5), 0.5).with_stops(&[(0.0, CYAN.with_alpha(200)), (1.0, CYAN.with_alpha(0))]));
    }
    for (i, c) in nodes.iter().enumerate() {
        let at = i as f32 / (n - 1) as f32;
        let on = ((p - at) * 8.0 + 1.0).clamp(0.0, 1.0) * if p > 0.001 { 1.0 } else { 0.0 };
        if on > 0.0 {
            book.circle(*c, 18.0 + 6.0 * on, MINT.with_alpha((60.0 * on) as u8));
        }
        book.circle(*c, 11.0, GROUND.lerp(Color::rgb(30, 33, 48), 1.0 - on).lerp(MINT, on));
        book.ring(*c, 11.0, 1.5, Color::rgba(255, 255, 255, (60.0 + 140.0 * on) as u8));
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
    book.stroke(path, flow_gradient(), 2.6);
}

fn mic(book: &mut Sketchbook, s: Size) {
    let c = Offset::new(s.width / 2.0, s.height / 2.0);
    book.circle(c, s.width / 2.0, Color::rgba(255, 255, 255, 14));
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

pub fn screen() -> impl Widget {
    FlowHome
}
