//! `U` — an offline persona companion.
//!
//! Two screens: **home**, which opens directly, and **settings**, reached by
//! the gear at the top right and left by the back button. Home is one
//! decision, presented whole: a question, two answers, a way to pass, and
//! the persona's cell breathing while it waits. Answering *is* training —
//! the session count and the burst of the field are the whole of what the
//! prototype records. Settings is the form that shapes the persona: a name,
//! a voice, a decision style, a training rhythm, and the privacy posture.
//!
//! # What is real and what is painted
//!
//! This is a prototype: every value the form edits lives in a signal, every
//! control is wired, and answering a question really does count a session
//! and fire the field — but the choice is shown back and then dropped:
//! nothing is persisted and no persona model exists yet. The screens are
//! the deliverable; the event-sourced core they will front is the next
//! step.
//!
//! # The shape of the state
//!
//! Everything the two screens share lives in [`AppState`], created once at
//! startup and cloned into whatever wants it — a signal is a handle, so a
//! clone is a reference, not a copy. Routes are *builders* rather than trees
//! for the reason `Navigator`'s own documentation gives: a stored node would
//! be frozen at the moment it was pushed. The builders upgrade a shared slot,
//! because the home route must exist before the state that contains the
//! navigator does — the one cycle in this design, broken by indirection.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use vieww::animation::Ticker;
use vieww::prelude::*;
use vieww::{FrameDriver, NavigatorController, ScrollController};
use vieww_platform_winit::App;
use vieww_widget::RouteTransition;

#[cfg(target_os = "android")]
use vieww_platform_winit::AndroidApp;

pub mod ask;
pub mod home;
pub mod neurons;
pub mod palette;
pub mod settings;

use home::HomeScreen;
use settings::SettingsScreen;

/// The phone-shaped surface the app is composed for.
pub const SURFACE: Size = Size::new(420.0, 880.0);

/// The state slot every route builder upgrades. `Rc<RefCell<Option<..>>>`
/// rather than `Rc<AppState>` because the navigator — inside the state — must
/// exist before the state does, and something has to hold the seam.
pub type Shared = Rc<RefCell<Option<AppState>>>;

/// Everything both screens share.
#[derive(Clone)]
pub struct AppState {
    /// The two-screen stack. Home is the root; settings is pushed onto it.
    pub nav: NavigatorController,
    /// The settings screen's scroll, with a life of its own so a fling
    /// survives the screen being covered and returned to.
    pub scroll: ScrollController,
    /// The clock, published as a signal so the neuron field can be a pure
    /// function of time. Held here because the frame driver's registration
    /// is weak — somebody must own the clock, and the tree holds the widget,
    /// the widget holds the state, the state keeps it ticking.
    pub clock: Rc<RefCell<Clock>>,
    /// The training afterglow, decaying to zero on its own ticker.
    pub decay: Rc<RefCell<Decay>>,
    /// Seconds since the app opened. Reading it subscribes; the clock ticker
    /// writes it every frame.
    pub time: Signal<f32>,
    /// `0..1`, set by a training session and eased back to `0` by `decay`.
    pub burst: Signal<f32>,
    /// What U should call you.
    pub name: Signal<TextEditingValue>,
    /// 0 direct · 1 warm · 2 analytical.
    pub tone: Signal<usize>,
    /// 0 fast and intuitive · 1 deliberate and thorough.
    pub style: Signal<usize>,
    /// Training sessions per day, `1..=12`.
    pub daily: Signal<f32>,
    /// A pulse when a memory lands.
    pub haptics: Signal<bool>,
    /// Dim the neuron field — the app's own reduced-motion switch.
    pub calm: Signal<bool>,
    /// Require a fingerprint to open U.
    pub biometric: Signal<bool>,
    /// Training sessions completed today.
    pub sessions: Signal<u32>,
    /// The question on the card, as an index into [`ask::ASKS`].
    pub ask: Signal<usize>,
    /// The last answer U recorded, shown back until the next one replaces
    /// it. `None` until a first answer lands — nothing was noted before
    /// then, and pretending otherwise is the one lie this screen refuses.
    pub noted: Signal<Option<&'static str>>,
}

/// Hand-written: the state is handles all the way down, and "Shared(0x…)" is
/// the honest summary of it.
impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    /// One training session: count it, and fire the field.
    pub fn train(&self) {
        self.sessions.update(|n| *n += 1);
        self.burst.set(1.0);
    }

    /// Answer the question on the card: note the choice back, count the
    /// session, fire the field, bring up the next question.
    pub fn answer(&self, choice: bool) {
        let ask = &ask::ASKS[self.ask.peek() % ask::ASKS.len()];
        self.noted.set(Some(if choice { ask.a } else { ask.b }));
        self.train();
        self.advance();
    }

    /// Pass on the question: nothing noted, nothing counted.
    pub fn skip(&self) {
        self.noted.set(None);
        self.advance();
    }

    /// The next question, around the bank.
    fn advance(&self) {
        self.ask.update(|i| *i = (*i + 1) % ask::ASKS.len());
    }

    /// What U calls you, or "you" until told otherwise.
    #[must_use]
    pub fn display_name(&self) -> String {
        let name = self.name.peek().text.trim().to_owned();
        if name.is_empty() {
            "you".to_owned()
        } else {
            name
        }
    }
}

/// Publishes the wall of time the animations read, at half the display's
/// rate.
///
/// Everything the clock drives — the field, the breathing cell, the gear —
/// repaints whatever it sits on, and on a phone that paint is the app's
/// whole cost. Half rate halves it, and at the speeds these move nothing
/// reads as a step. Press feedback and route transitions do not read the
/// clock and keep the display's full rate; so does the burst decay, which
/// is the answer's own feedback and only lasts moments.
#[derive(Debug)]
pub struct Clock {
    pub time: Signal<f32>,
    last: f32,
}

impl Ticker for Clock {
    fn tick(&mut self, now: Duration) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let t = now.as_secs_f32();
        let stepped = (t * 30.0).floor() / 30.0;
        if stepped != self.last {
            self.last = stepped;
            self.time.set(stepped);
        }
        true
    }

    fn is_animating(&self) -> bool {
        // The field is alive; a still U would be a dead one.
        true
    }
}

/// Eases the training afterglow back to rest.
#[derive(Debug)]
pub struct Decay {
    pub burst: Signal<f32>,
    last: Option<Duration>,
}

impl Decay {
    fn new(burst: Signal<f32>) -> Self {
        Self { burst, last: None }
    }
}

impl Ticker for Decay {
    fn tick(&mut self, now: Duration) -> bool {
        let dt = self
            .last
            .map_or(1.0 / 60.0, |was| (now - was).as_secs_f32().clamp(0.0, 0.1));
        self.last = Some(now);
        let current = self.burst.peek();
        let next = (current - dt * 0.55).max(0.0);
        if next != current {
            self.burst.set(next);
        }
        next > 0.0
    }

    fn is_animating(&self) -> bool {
        self.burst.peek() > 0.0
    }
}

/// Mount the app onto a driver: create the state, wire the routes, attach the
/// tickers. Returns the shared slot — for `root`, and for the screenshot tool.
#[must_use]
pub fn mount(driver: &mut FrameDriver) -> Shared {
    let runtime = driver.elements().runtime().clone();

    let time = runtime.signal(0.0_f32);
    let burst = runtime.signal(0.0_f32);
    let clock = Rc::new(RefCell::new(Clock {
        time: time.clone(),
        last: -1.0,
    }));
    let decay = Rc::new(RefCell::new(Decay::new(burst.clone())));

    let slot: Shared = Rc::new(RefCell::new(None));

    let nav = NavigatorController::new(&runtime, home_route(slot.clone()));
    let scroll = ScrollController::new(&runtime, ScrollPhysics::android());

    let state = AppState {
        nav,
        scroll,
        clock,
        decay,
        time,
        burst,
        name: runtime.signal(TextEditingValue::new("Teja")),
        tone: runtime.signal(1usize),
        style: runtime.signal(0usize),
        daily: runtime.signal(4.0_f32),
        haptics: runtime.signal(true),
        calm: runtime.signal(false),
        biometric: runtime.signal(true),
        sessions: runtime.signal(0u32),
        ask: runtime.signal(0usize),
        noted: runtime.signal(None::<&'static str>),
    };

    // The clock and the decay are weakly held by the driver; the state is the
    // strong owner that keeps them alive for as long as the app runs.
    driver.tickers().add(&state.clock);
    driver.tickers().add(&state.decay);
    state.nav.attach(driver.tickers());
    state.scroll.attach(driver.tickers());

    *slot.borrow_mut() = Some(state);
    slot
}

/// A route builder's first move: the state, or a panic that names the bug.
#[must_use]
pub fn take(shared: &Shared) -> AppState {
    shared
        .borrow()
        .as_ref()
        .expect("U's state is mounted before any route can build")
        .clone()
}

/// The root widget: the navigator, handed the current stack.
#[must_use]
pub fn root(shared: &Shared) -> WidgetNode {
    UShell {
        shared: shared.clone(),
    }
    .into()
}

/// Home — the screen the app opens on.
#[must_use]
pub fn home_route(shared: Shared) -> Route {
    Route::new(
        "home",
        Rc::new(move |_| {
            HomeScreen {
                state: take(&shared),
                shared: shared.clone(),
            }
            .into()
        }),
    )
}

/// Settings — pushed over home, and popped back to it.
///
/// The transition is a zoom from the centre with a fade and a slight rise —
/// a thought surfacing, rather than a page sliding. Both the movement and the
/// fade are paint-time, so the flight costs no layout.
#[must_use]
pub fn settings_route(shared: Shared) -> Route {
    Route::new(
        "settings",
        Rc::new(move |_| {
            SettingsScreen {
                state: take(&shared),
            }
            .into()
        }),
    )
    .transition(RouteTransition::custom(|screen, t, surface| {
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        let scale = 0.92 + 0.08 * eased;
        let rise = (1.0 - eased) * 26.0;
        Opacity::new(t)
            .child(
                Transformed::translate(Offset::new(0.0, rise)).child(
                    Transformed::new(Transform::scale_around(
                        Offset::new(surface.width * 0.5, surface.height * 0.42),
                        scale,
                        scale,
                    ))
                    .child(screen),
                ),
            )
            .into()
    }))
}

#[derive(Debug)]
struct UShell {
    shared: Shared,
}

#[widget]
impl UShell {
    fn build(&self, _ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = take(&self.shared);
        WidgetNode::new(
            Navigator::new(state.nav.routes())
                .leaving(state.nav.leaving())
                .surface(SURFACE)
                .key("u-navigator"),
        )
    }
}

/// The desktop entry. Android's is `android_main`, below.
///
/// ```console
/// cargo run -p u --release
/// ```
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let report = App::new()
        .title("U")
        .size(SURFACE)
        .theme(palette::theme())
        .run(|driver| {
            let shared = mount(driver);
            driver.set_root(root(&shared));
        })?;
    println!("{report}");
    Ok(())
}

/// Called by android-activity's glue once the activity exists. Plain Rust ABI
/// rather than `extern "C"`, for the reason `examples/android.rs` gives: the
/// glue looks the symbol up by name and calls it as Rust, and `AndroidApp` is
/// not `repr(C)`.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("U"),
    );
    let result = App::new()
        .title("U")
        .size(SURFACE)
        .theme(palette::theme())
        .run_android(android, |driver| {
            let shared = mount(driver);
            driver.set_root(root(&shared));
        });
    if let Err(error) = result {
        // A phone discards stderr; this is the only way the reason for an
        // activity that opened and vanished reaches anybody.
        log::error!("U: {error}");
    }
}
