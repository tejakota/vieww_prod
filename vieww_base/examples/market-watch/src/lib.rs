//! Market Watch — the screen the Valdi-vs-Vieww comparison renders.
//!
//! The same screen is written in Valdi (`compare/valdi/apps/market_watch`), with
//! the same data generator, so both frameworks draw the same pixels from the
//! same numbers. This crate holds the Vieww side: the model, the widgets and
//! the scripted load ([`Load`]) both the headless benchmark (`src/main.rs`)
//! and the Android app (`examples/android.rs`) drive.
//!
//! A ticker list: shadowed rounded cards, a circular avatar with initials, two
//! text lines, a 32-point sparkline, a price and a red/green change pill under a
//! gradient header holding the portfolio total. Each row reads its own signal,
//! so a price change rebuilds that row and nothing else.

use std::cell::Cell;
use std::rc::Rc;

use vieww_element::{Runtime, Signal};
use vieww_foundation::{Color, Constraints, EdgeInsets, Gradient, Offset, Shadow, Size};
use vieww_widget::prelude::*;
use vieww_widget::{CustomPaint, CustomPainter, DrawInstruction, ListView, Scrollable};

/// Row pitch: a 70px card plus 5px above and below.
pub const ROW_H: f32 = 80.0;
/// Sparkline length.
pub const POINTS: usize = 32;
/// Rows ticked by a "live" frame: the same three the Valdi app picks.
pub const LIVE_ROWS: usize = 3;

const BG: Color = Color::rgb(243, 245, 249);
const INK: Color = Color::rgb(20, 26, 38);
const MUTED: Color = Color::rgb(112, 122, 140);
const UP: Color = Color::rgb(22, 163, 106);
const DOWN: Color = Color::rgb(220, 64, 72);

// ---------------------------------------------------------------- data

/// One ticker. `hist` is replaced, never mutated, on a tick, so the sparkline
/// painter can answer "did I change" with a pointer compare.
#[derive(Clone, Debug)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub hue: Color,
    pub hist: Rc<Vec<f32>>,
}

impl Quote {
    #[must_use]
    pub fn price(&self) -> f32 {
        self.hist.last().copied().unwrap_or(0.0)
    }
    #[must_use]
    pub fn change(&self) -> f32 {
        let first = self.hist[0];
        (self.price() - first) / first * 100.0
    }
}

/// The generator both frameworks use: an LCG, so the TypeScript side can
/// reproduce it exactly with 32-bit integer arithmetic.
pub struct Lcg(u32);
impl Lcg {
    #[must_use]
    pub const fn new() -> Self {
        Self(0x9e37_79b9)
    }
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 % 10_000) as f32 / 10_000.0
    }
}
impl Default for Lcg {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn make_quote(i: usize, rng: &mut Lcg) -> Quote {
    const L: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut p = 20.0 + rng.next() * 400.0;
    let hist = (0..POINTS)
        .map(|_| {
            p *= 1.0 + (rng.next() - 0.48) * 0.04;
            p
        })
        .collect();
    let symbol = (0..4).map(|k| L[(i * 7 + k * 11 + i / 26) % 26] as char).collect();
    let palette = [
        Color::rgb(93, 120, 246),
        Color::rgb(239, 108, 96),
        Color::rgb(246, 170, 60),
        Color::rgb(64, 180, 150),
        Color::rgb(160, 96, 230),
    ];
    Quote {
        symbol,
        name: format!("Company {i:04} Holdings"),
        hue: palette[i % palette.len()],
        hist: Rc::new(hist),
    }
}

/// Push one new price onto a quote's history, as the Valdi app does: the
/// opposite way to the last move, 6% up or 5% down, so a price oscillates
/// instead of compounding through thousands of ticks. `index` staggers the
/// size of the move so neighbouring rows do not move in lockstep.
pub fn tick_quote(q: &mut Quote, index: usize) {
    let mut h = (*q.hist).clone();
    let n = h.len();
    let rose = h[n - 1] >= h[n - 2];
    let last = h[n - 1];
    h.remove(0);
    let size = 1.0 + (index % 3) as f32 * 0.01; // 1.00, 1.01, 1.02
    h.push(if rose { last * (0.95 / size) } else { last * (1.06 * size) });
    q.hist = Rc::new(h);
}

// --------------------------------------------------------------- model

/// Everything the screen reads, as signals.
#[derive(Clone)]
pub struct Model {
    runtime: Runtime,
    pub quotes: Signal<Rc<Vec<Signal<Quote>>>>,
    pub total: Signal<f32>,
    pub offset: Signal<f32>,
    ticks: Rc<Cell<usize>>,
}

impl Model {
    #[must_use]
    pub fn new(runtime: &Runtime, rows: usize) -> Self {
        let model = Self {
            runtime: runtime.clone(),
            quotes: runtime.signal(Rc::new(Vec::new())),
            total: runtime.signal(0.0),
            offset: runtime.signal(0.0),
            ticks: Rc::new(Cell::new(0)),
        };
        model.load(rows);
        model
    }

    /// Replace the whole list with `rows` fresh quotes, back at the top.
    pub fn load(&self, rows: usize) {
        let mut rng = Lcg::new();
        let quotes: Vec<Signal<Quote>> =
            (0..rows).map(|i| self.runtime.signal(make_quote(i, &mut rng))).collect();
        self.total.set(quotes.iter().map(|q| q.with(Quote::price)).sum());
        self.quotes.set(Rc::new(quotes));
        self.offset.set(0.0);
        self.ticks.set(0);
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.quotes.with(|q| q.len())
    }

    /// Rows currently on screen, for the "storm" load that ticks all of them.
    #[must_use]
    pub fn visible(&self, viewport: f32) -> std::ops::Range<usize> {
        let first = (self.offset.get() / ROW_H).floor().max(0.0) as usize;
        let count = (viewport / ROW_H).ceil() as usize + 1;
        first..(first + count).min(self.rows())
    }

    /// Tick `rows` (indices wrap) and update the header total. The header
    /// total is the running sum adjusted by each change, as in the Valdi app,
    /// so a tick costs the rows it touches and not a pass over the list.
    pub fn tick(&self, rows: impl IntoIterator<Item = usize>) {
        let quotes = self.quotes.get();
        let n = quotes.len();
        if n == 0 {
            return;
        }
        let mut delta = 0.0;
        for i in rows {
            let i = i % n;
            quotes[i].update(|q| {
                let before = q.price();
                tick_quote(q, i);
                delta += q.price() - before;
            });
        }
        self.total.update(|t| *t += delta);
        self.ticks.set(self.ticks.get() + 1);
    }

    /// The three rows a "live" tick touches: they walk down the list, three
    /// apart, as the Valdi app's do. Starts inside the visible window.
    pub fn tick_live(&self, viewport: f32) {
        let first = self.visible(viewport).start;
        let k = self.ticks.get() * LIVE_ROWS;
        self.tick((0..LIVE_ROWS).map(|j| first + 1 + (k + j * 3) % 9));
    }

    /// Every visible row plus the header.
    pub fn tick_storm(&self, viewport: f32) {
        self.tick(self.visible(viewport));
    }

    pub fn scroll_by(&self, px: f32) {
        let max = (self.rows() as f32 * ROW_H - 1.0).max(0.0);
        self.offset.update(|o| *o = (*o + px).clamp(0.0, max));
    }
}

// ---------------------------------------------------------- sparkline

struct Spark {
    hist: Rc<Vec<f32>>,
    color: Color,
}

impl CustomPainter for Spark {
    fn paint(&self, size: Size) -> Vec<DrawInstruction> {
        let (lo, hi) = self
            .hist
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let span = (hi - lo).max(1e-3);
        let step = size.width / (self.hist.len() - 1) as f32;
        let pt = |i: usize| {
            Offset::new(
                i as f32 * step,
                size.height - (self.hist[i] - lo) / span * (size.height - 2.0) - 1.0,
            )
        };
        (1..self.hist.len())
            .map(|i| DrawInstruction::DrawLine { from: pt(i - 1), to: pt(i), color: self.color, width: 1.6 })
            .collect()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn should_repaint(&self, prev: &dyn CustomPainter) -> bool {
        prev.as_any()
            .downcast_ref::<Spark>()
            .is_none_or(|p| !Rc::ptr_eq(&p.hist, &self.hist))
    }
}

// --------------------------------------------------------------- row

#[derive(Debug)]
struct Row {
    quote: Signal<Quote>,
    width: f32,
}

impl Widget for Row {
    fn debug_name(&self) -> &'static str {
        "Row"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let q = self.quote.get(); // subscribes this row, and only this row
        let ch = q.change();
        let tone = if ch >= 0.0 { UP } else { DOWN };
        // The middle column takes what the fixed parts leave, so the row fits
        // a 360dp phone as well as the 720px headless frame.
        let fixed = 28.0 + 40.0 + 120.0 + 120.0 + 3.0 * 12.0;
        let names_w = (self.width - fixed).max(60.0);
        let avatar = Clip::rounded(20.0).child(Container::new().color(q.hue).child(
            SizedBox::square(40.0).child(
                Center::new().child(Text::new(&q.symbol[..2]).color(Color::WHITE).size(14.0).bold()),
            ),
        ));
        let names = SizedBox::from_size(Size::new(names_w, 40.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(2.0)
                .children(children![
                    Text::new(&q.symbol).color(INK).size(16.0).bold(),
                    Text::new(&q.name).color(MUTED).size(12.0),
                ]),
        );
        let spark = CustomPaint::sized(Size::new(120.0, 36.0), Spark { hist: q.hist.clone(), color: tone });
        let pill = Container::new()
            .decoration(BoxDecoration::new().color(tone).radius(8.0))
            .padding(EdgeInsets::symmetric(8.0, 3.0))
            .child(Text::new(format!("{ch:+.2}%")).color(Color::WHITE).size(12.0));
        let money = SizedBox::from_size(Size::new(120.0, 46.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::End)
                .spacing(4.0)
                .children(children![
                    Text::new(format!("${:.2}", q.price())).color(INK).size(16.0).bold(),
                    pill,
                ]),
        );
        Padding::new(EdgeInsets::symmetric(0.0, 5.0))
            .child(
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .color(Color::WHITE)
                            .radius(14.0)
                            .shadow(Shadow::new(Color::rgba(20, 26, 38, 30), Offset::new(0.0, 3.0), 8.0)),
                    )
                    .padding(EdgeInsets::symmetric(14.0, 12.0))
                    .child(
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .spacing(12.0)
                            .children(children![avatar, names, spark, money]),
                    ),
            )
            .into()
    }
}
vieww::widget::widget_node_from!(Row);

// ------------------------------------------------------------ header

#[derive(Debug)]
struct Header {
    total: Signal<f32>,
    rows: usize,
}
impl Widget for Header {
    fn debug_name(&self) -> &'static str {
        "Header"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let soft = Color::rgba(255, 255, 255, 190);
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .gradient(
                        Gradient::vertical().with_dither().between(Color::rgb(58, 92, 246), Color::rgb(140, 72, 230)),
                    )
                    .radius(20.0),
            )
            .padding(EdgeInsets::all(22.0))
            .child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .spacing(6.0)
                    .children(children![
                        Text::new("Portfolio value").color(soft).size(13.0),
                        Text::new(format!("${:.2}", self.total.get())).color(Color::WHITE).size(32.0).bold(),
                        Text::new(format!("{} live tickers · virtualised", self.rows)).color(soft).size(12.0),
                    ]),
            )
            .into()
    }
}
vieww::widget::widget_node_from!(Header);

// ------------------------------------------------------------ screen

/// Header plus list.
///
/// `size` is the logical window; the list takes what the header leaves.
#[derive(Debug)]
pub struct Screen {
    model_quotes: Signal<Rc<Vec<Signal<Quote>>>>,
    total: Signal<f32>,
    offset: Signal<f32>,
    size: Size,
}

/// Height of the header plus its gap and the page padding: what the list loses.
pub const CHROME_H: f32 = 170.0;

impl Screen {
    #[must_use]
    pub fn new(model: &Model, size: Size) -> Self {
        Self { model_quotes: model.quotes.clone(), total: model.total.clone(), offset: model.offset.clone(), size }
    }
    /// The list's viewport height, for [`Model::visible`].
    #[must_use]
    pub fn viewport(size: Size) -> f32 {
        (size.height - CHROME_H - 20.0).max(ROW_H)
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model").field("rows", &self.rows()).finish()
    }
}

impl Widget for Screen {
    fn debug_name(&self) -> &'static str {
        "Screen"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let quotes = self.model_quotes.get();
        let rows = quotes.len();
        let width = self.size.width - 40.0;
        let list = Constrained::new(Constraints::tight(Size::new(width, Self::viewport(self.size)))).child(
            Scrollable::vertical(self.offset.get()).child(ListView::new(
                rows,
                ROW_H,
                Rc::new(move |i: usize| {
                    RepaintBoundary::new().child(Row { quote: quotes[i].clone(), width }).into()
                }),
            )),
        );
        Container::new()
            .color(BG)
            .padding(EdgeInsets::all(20.0))
            .child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(16.0)
                    .children(children![Header { total: self.total.clone(), rows }, list]),
            )
            .into()
    }
}
vieww::widget::widget_node_from!(Screen);

// ---------------------------------------------------------- scripted load

/// One phase of the comparison schedule. The Valdi app runs the same list, in
/// the same order, with the same names; the device runner keys every
/// measurement on these names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// Replace the list with `n` rows and draw it.
    Load(usize),
    /// Nothing changes. A framework that draws here is wasting the battery.
    Idle,
    /// One tick of three rows and the header.
    Tick,
    /// Jump 4,000px down the list.
    Jump,
    /// Scroll 8px a frame, nothing ticking.
    Scroll,
    /// Scroll 8px a frame and tick three rows a frame: a live market feed.
    Live,
    /// Tick every visible row and the header every frame, no scroll.
    Storm,
}

/// Seconds each continuous phase runs, and the long thermal pass.
pub const PHASE_SECONDS: f32 = 8.0;
pub const THERMAL_SECONDS: f32 = 60.0;
/// Pixels a scrolling frame moves.
pub const SCROLL_STEP: f32 = 8.0;

/// The schedule: (name, load, seconds). Both apps print `__MW__ phase=<name>`
/// at each boundary, so these names are the protocol.
#[must_use]
pub fn schedule(thermal: bool) -> Vec<(String, Load, f32)> {
    let mut s = Vec::new();
    for rows in [2_000, 20_000] {
        let k = rows / 1000;
        s.push((format!("load{k}k"), Load::Load(rows), 2.0));
        s.push((format!("idle{k}k"), Load::Idle, 2.0));
        s.push((format!("tick{k}k"), Load::Tick, 1.0));
        s.push((format!("jump{k}k"), Load::Jump, 1.0));
        s.push((format!("scroll{k}k"), Load::Scroll, PHASE_SECONDS));
        s.push((format!("live{k}k"), Load::Live, PHASE_SECONDS));
        s.push((format!("storm{k}k"), Load::Storm, PHASE_SECONDS));
    }
    if thermal {
        s.push(("load2k-thermal".into(), Load::Load(2_000), 2.0));
        s.push(("thermal".into(), Load::Live, THERMAL_SECONDS));
    }
    s
}

impl Load {
    /// Whether the phase changes something every frame.
    #[must_use]
    pub const fn continuous(self) -> bool {
        matches!(self, Self::Scroll | Self::Live | Self::Storm)
    }

    /// Apply the phase's one-off change (on entry), if it has one.
    pub fn enter(self, model: &Model, viewport: f32) {
        match self {
            Self::Load(n) => model.load(n),
            Self::Tick => model.tick_live(viewport),
            Self::Jump => model.scroll_by(4_000.0),
            _ => {}
        }
    }

    /// Apply one frame's change, for a continuous phase.
    pub fn frame(self, model: &Model, viewport: f32) {
        match self {
            Self::Scroll => model.scroll_by(SCROLL_STEP),
            Self::Live => {
                model.scroll_by(SCROLL_STEP);
                model.tick_live(viewport);
            }
            Self::Storm => model.tick_storm(viewport),
            _ => {}
        }
    }
}

// ------------------------------------------------------------- stats

/// Median, p95, p99 and max of a sample, in the sample's unit.
#[must_use]
pub fn summary(samples: &[f64]) -> (f64, f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    (at(0.5), at(0.95), at(0.99), v[v.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Valdi app computes these same quotes in TypeScript (f64, `Math.imul`);
    /// Vieww in Rust (f32). Symbols must match exactly and prices to the cent,
    /// or the two apps are not drawing the same screen. Expected values come from
    /// running the TypeScript generator under Node.
    #[test]
    fn quotes_match_the_valdi_generator() {
        let mut rng = Lcg::new();
        let q: Vec<Quote> = (0..2000).map(|i| make_quote(i, &mut rng)).collect();
        for (i, symbol, price) in [(0, "ALWH", 135.65), (1, "HSDO", 268.20), (999, "LWHS", 348.06), (1999, "DOZK", 33.36)] {
            assert_eq!(q[i].symbol, symbol, "row {i} symbol");
            assert!((q[i].price() - price).abs() < 0.006, "row {i}: {} vs Valdi's {price}", q[i].price());
        }
    }
}
