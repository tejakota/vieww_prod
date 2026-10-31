//! `U` — an offline decision companion.
//!
//! Three rooms and a sheet: **home**, which the app opens on through a
//! staged splash; **settings**, reached by the gear at the top right or a
//! right-to-left swipe; the **decision workspace**, the room home's own
//! action opens, where a person brings any situation and reads back the
//! assessment; and the **ask sheet**, the one question, pushed by home's
//! domains, its recent strip or its nudge. Home is the persona's room — the
//! companion at its centre — and everything on it is either who lives
//! there or a way to bring U a decision. The asking has two phases over an
//! event log: the **first sketch** is the first twelve answers, completed
//! once and never repeated, and the **daily rhythm** after it is one
//! question at a time against the day's target, rotating for variety and
//! counted by nothing but the day. Settings is the form that shapes the
//! persona: a name, a voice, a decision style, a training rhythm, and the
//! privacy posture.
//!
//! # What is real and what is painted
//!
//! Two memories, each as real as the other. The **training and settings
//! memory** is the event log: every answer, pass and setting is an
//! [`events::Event`] appended *before* it is applied, and the state at
//! startup is a fold of that log — which is why the sketch is completed
//! once ever rather than once per process. The **decision memory** is the
//! engine ([`engine`]): every situation brought, every assessment written
//! back, and every verdict on one, kept by `u-memory` in the app's private
//! directory as records the screens never rewrite behind. The persona the
//! settings form edits by hand and the persona the engine reads hints
//! from are one file, saved by the same engine that learns into it.
//!
//! # The shape of the state
//!
//! Everything the rooms share lives in [`AppState`], created once at
//! startup and cloned into whatever wants it — a signal is a handle, so a
//! clone is a reference, not a copy. Routes are *builders* rather than trees
//! for the reason `Navigator`'s own documentation gives: a stored node would
//! be frozen at the moment it was pushed. The builders upgrade a shared slot,
//! because the home route must exist before the state that contains the
//! navigator does — the one cycle in this design, broken by indirection.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use vieww::animation::Ticker;
use vieww::prelude::*;
use vieww::{FrameDriver, NavigatorController, ScrollController};
#[cfg(target_os = "android")]
use vieww_platform_winit::AndroidApp;
use vieww_platform_winit::App;
use vieww_text::FontStore;
use vieww_widget::RouteTransition;

pub mod ask;
pub mod controls;
pub mod engine;
pub mod events;
pub mod home;
pub mod neurons;
pub mod palette;
pub mod settings;
pub mod sheet;
pub mod stage;
pub mod workspace;

use home::HomeScreen;
use settings::SettingsScreen;
use sheet::AskSheet;
use workspace::WorkspaceScreen;

/// The phone-shaped surface the app is composed for — the reference's own
/// canvas, 420 × 920, which the [`stage`] scales onto whatever screen it is
/// given.
pub const SURFACE: Size = Size::new(420.0, 920.0);

/// The state slot every route builder upgrades. `Rc<RefCell<Option<..>>>`
/// rather than `Rc<AppState>` because the navigator — inside the state — must
/// exist before the state does, and something has to hold the seam.
pub type Shared = Rc<RefCell<Option<AppState>>>;

/// The typeface the reference screens are set in: **Inter**, in the three
/// weights the app draws with, embedded so the app carries its own face onto
/// every device it installs on.
///
/// The reference asks for weights 400 to 730; the framework's type scale
/// names Regular, Medium and Bold, and those three cover every line the
/// screens draw. Embedded rather than resolved from the system, because the
/// system's face is Roboto on a phone and DejaVu on a desktop and neither is
/// Inter — and the app is an exact clone of drawings set in Inter.
///
/// `with_application_faces` puts these ahead of both the embedded and any
/// system face for every tie, and points the generic `sans-serif` at Inter,
/// so no style in the tree has to name a family to get it. The faces are
/// ~975 KB together — three trips for a webpage, and one mmap into a process
/// that stays under 3 MB of heap on a low-RAM phone.
#[must_use]
pub fn fonts() -> FontStore {
    const REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
    const MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
    FontStore::with_application_faces(
        [REGULAR.to_vec(), MEDIUM.to_vec(), BOLD.to_vec()],
        Some("Inter"),
        None,
    )
}

/// Everything both screens share.
#[derive(Clone)]
pub struct AppState {
    /// The two-screen stack. Home is the root; settings is pushed onto it.
    pub nav: NavigatorController,
    /// The settings screen's scroll, with a life of its own so a fling
    /// survives the screen being covered and returned to.
    pub scroll: ScrollController,
    /// The workspace's scroll — a second life, so a fling in the decision
    /// workspace survives the sheet opening over it.
    pub work: ScrollController,
    /// The decision engine: memory, persona, runtime — the plan's `u-app`
    /// orchestration, held behind a `RefCell` because a decision is one
    /// synchronous call and nothing else ever borrows it mid-build.
    pub engine: Rc<RefCell<engine::Engine>>,
    /// The clock, published as a signal so the neuron field can be a pure
    /// function of time. Held here because the frame driver's registration
    /// is weak — somebody must own the clock, and the tree holds the widget,
    /// the widget holds the state, the state keeps it ticking.
    pub clock: Rc<RefCell<Clock>>,
    /// The training afterglow, decaying to zero on its own ticker.
    pub decay: Rc<RefCell<Decay>>,
    /// The append-only log every change is written to before it is applied.
    /// `Rc` so the clones a route builder takes share the one log.
    pub log: Rc<events::Log>,
    /// The UTC day the day-counter belongs to. `Rc<Cell<..>>` for the same
    /// reason: a plain field would copy, and copies would disagree.
    pub day: Rc<Cell<u64>>,
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
    /// Training sessions today — the sketch's answers and the day's own, reset
    /// by the first event of a new UTC day.
    pub sessions: Signal<u32>,
    /// Answers recorded, ever — the sketch is the first [`ask::ASKS.len()`]
    /// of them, and this number is the only thing that decides which phase
    /// home is in.
    pub answers: Signal<u32>,
    /// Whether the sketch-complete moment has been seen. It shows once, ever;
    /// the log is what makes "ever" true.
    pub seen_sketch: Signal<bool>,
    /// The question on the card, as an index into [`ask::ASKS`].
    pub ask: Signal<usize>,
    /// The last answer U recorded, shown back until the next one replaces
    /// it. `None` until a first answer lands — nothing was noted before
    /// then, and pretending otherwise is the one lie this screen refuses.
    pub noted: Signal<Option<&'static str>>,
    /// Whether home's nudge is showing. View state, deliberately not an
    /// event: declining a question for now is not a fact about a person,
    /// and the nudge is entitled to return the next time the app opens.
    pub nudge: Signal<bool>,
    /// The situation being brought to the decision workspace.
    pub situation: Signal<TextEditingValue>,
    /// The options the person is considering, one per line.
    pub choices: Signal<TextEditingValue>,
    /// What matters most, most important first.
    pub wants: Signal<TextEditingValue>,
    /// The constraints they named.
    pub bounds: Signal<TextEditingValue>,
    /// The answer to the one question the last assessment asked.
    pub follow_up: Signal<TextEditingValue>,
    /// The last assessment, shown in the workspace.
    pub assessment: Signal<Option<Rc<u_decision::Assessment>>>,
    /// Why the last assess refused, when it refused.
    pub assess_error: Signal<Option<String>>,
    /// Whether the person gave their verdict on the last assessment.
    pub feedback_given: Signal<bool>,
    /// How many decisions the memory holds — home's recent strip reads it,
    /// so the strip is honest the moment a decision lands.
    pub decisions: Signal<u32>,
    /// The moment the opening was skipped at, if it was. `INFINITY` when it
    /// plays out in full; a tap while it plays resolves it at once, and
    /// `calm` resolves it before it begins. A f32 rather than a bool so the
    /// fade that follows a skip can start from the moment of the tap.
    pub skip_at: Signal<f32>,
}

/// Hand-written: the state is handles all the way down, and "Shared(0x…)" is
/// the honest summary of it.
impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    /// Answer the question on the card: append the fact, fold it in — which
    /// counts the session, notes the choice back, and brings up the next
    /// question — then fire the field, because a choice landing is the one
    /// thing this screen celebrates.
    pub fn answer(&self, choice: bool) {
        let ask = self.ask.peek() % ask::ASKS.len();
        self.commit(events::Event::Answered {
            day: events::today(),
            ask,
            choice,
        });
        self.burst.set(1.0);
    }

    /// Pass on the question: noted nothing, counted nothing, next question.
    pub fn skip(&self) {
        let ask = self.ask.peek() % ask::ASKS.len();
        self.commit(events::Event::Skipped {
            day: events::today(),
            ask,
        });
    }

    /// Dismiss the sketch-complete moment: it will not show again.
    pub fn see_sketch(&self) {
        self.commit(events::Event::SketchSeen);
    }

    /// Dismiss home's nudge for this session. Not an event — see [`nudge`].
    pub fn dismiss_nudge(&self) {
        self.nudge.set(false);
    }

    /// Point the next question at a domain: the next ask whose tag is among
    /// `tags`, rotating from where the bank stands. Choosing *which* question
    /// to show is a view decision, not a fact — the answer, when it lands, is
    /// what the log records, and it records the ask it was actually asked.
    pub fn ask_in(&self, tags: &[&'static str]) {
        let n = ask::ASKS.len();
        let start = self.ask.peek() % n;
        for step in 1..=n {
            let i = (start + step) % n;
            if tags.contains(&ask::ASKS[i].tag) {
                self.ask.set(i);
                return;
            }
        }
    }

    /// Append the event, then fold it in — the log first, because a state
    /// that has moved without a record of why is the one lie this app
    /// refuses.
    fn commit(&self, event: events::Event) {
        self.log.append(&event);
        self.apply(&event);
    }

    /// The fold: state as a function of one event. This runs at startup over
    /// the whole log and again on every change, which is what keeps the two
    /// paths from disagreeing. It is also the whole of the prototype's
    /// "model", and the seam where a real one goes.
    fn apply(&self, event: &events::Event) {
        use events::Event;
        match event {
            Event::Answered { day, ask, choice } => {
                self.rollover(*day);
                self.answers.update(|n| *n += 1);
                self.sessions.update(|n| *n += 1);
                let question = &ask::ASKS[*ask % ask::ASKS.len()];
                self.noted
                    .set(Some(if *choice { question.a } else { question.b }));
                self.ask.set((*ask + 1) % ask::ASKS.len());
            }
            Event::Skipped { day, ask } => {
                self.rollover(*day);
                self.noted.set(None);
                self.ask.set((*ask + 1) % ask::ASKS.len());
            }
            Event::SketchSeen => self.seen_sketch.set(true),
            Event::Named(name) => self.name.set(TextEditingValue::new(name.clone())),
            Event::Toned(voice) => self.tone.set(*voice),
            Event::Styled(style) => self.style.set(*style),
            Event::Daily(daily) => self.daily.set(*daily),
            Event::Haptics(on) => self.haptics.set(*on),
            Event::Calm(on) => self.calm.set(*on),
            Event::Biometric(on) => self.biometric.set(*on),
        }
    }

    /// A new day's first event resets the day's counter.
    fn rollover(&self, day: u64) {
        if self.day.get() != day {
            self.day.set(day);
            self.sessions.set(0);
        }
    }

    /// The settings screen's writes. Each is an event, so the form restores
    /// through the same fold the answers do.
    pub fn set_name(&self, next: TextEditingValue) {
        self.commit(events::Event::Named(next.text));
    }

    /// Choose the voice: 0 direct, 1 warm, 2 analytical.
    pub fn set_tone(&self, voice: usize) {
        self.commit(events::Event::Toned(voice));
    }

    /// Choose the decision style.
    pub fn set_style(&self, style: usize) {
        self.commit(events::Event::Styled(style));
    }

    /// Choose the training rhythm, in sessions per day.
    pub fn set_daily(&self, daily: f32) {
        self.commit(events::Event::Daily(daily));
    }

    /// Turn the landing pulse on or off.
    pub fn set_haptics(&self, on: bool) {
        self.commit(events::Event::Haptics(on));
    }

    /// Dim or brighten the neuron field.
    pub fn set_calm(&self, on: bool) {
        self.commit(events::Event::Calm(on));
    }

    /// Require or drop the fingerprint gate.
    pub fn set_biometric(&self, on: bool) {
        self.commit(events::Event::Biometric(on));
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

    // ---- the decision workspace ------------------------------------------

    /// Bring the decision: read the fields, run the engine, keep the record
    /// of what the person actually typed — never a normalised replacement.
    ///
    /// The workspace's fields are deliberately not events in the training
    /// log: they are the new decision memory's own records, written by the
    /// engine, and duplicating them into the log would be two histories
    /// disagreeing about one life.
    pub fn assess(&self) {
        let input = u_decision::Input {
            situation: self.situation.peek().text.clone(),
            options: lines_of(&self.choices.peek().text),
            matters: lines_of(&self.wants.peek().text),
            constraints: lines_of(&self.bounds.peek().text),
            notes: String::new(),
        };
        let mut engine = self.engine.borrow_mut();
        match engine.decide(&input) {
            Ok(assessment) => {
                self.assessment.set(Some(Rc::new(assessment)));
                self.assess_error.set(None);
                self.decisions.update(|n| *n += 1);
            }
            Err(error) => {
                self.assess_error.set(Some(error.to_string()));
            }
        }
    }

    /// Fold the answer to the one question into the constraints and re-read
    /// the whole thing — §2.5's loop: one question, answered, never re-asked.
    pub fn fold_answer(&self) {
        let answer = self.follow_up.peek().text.trim().to_owned();
        if answer.is_empty() {
            return;
        }
        let mut bounds = self.bounds.peek().text.clone();
        if !bounds.is_empty() && !bounds.ends_with('\n') {
            bounds.push('\n');
        }
        bounds.push_str(&answer);
        self.bounds.set(TextEditingValue::new(bounds));
        self.follow_up.set(TextEditingValue::new(String::new()));
        self.assess();
    }

    /// Abandon the decision in flight — §5.6's "cancel work when requested
    /// or when the user leaves". Called on the way out of the workspace,
    /// whatever screen is going on next: the baseline has nothing to
    /// cancel, and a model stops between tokens.
    pub fn cancel_assessment(&self) {
        self.engine.borrow().cancel();
    }

    /// Give the verdict on the last assessment — recorded as memory,
    /// taught to nothing (§7.3: a verdict is not a preference).
    pub fn give_feedback(&self, helpful: bool) {
        let verdict = if helpful {
            u_memory::Verdict::Helpful
        } else {
            u_memory::Verdict::NotHelpful
        };
        let mut engine = self.engine.borrow_mut();
        let _ = engine.give_feedback(verdict, None);
        self.feedback_given.set(true);
    }

    /// The workspace's field writes — view state, not events, for the same
    /// reason the nudge is not: an unfinished decision is not yet a fact.
    pub fn set_situation(&self, next: TextEditingValue) {
        self.situation.set(next);
    }

    /// See [`set_situation`](Self::set_situation).
    pub fn set_choices(&self, next: TextEditingValue) {
        self.choices.set(next);
    }

    /// See [`set_situation`](Self::set_situation).
    pub fn set_wants(&self, next: TextEditingValue) {
        self.wants.set(next);
    }

    /// See [`set_situation`](Self::set_situation).
    pub fn set_bounds(&self, next: TextEditingValue) {
        self.bounds.set(next);
    }

    /// See [`set_situation`](Self::set_situation).
    pub fn set_follow_up(&self, next: TextEditingValue) {
        self.follow_up.set(next);
    }

    /// The most recent decision's own words, for home's strip — `None` on a
    /// first run, where showing nothing is more honest than a placeholder.
    #[must_use]
    pub fn latest_decision_words(&self) -> Option<String> {
        let engine = self.engine.borrow();
        engine.latest_decision().ok().flatten().map(|d| d.situation)
    }
}

/// One entry per non-empty line, trimmed — the shape the workspace's
/// multiline fields hand the engine, so "stay · move" typed as two lines is
/// two options and never one option with a newline in it.
fn lines_of(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
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

/// Mount the app onto a driver with the given log and engine: create the
/// state, fold the log's history into it, wire the routes, attach the
/// tickers. Returns the shared slot — for `root`, and for the screenshot
/// tool.
///
/// The engine arrives built rather than as a directory because *why* it
/// might not have been is a platform story, told where the platform can
/// hear it: a phone logs it, a desktop prints it, and a headless render
/// never had a directory to begin with.
#[must_use]
pub fn mount_with(driver: &mut FrameDriver, log: events::Log, engine: engine::Engine) -> Shared {
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
    let work = ScrollController::new(&runtime, ScrollPhysics::android());

    // How many decisions the memory already holds, so home's strip is honest
    // the moment the app opens rather than the first time a decision lands.
    let decisions = runtime.signal(engine.decision_count());

    let state = AppState {
        nav,
        scroll,
        work,
        engine: Rc::new(RefCell::new(engine)),
        clock,
        decay,
        log: Rc::new(log),
        day: Rc::new(Cell::new(0)),
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
        answers: runtime.signal(0u32),
        seen_sketch: runtime.signal(false),
        ask: runtime.signal(0usize),
        noted: runtime.signal(None::<&'static str>),
        nudge: runtime.signal(true),
        situation: runtime.signal(TextEditingValue::new(String::new())),
        choices: runtime.signal(TextEditingValue::new(String::new())),
        wants: runtime.signal(TextEditingValue::new(String::new())),
        bounds: runtime.signal(TextEditingValue::new(String::new())),
        follow_up: runtime.signal(TextEditingValue::new(String::new())),
        assessment: runtime.signal(None::<Rc<u_decision::Assessment>>),
        assess_error: runtime.signal(None::<String>),
        feedback_given: runtime.signal(false),
        decisions,
        skip_at: runtime.signal(f32::INFINITY),
    };

    // The fold: whatever the log holds, the state becomes. This is the only
    // path by which anything survives a restart, and it runs through the
    // same `apply` every live change does, so the two cannot disagree.
    for event in state.log.history() {
        state.apply(event);
    }

    // A person who asked for calm does not open on a four-second staging:
    // the opening resolves before it begins, and the room simply fades in.
    if state.calm.get() {
        state.skip_at.set(0.0);
    }

    // The clock and the decay are weakly held by the driver; the state is the
    // strong owner that keeps them alive for as long as the app runs.
    driver.tickers().add(&state.clock);
    driver.tickers().add(&state.decay);
    state.nav.attach(driver.tickers());
    state.scroll.attach(driver.tickers());
    state.work.attach(driver.tickers());

    *slot.borrow_mut() = Some(state);
    slot
}

/// Mount with a log that keeps nothing and an engine with nowhere to keep
/// anything — the screenshot tool's mount, so its renders never depend on
/// the machine drawing them.
#[must_use]
pub fn mount(driver: &mut FrameDriver) -> Shared {
    mount_with(driver, events::Log::volatile(), engine::Engine::scratch())
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

/// The ask sheet — the one question, rising from the bottom over a scrim.
/// Home stays on stage beneath it, which is the point: the companion is
/// still there while you answer, and the field fires behind the scrim when
/// a choice lands.
#[must_use]
pub fn ask_route(shared: Shared) -> Route {
    Route::modal(
        "ask",
        Rc::new(move |_| {
            AskSheet {
                state: take(&shared),
                shared: shared.clone(),
            }
            .into()
        }),
    )
    .transition(RouteTransition::SlideFromBottom)
}

/// The decision workspace — the room home's own action opens, pushed like
/// every phone pushes a page and popped the same way: the chevron, or a
/// swipe back along the direction it arrived from.
///
/// A push rather than a sheet, because this is not a question over the
/// room — it is the work itself: the situation, the assessment, and the
/// one next step, staying until the person is done with them.
#[must_use]
pub fn workspace_route(shared: Shared) -> Route {
    Route::new(
        "workspace",
        Rc::new(move |_| {
            WorkspaceScreen {
                state: take(&shared),
                shared: shared.clone(),
            }
            .into()
        }),
    )
    .transition(RouteTransition::SlideFromEnd)
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
///
/// With the `model` feature (`cargo run -p u --release --features model`),
/// the engine opens on Qwen3-0.6B when verified weights are present —
/// see `docs/models.md` — and on the deterministic baseline otherwise,
/// which is also what every default build opens on. The §10 release
/// gates keep the model out of APK builds until Phase 3's physical-device
/// reports exist.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let data = vieww_platform_winit::data_dir();
    // The engine's memory lives beside the log's, under a name of its own.
    // A store that cannot be opened is said and fallen back from — the
    // person still gets the app, on a memory that keeps nothing, rather
    // than a window that never comes.
    let engine = engine_at(data.join("u-store"));
    let report = App::new()
        .title("U")
        .size(SURFACE)
        .theme(palette::theme())
        .run(|driver| {
            driver.set_fonts(fonts());
            let log = events::Log::at(vieww_platform_winit::data_dir().join("u-events.log"));
            let shared = mount_with(driver, log, engine);
            driver.set_root(root(&shared));
        })?;
    println!("{report}");
    Ok(())
}

/// Open the engine: the deterministic baseline, or — when this build grew
/// the `model` feature — the model this machine's cohort allows, if its
/// verified weights are present. Failure to open a model is not failure
/// to open the app: the baseline answers, and the reason is said.
fn engine_at(store_dir: std::path::PathBuf) -> engine::Engine {
    #[cfg(feature = "model")]
    {
        if let Some(engine) = engine_with_model(&store_dir) {
            return engine;
        }
    }
    engine::Engine::at(&store_dir).unwrap_or_else(|error| {
        eprintln!(
            "U: the decision memory could not be opened ({error}); this session keeps nothing"
        );
        engine::Engine::scratch()
    })
}

/// The model this machine's own cohort allows, loaded from the directory
/// the benchmark fetches into (`U_MODELS_DIR`, or `~/.u/models`), verified
/// against its manifest before anything runs. `None` — and the baseline
/// takes over — when the weights are absent, the digest is wrong, or the
/// tokenizer will not open: every one a fact to say, not a wall to hit.
#[cfg(feature = "model")]
fn engine_with_model(store_dir: &std::path::Path) -> Option<engine::Engine> {
    use u_ml_runtime::{cohort, Manifest, Qwen, Runtime as _, TokenizerManifest};

    let dir = std::env::var_os("U_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::Path::new(&home).join(".u").join("models"))
        })?;
    // The cohort reads this machine's own memory: the caps a phone earns
    // are the caps this decision runs under, which is §5.5's whole point.
    let caps = cohort::current().caps();
    let tokenizer = TokenizerManifest::embedded();
    for manifest in Manifest::embedded() {
        if !cohort::current()
            .quant_preference()
            .contains(&manifest.quant)
        {
            continue;
        }
        if !dir.join(&manifest.file).is_file() {
            continue;
        }
        match Qwen::load(&manifest, &tokenizer, &dir, caps) {
            Ok(qwen) => {
                eprintln!(
                    "U: {} loaded, under this machine's cohort caps",
                    qwen.name()
                );
                return engine::Engine::at_with(store_dir, Box::new(qwen)).ok();
            }
            Err(error) => {
                eprintln!(
                    "U: {} refused to load ({error}); the baseline answers",
                    manifest.file
                );
            }
        }
    }
    None
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
    // The log's home is the activity's private directory. Before the activity
    // is attached there may not be one; the run keeps nothing rather than
    // refusing to start, and says so where a person can read it.
    let internal = android.internal_data_path();
    let log = match &internal {
        Some(dir) => events::Log::at(dir.join("u-events.log")),
        None => {
            log::warn!("U: no internal data path; this session keeps nothing");
            events::Log::volatile()
        }
    };
    // The engine's store lives there too, and is refused for the same
    // reasons it would be on a desktop: a schema from a newer build is
    // left untouched rather than half-read, and the app opens on a memory
    // that keeps nothing rather than not at all.
    let engine = match &internal {
        Some(dir) => engine::Engine::at(dir.join("u-store")).unwrap_or_else(|error| {
            log::warn!(
                "U: the decision memory could not be opened ({error}); this session keeps nothing"
            );
            engine::Engine::scratch()
        }),
        None => engine::Engine::scratch(),
    };
    let result = App::new()
        .title("U")
        .size(SURFACE)
        .theme(palette::theme())
        .run_android(android, move |driver| {
            driver.set_fonts(fonts());
            let shared = mount_with(driver, log, engine);
            driver.set_root(root(&shared));
        });
    if let Err(error) = result {
        // A phone discards stderr; this is the only way the reason for an
        // activity that opened and vanished reaches anybody.
        log::error!("U: {error}");
    }
}
