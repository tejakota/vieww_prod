//! Choosing the GPU path's route — direct or blit — by measuring both on
//! this device.
//!
//! # Why this exists
//!
//! A depth-0 opaque frame can reach the screen two ways
//! (`NativeRenderer::present_planned`): **direct**, one render pass straight
//! into the swapchain image, redrawing every pixel of every frame; or
//! **blit**, into the kept `RGBA16F` intermediate, redrawing and carrying
//! only the damage, then copying out. Which is cheaper is a fact about the
//! phone's GPU, and it goes both ways:
//!
//! | phone | direct | blit | right choice |
//! |---|---|---|---|
//! | S20-class (SM8250), Device Farm 43709382309 | 4.95 ms | ~21 ms | **direct** |
//! | Redmi Note 7 Pro (SM6150, Adreno 612), 2026-11-16 | 28.9 ms, every phase | 12.5 scroll / 19 live / 18 storm | **blit** |
//!
//! On the fast GPU the full pass is cheap and the intermediate's traffic is
//! the frame. On the slow one the full pass *is* the frame — 29 ms whatever
//! changed, 34 fps — and the damage is what gets a scroll to the display's
//! rate (16.6 ms, 446-484 frames of 480). A constant picks wrong on one of
//! them, so this measures, the way [`crate::rasterizer_choice`] does for
//! CPU against GPU — but within a run, because both routes share a device,
//! a swapchain and a renderer, and switching is one frame's full redraw.
//!
//! # How
//!
//! Frames go direct first. The chooser reads each eligible frame's GPU time
//! (the renderer's timestamp queries) and takes the median of a window:
//!
//! 1. A route whose median **fits** ([`FITS`]) is kept and nothing is tried:
//!    a phone that makes its frames is never made to miss some to find out
//!    whether the other route makes them faster.
//! 2. Otherwise the other route is **tried** for one window. It takes over
//!    only if it is clearly faster ([`CLEARLY_FASTER`]); a tie stays where
//!    it is.
//! 3. The answer is **held**, then checked again while the frame still does
//!    not fit — what is on screen changes what the blit route costs. Each
//!    check that confirms the answer doubles the hold ([`HOLD`] up to
//!    [`HOLD_MAX`]).
//! 4. A check **measures the route in use first, and tries the other only
//!    if it has stopped beating what the other last measured**. The first
//!    version tried the other route at every check, and on the Redmi
//!    (2026-11-16, auto) those trials were the run's jank: direct lost 29.0
//!    to 11.5, then 29.1 to 17.2, 29.0 to 17.7, 29.1 to 21.0 — four times
//!    the same answer, each a second at 34 fps, and all 35 of a live phase's
//!    janky frames. A loser that lost by that much is not asked again until
//!    the winner has slowed to within [`CLEARLY_FASTER`] of it, or
//!    [`RETRY`] frames have passed since it was last measured — its own
//!    cost may have changed too.
//!
//! The first frames after a switch are not counted ([`SETTLE`]): the
//! renderer reports a frame's GPU time a frame or two late, and the frame
//! that changes route redraws in full on either.
//!
//! # Remembered between launches
//!
//! Measuring costs the first trial: a second on the losing route, which on
//! the Redmi (2026-11-17, a Game Loop launch over USB) was most of a scroll
//! phase's jank, 38 late frames with a 265 ms worst one. So each trial's
//! answer is written to a small file beside the pipeline cache
//! ([`set_memory_file`]), keyed by the GPU, its driver (`driver_key`) and
//! this crate's version. The next launch on the same device starts on the
//! remembered route, with the loser's median as `other`, and so begins by
//! *measuring the route in use*: if it fits, or is still clearly ahead of
//! what the loser measured, nothing is tried. A different GPU, a driver
//! update or a new build finds no match and measures from scratch.
//!
//! # What this is not
//!
//! It is not consulted for a frame that cannot go direct (layers, a
//! translucent window, a resolution scale, a surface without colour
//! attachments): those blit, and are not samples of anything.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

static MEMORY_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Where route answers are kept between launches, or `None` for nowhere.
/// Set before the first GPU frame (beside the pipeline cache).
pub(crate) fn set_memory_file(file: Option<PathBuf>) {
    if let Ok(mut current) = MEMORY_FILE.lock() {
        *current = file;
    }
}

fn memory_file() -> Option<PathBuf> {
    MEMORY_FILE.lock().ok()?.clone()
}

/// A remembered answer: the route, and both medians that decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Memory {
    key: String,
    route: Route,
    direct: Duration,
    blit: Duration,
}

impl Memory {
    fn to_text(&self) -> String {
        format!(
            "key={}\nroute={}\ndirect_us={}\nblit_us={}\n",
            self.key,
            match self.route {
                Route::Direct => "direct",
                Route::Blit => "blit",
            },
            self.direct.as_micros(),
            self.blit.as_micros()
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
                .map(str::trim)
        };
        let us = |name: &str| field(name)?.parse::<u64>().ok().map(Duration::from_micros);
        Some(Self {
            key: field("key")?.to_owned(),
            route: match field("route")? {
                "direct" => Route::Direct,
                "blit" => Route::Blit,
                _ => return None,
            },
            direct: us("direct_us")?,
            blit: us("blit_us")?,
        })
    }

    fn read(file: &Path, key: &str) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(file).ok()?).filter(|m| m.key == key)
    }
}

/// The key a remembered answer is filed under: the GPU and driver, and this
/// crate's version (the routes' costs are the renderer's as much as the
/// GPU's).
pub(crate) fn memory_key(driver_key: &str) -> String {
    format!("{driver_key}:{}", env!("CARGO_PKG_VERSION"))
}

/// A way to the swapchain image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// One pass into the swapchain image; every pixel, every frame.
    Direct,
    /// Damage into the kept intermediate, then a copy out.
    Blit,
}

impl Route {
    const fn other(self) -> Self {
        match self {
            Self::Direct => Self::Blit,
            Self::Blit => Self::Direct,
        }
    }
}

/// Who decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Always this route (`VIEWW_GPU_DIRECT=1` / `=0`).
    Fixed(Route),
    /// Measure (`VIEWW_GPU_DIRECT=auto`; the default on Android).
    Auto,
}

/// GPU frames in a measurement.
const WINDOW: usize = 30;
/// Frames after a switch that are not measured.
const SETTLE: u32 = 4;
/// A median GPU frame at or under this is making a 60 Hz display's frames
/// with room for the CPU's half; nothing is tried against it.
const FITS: Duration = Duration::from_micros(12_500);
/// The challenger's median must be under this share of the incumbent's.
const CLEARLY_FASTER: f64 = 0.9;
/// Frames an answer is held before it is checked again, at first.
const HOLD: u32 = 600;
/// And at most.
const HOLD_MAX: u32 = 38_400;
/// Frames after which the other route is tried again however well the one
/// in use compares with its last measurement: ten minutes at 60 Hz.
const RETRY: u32 = 36_000;

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// Measuring the route in use.
    Measure,
    /// Measuring the other route against `incumbent`'s median.
    Trial { incumbent: Duration },
    /// Using the answer for this many more frames.
    Hold { left: u32 },
}

/// See the module docs.
#[derive(Debug, Clone)]
pub(crate) struct RouteChooser {
    mode: Mode,
    current: Route,
    phase: Phase,
    samples: Vec<Duration>,
    settle: u32,
    hold: u32,
    /// Times a trial replaced the route in use.
    switches: u32,
    /// The last two medians compared, `(direct, blit)`, for narration.
    compared: Option<(Duration, Duration)>,
    /// What the route *not* in use last measured, and how many eligible
    /// frames ago.
    other: Option<(Duration, u32)>,
    /// Where a trial's answer is written, and under which key.
    memory: Option<(PathBuf, String)>,
    /// Whether this launch started from a remembered answer.
    remembered: bool,
}

impl RouteChooser {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            current: match mode {
                Mode::Fixed(route) => route,
                Mode::Auto => Route::Direct,
            },
            phase: Phase::Measure,
            samples: Vec::with_capacity(WINDOW),
            settle: SETTLE,
            hold: HOLD,
            switches: 0,
            compared: None,
            other: None,
            memory: None,
            remembered: false,
        }
    }

    /// Start from what an earlier launch measured on this device, and keep
    /// this launch's answers there too. `key` is [`memory_key`]'s. Called
    /// once, when the GPU device is known; does nothing outside
    /// [`Mode::Auto`] or without a [`set_memory_file`].
    pub(crate) fn remember(&mut self, key: String) {
        if self.mode != Mode::Auto {
            return;
        }
        let Some(file) = memory_file() else {
            return;
        };
        if let Some(m) = Memory::read(&file, &key) {
            self.current = m.route;
            self.other = Some((
                match m.route {
                    Route::Direct => m.blit,
                    Route::Blit => m.direct,
                },
                0,
            ));
            self.compared = Some((m.direct, m.blit));
            self.remembered = true;
            self.begin(Phase::Measure);
        }
        self.memory = Some((file, key));
    }

    /// Whether this launch started on a remembered route.
    pub(crate) const fn is_remembered(&self) -> bool {
        self.remembered
    }

    fn save(&self) {
        let (Some((file, key)), Some((direct, blit))) = (&self.memory, self.compared) else {
            return;
        };
        let text = Memory {
            key: key.clone(),
            route: self.current,
            direct,
            blit,
        }
        .to_text();
        // A few bytes, once per trial: rare, and losing it costs only the
        // next launch's head start.
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, text);
    }

    /// The route an eligible frame takes now.
    pub(crate) const fn route(&self) -> Route {
        self.current
    }

    /// Whether the route is being measured rather than fixed.
    pub(crate) const fn is_auto(&self) -> bool {
        matches!(self.mode, Mode::Auto)
    }

    /// The medians the last comparison weighed, `(direct, blit)`.
    pub(crate) const fn compared(&self) -> Option<(Duration, Duration)> {
        self.compared
    }

    /// One eligible frame went through [`route`](Self::route) and the
    /// renderer's latest GPU time is `gpu` (zero when it has none yet).
    /// Returns whether the route changed.
    pub(crate) fn observe(&mut self, gpu: Duration) -> bool {
        if self.mode != Mode::Auto {
            return false;
        }
        if let Some((_, age)) = &mut self.other {
            *age = age.saturating_add(1);
        }
        if let Phase::Hold { left } = &mut self.phase {
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.begin(Phase::Measure);
            }
            return false;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return false;
        }
        if gpu.is_zero() {
            // No timestamp queries on this device, or none back yet.
            return false;
        }
        self.samples.push(gpu);
        if self.samples.len() < WINDOW {
            return false;
        }
        self.samples.sort_unstable();
        let median = self.samples[WINDOW / 2];
        match self.phase.clone() {
            Phase::Measure => {
                // Still clearly ahead of what the other route last
                // measured, recently enough to believe: nothing to try.
                let still_ahead = self.other.is_some_and(|(other, age)| {
                    age < RETRY && median.as_secs_f64() < other.as_secs_f64() * CLEARLY_FASTER
                });
                if median <= FITS {
                    self.begin(Phase::Hold { left: self.hold });
                    false
                } else if still_ahead {
                    self.hold = (self.hold.saturating_mul(2)).min(HOLD_MAX);
                    self.begin(Phase::Hold { left: self.hold });
                    false
                } else {
                    self.current = self.current.other();
                    self.begin(Phase::Trial { incumbent: median });
                    true
                }
            }
            Phase::Trial { incumbent } => {
                // `current` is the challenger here.
                self.compared = Some(match self.current {
                    Route::Direct => (median, incumbent),
                    Route::Blit => (incumbent, median),
                });
                let wins = median.as_secs_f64() < incumbent.as_secs_f64() * CLEARLY_FASTER;
                if wins {
                    // A new answer starts its own schedule.
                    self.hold = HOLD;
                    self.switches += 1;
                    self.other = Some((incumbent, 0));
                } else {
                    self.current = self.current.other();
                    self.hold = (self.hold.saturating_mul(2)).min(HOLD_MAX);
                    self.other = Some((median, 0));
                }
                self.begin(Phase::Hold { left: self.hold });
                self.save();
                self.remembered = false;
                !wins
            }
            Phase::Hold { .. } => false,
        }
    }

    fn begin(&mut self, phase: Phase) {
        self.phase = phase;
        self.samples.clear();
        self.settle = SETTLE;
    }

    /// Times a trial replaced the route in use.
    #[cfg(test)]
    pub(crate) const fn switches(&self) -> u32 {
        self.switches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(v: u64) -> Duration {
        Duration::from_micros(v * 1000)
    }

    /// Feed `frames` frames whose GPU time depends on the route in use.
    fn run(chooser: &mut RouteChooser, frames: usize, direct: Duration, blit: Duration) {
        for _ in 0..frames {
            let gpu = match chooser.route() {
                Route::Direct => direct,
                Route::Blit => blit,
            };
            chooser.observe(gpu);
        }
    }

    /// The memory file is process-wide; tests that use it take turns.
    static FILE_LOCK: Mutex<()> = Mutex::new(());

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vieww-gpu-route-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("gpu-route-test.txt")
    }

    #[test]
    fn a_memory_reads_back_what_it_wrote() {
        let m = Memory {
            key: "5143:6040:01020304:ab:0.1".into(),
            route: Route::Blit,
            direct: ms(29),
            blit: ms(12),
        };
        assert_eq!(Memory::parse(&m.to_text()), Some(m));
        assert_eq!(Memory::parse("route=sideways\n"), None);
    }

    /// The Redmi's second launch: the first launch's trial is remembered, so
    /// it starts on blit and never spends a window on direct.
    #[test]
    fn a_remembered_answer_skips_the_first_trial() {
        let _turn = FILE_LOCK.lock().unwrap();
        let file = temp_file("skip");
        set_memory_file(Some(file.clone()));
        // First launch: measures, tries blit, keeps it, writes it down.
        let mut first = RouteChooser::new(Mode::Auto);
        first.remember("redmi".into());
        assert!(!first.is_remembered());
        run(&mut first, 200, ms(29), ms(12));
        assert_eq!(first.route(), Route::Blit);
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("route=blit"));
        // Second launch: on blit from its first frame, no frame on direct.
        let mut second = RouteChooser::new(Mode::Auto);
        second.remember("redmi".into());
        assert!(second.is_remembered());
        let mut on_direct = 0;
        for _ in 0..5_000 {
            on_direct += usize::from(second.route() == Route::Direct);
            second.observe(match second.route() {
                Route::Direct => ms(29),
                Route::Blit => ms(12),
            });
        }
        assert_eq!(
            on_direct, 0,
            "{on_direct} frames re-trying the remembered loser"
        );
        // Another GPU or driver (another key) measures from scratch.
        let mut other = RouteChooser::new(Mode::Auto);
        other.remember("another-gpu".into());
        assert!(!other.is_remembered());
        assert_eq!(other.route(), Route::Direct);
        set_memory_file(None);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// A remembered answer that has gone stale (the blit route got slow) is
    /// still checked: measuring it first finds it no longer ahead.
    #[test]
    fn a_stale_memory_is_corrected() {
        let _turn = FILE_LOCK.lock().unwrap();
        let file = temp_file("stale");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            Memory {
                key: "k".into(),
                route: Route::Blit,
                direct: ms(6),
                blit: ms(5),
            }
            .to_text(),
        )
        .unwrap();
        set_memory_file(Some(file.clone()));
        let mut c = RouteChooser::new(Mode::Auto);
        c.remember("k".into());
        assert_eq!(c.route(), Route::Blit);
        run(&mut c, 400, ms(6), ms(25));
        assert_eq!(c.route(), Route::Direct);
        set_memory_file(None);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// A fixed mode never reads or writes a memory.
    #[test]
    fn a_fixed_route_ignores_memory() {
        let _turn = FILE_LOCK.lock().unwrap();
        let file = temp_file("fixed");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            Memory {
                key: "k".into(),
                route: Route::Blit,
                direct: ms(30),
                blit: ms(5),
            }
            .to_text(),
        )
        .unwrap();
        set_memory_file(Some(file.clone()));
        let mut c = RouteChooser::new(Mode::Fixed(Route::Direct));
        c.remember("k".into());
        assert_eq!(c.route(), Route::Direct);
        set_memory_file(None);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// The Redmi Note 7 Pro: direct 29 ms, blit 14-19.
    #[test]
    fn a_fill_bound_phone_ends_up_on_the_blit_route() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        assert_eq!(chooser.route(), Route::Direct);
        run(&mut chooser, 200, ms(29), ms(17));
        assert_eq!(chooser.route(), Route::Blit);
        assert_eq!(chooser.switches(), 1);
        assert_eq!(chooser.compared(), Some((ms(29), ms(17))));
        // And stays there: later checks try direct for a window and return.
        let mut on_direct = 0;
        for _ in 0..20_000 {
            on_direct += usize::from(chooser.route() == Route::Direct);
            chooser.observe(match chooser.route() {
                Route::Direct => ms(29),
                Route::Blit => ms(17),
            });
        }
        assert_eq!(chooser.route(), Route::Blit);
        assert_eq!(chooser.switches(), 1);
        // The checks measure blit, find it still far ahead of direct's 29,
        // and try nothing: the phone's own auto run spent a second on
        // direct at each of them.
        assert_eq!(
            on_direct, 0,
            "{on_direct} frames spent re-trying a known loser"
        );
    }

    /// The Redmi's thermal minute: the blit route slows towards the
    /// direct route's last measurement, and that is when it is asked again.
    #[test]
    fn a_loser_is_tried_again_when_the_winner_slows_to_it() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        run(&mut chooser, 200, ms(29), ms(17));
        assert_eq!(chooser.route(), Route::Blit);
        // 27 ms is not under 0.9 x 29: the next check tries direct, which
        // still loses to it by too little to matter either way — and stays
        // lost, because 29 is not clearly faster than 27.
        let mut on_direct = 0;
        for _ in 0..2_000 {
            on_direct += usize::from(chooser.route() == Route::Direct);
            chooser.observe(match chooser.route() {
                Route::Direct => ms(29),
                Route::Blit => ms(27),
            });
        }
        assert!(on_direct > 0, "direct was never asked again");
        assert_eq!(chooser.route(), Route::Blit);
    }

    /// A measurement is not believed for ever: the other route's cost can
    /// change too.
    #[test]
    fn an_old_measurement_is_retaken() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        run(&mut chooser, 200, ms(29), ms(17));
        assert_eq!(chooser.route(), Route::Blit);
        // Direct becomes the faster route; blit still beats direct's stale
        // 29, so only the retry age finds out.
        run(
            &mut chooser,
            RETRY as usize + 2 * HOLD_MAX as usize,
            ms(13),
            ms(17),
        );
        assert_eq!(chooser.route(), Route::Direct);
    }

    /// The S20: direct fits, so the slower route is never shown.
    #[test]
    fn a_route_that_fits_is_never_tried_against() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        for _ in 0..50_000 {
            assert_eq!(chooser.route(), Route::Direct);
            chooser.observe(ms(5));
        }
        assert_eq!(chooser.switches(), 0);
    }

    /// Both miss and neither is clearly faster: stay, and check less often.
    #[test]
    fn a_tie_stays_where_it_is() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        run(&mut chooser, 5_000, ms(30), ms(29));
        assert_eq!(chooser.route(), Route::Direct);
        assert_eq!(chooser.switches(), 0);
    }

    /// The content changes under a settled answer: the next check moves.
    #[test]
    fn the_answer_follows_the_phone_when_the_costs_change() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        run(&mut chooser, 200, ms(29), ms(17));
        assert_eq!(chooser.route(), Route::Blit);
        run(&mut chooser, 2_000, ms(14), ms(40));
        assert_eq!(chooser.route(), Route::Direct);
    }

    #[test]
    fn a_fixed_route_ignores_what_it_costs() {
        for route in [Route::Direct, Route::Blit] {
            let mut chooser = RouteChooser::new(Mode::Fixed(route));
            run(&mut chooser, 5_000, ms(40), ms(40));
            assert_eq!(chooser.route(), route);
        }
    }

    /// No timestamps, no opinion: the default route, for ever.
    #[test]
    fn a_device_without_gpu_times_stays_direct() {
        let mut chooser = RouteChooser::new(Mode::Auto);
        run(&mut chooser, 5_000, Duration::ZERO, Duration::ZERO);
        assert_eq!(chooser.route(), Route::Direct);
    }
}
