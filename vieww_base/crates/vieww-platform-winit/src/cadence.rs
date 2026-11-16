//! Which frame rate a window draws at: the panel's, or 60 Hz when the panel's
//! cannot be held.
//!
//! # Why
//!
//! The frame budget used to be whatever the panel reported. On a 120 Hz phone
//! that is 8.33 ms whether or not anything can meet it, and what a person sees
//! when it is missed is not "a bit under 120" but **uneven pacing**: frames
//! shown for one vsync, then two, then one. Device Farm run 39402544760, the
//! Galaxy A73: the shipping GPU path's median was inside 8.33 ms and 52 of 240
//! frames (22%) were not. A steady 60 looks better than that ragged 120, and
//! every native toolkit and game engine that runs on these panels makes the
//! same trade.
//!
//! So a window holds one of two rates — the panel's, or 60 Hz — and moves
//! between them on evidence:
//!
//! - **Down** to 60 when, for [`DOWN_WINDOWS`] seconds of continuous frames
//!   in a row at the panel's rate, more than [`MAX_OVER_SHARE`] of them
//!   **missed a vsync**: came more than [`LATE_FACTOR`] budgets after the
//!   one before. A frame whose work runs past the budget is not by itself a
//!   miss. The next frame's work overlaps this one's display, and run
//!   43879609229's SM8750 drew scroll at 131.5 fps with 0.3% of frames
//!   missed while its 95th-percentile work was 8.74 ms against 8.33.
//! - **Up** to the panel's rate when, over [`UP_WINDOWS`] windows in a row at
//!   60 Hz, the 95th-percentile frame's own work would fit the panel's budget
//!   with [`UP_HEADROOM`] to spare; or, failing that, as a **probe** after
//!   [`PROBE_FIRST`] of continuous frames at 60 Hz. A probe that fails, that
//!   drops again within [`PROBE_FAILED_WITHIN`], doubles the wait for the
//!   next, up to [`PROBE_MAX`]. The probe exists because work measured at
//!   60 Hz is work at 60 Hz clocks: the GPU's governor and, without a
//!   performance hint, the CPU's slow down to fit 16.7 ms. Judged on that
//!   alone, a phone that holds 120 Hz with room looks like one that cannot,
//!   and run 43912018582's SM8850 went back up once in four runs.
//!
//! What is *never* traded is resolution: rendering the interface below the
//! panel's pixel density softens text, and a framework that wants to be the
//! best one does not do that to its UI to win a frame rate. The resolution
//! scale stays an explicit opt-in (`crate::quality`).
//!
//! # What "work" is here
//!
//! The frame's own work, without the time spent waiting for a buffer to
//! present into (`acquire`): at 60 Hz on a 120 Hz panel that wait is the
//! pacing itself, and counting it would make every frame look like it needs
//! the whole 16.7 ms — the rate could never come back up.
//!
//! # Holding the rate
//!
//! The policy only decides. The window asks the platform for the panel's
//! rate once, at start ([`request`], seamless switches only), and from then
//! on paces its own frames at whatever the policy holds. At 60 Hz on a
//! 120 Hz panel each frame is simply shown for two vsyncs, steadily. It does
//! not ask the panel to change mode when the policy changes: a probe of the
//! panel's rate then costs no mode switch, and no switch can blank a frame.
//!
//! An application that states its rate (`App::refresh_rate`) is never
//! adapted, and `VIEWW_CADENCE=panel` (Android: `debug.vieww.cadence`) turns
//! adaptation off for a measurement that wants the panel's rate regardless.

use std::time::Duration;

/// The rate a window falls back to.
pub(crate) const FALLBACK_HZ: f32 = 60.0;

/// More than this share of a window's frames over the panel's budget, and it
/// is not being held. One in twenty: the same share the Device Farm gate
/// allows (`ci/mobile`'s `frames_inside_their_budget`).
pub(crate) const MAX_OVER_SHARE: f64 = 0.05;

/// At 60 Hz, the 95th-percentile frame must fit this fraction of the panel's
/// budget before the panel's rate is tried again.
pub(crate) const UP_HEADROOM: f64 = 0.75;

/// Consecutive windows over [`MAX_OVER_SHARE`] before going down.
///
/// One used to be enough, and one heavy second is not a rate a phone cannot
/// hold: it is a data load, a jump, a list rebuilt. Run 43912018582's frame
/// rate log had both 120 Hz phones drop to 60 Hz on CPU and GPU alike within
/// the schedule's first phases, its loads and jumps, before any continuous
/// work started. Market Watch's tick and jump are a second each, back to
/// back, so three seconds in a row is the shortest stretch that is
/// sustained work rather than two bursts; a burst is a dropped frame or two,
/// the same as on every other toolkit.
pub(crate) const DOWN_WINDOWS: u32 = 3;

/// A frame came late (missed a vsync) when its interval from the frame
/// before is more than this many budgets.
pub(crate) const LATE_FACTOR: f64 = 1.5;

/// Continuous frames at 60 Hz before the first probe of the panel's rate.
pub(crate) const PROBE_FIRST: Duration = Duration::from_secs(5);

/// The longest wait between probes, however many have failed.
pub(crate) const PROBE_MAX: Duration = Duration::from_secs(60);

/// A probe that drops again within this was a failed probe.
pub(crate) const PROBE_FAILED_WITHIN: Duration = Duration::from_secs(10);

/// Consecutive windows that must clear [`UP_HEADROOM`] before going back up.
pub(crate) const UP_WINDOWS: u32 = 3;

/// A gap between frames this long means the app went idle; the evidence so
/// far describes a different stretch of work.
///
/// A second, not a few frame intervals. Interfaces draw in bursts — a tap,
/// a flick, a keyboard sliding in — with pauses of a few hundred
/// milliseconds between them, and a reset on every pause meant the policy
/// almost never saw a whole window: Device Farm run 39437190809's S26 CPU
/// pass drew 600 frames in 5.1 s, only 34 of its last 240 back to back, had
/// 17 of them late at 120 Hz, and dropped to 60 Hz 1.3 s after the suite
/// had already judged it. The stutter was there the whole time.
const IDLE_RESET: Duration = Duration::from_secs(1);

/// Frames in a window's first second are not evidence. Start-up is the
/// slowest stretch an app has — the first raster, pipelines, fonts — and
/// counting it would drop nearly every launch on a 120 Hz phone to 60 Hz,
/// to climb back three seconds later.
const WARM_UP: Duration = Duration::from_secs(1);

/// The rate policy for one window. See the module docs.
#[derive(Debug, Clone)]
pub(crate) struct Cadence {
    panel_hz: f32,
    target_hz: f32,
    /// Work of the frames in the current window, reused between windows.
    window: Vec<Duration>,
    window_len: usize,
    /// When the last frame was observed, for gap detection.
    last: Option<Duration>,
    /// When the first frame was observed, for [`WARM_UP`].
    first: Option<Duration>,
    /// Windows in a row at the fallback that cleared the headroom.
    clear_windows: u32,
    /// Windows in a row at the panel's rate that were over the allowance.
    over_windows: u32,
    /// Each window frame's interval from the frame before, beside `window`.
    gaps: Vec<Duration>,
    /// Continuous frame time at the fallback since the last change.
    at_fallback: Duration,
    /// How much of that before the next probe.
    probe_wait: Duration,
    /// When the last probe went up, if the rate is up because of one.
    probed_at: Option<Duration>,
    /// The last window's evidence, for the log line a change writes.
    evidence: String,
    switches: u32,
    /// Whether the platform has been asked for the starting rate yet.
    asked: bool,
}

impl Cadence {
    /// A policy for a panel at `panel_hz`, or `None` when there is nothing to
    /// adapt: a panel at or below the fallback rate.
    #[must_use]
    pub(crate) fn new(panel_hz: f32) -> Option<Self> {
        if !panel_hz.is_finite() || panel_hz <= FALLBACK_HZ + 0.5 {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a refresh rate is a small positive number"
        )]
        let window_len = (panel_hz.round() as usize).max(30);
        Some(Self {
            panel_hz,
            target_hz: panel_hz,
            window: Vec::with_capacity(window_len),
            window_len,
            last: None,
            first: None,
            clear_windows: 0,
            over_windows: 0,
            gaps: Vec::with_capacity(window_len),
            at_fallback: Duration::ZERO,
            probe_wait: PROBE_FIRST,
            probed_at: None,
            evidence: String::new(),
            switches: 0,
            asked: false,
        })
    }

    /// The rate the window should draw at now.
    #[must_use]
    pub(crate) const fn target_hz(&self) -> f32 {
        self.target_hz
    }

    /// The rate to ask the platform for before the first frame, once.
    ///
    /// The window used to ask only when the rate changed, so at the panel's
    /// rate it never asked at all. Adaptive panels (Samsung's 120 Hz, the
    /// Pixel 5's 90 Hz) then run a surface that has voted for nothing at
    /// 60 Hz. Run 43889062992 measured it: Vieww's frames came 16.67 ms apart
    /// on all of them while Valdi's views, which HWUI votes for, ran at
    /// 87-125 fps on the same phones.
    pub(crate) fn initial_request(&mut self) -> Option<f32> {
        if self.asked {
            return None;
        }
        self.asked = true;
        Some(self.target_hz)
    }

    /// What the last full window measured, for the log.
    #[must_use]
    pub(crate) fn evidence(&self) -> &str {
        &self.evidence
    }

    /// The panel's own rate.
    #[must_use]
    pub(crate) const fn panel_hz(&self) -> f32 {
        self.panel_hz
    }

    /// How many times the rate has changed.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn switches(&self) -> u32 {
        self.switches
    }

    fn budget(hz: f32) -> Duration {
        Duration::from_secs_f32(1.0 / hz)
    }

    /// One drawn frame's `work` (without its wait for a buffer), finished at
    /// `now`. Returns the new rate when this frame decided a change.
    pub(crate) fn observe(&mut self, work: Duration, now: Duration) -> Option<f32> {
        let gap = self.last.map(|last| now.saturating_sub(last));
        self.last = Some(now);
        let Some(gap) = gap.filter(|&g| g < IDLE_RESET) else {
            // The first frame, or the first after a pause: no interval to
            // judge, and the evidence so far describes another stretch.
            self.window.clear();
            self.gaps.clear();
            self.over_windows = 0;
            self.first.get_or_insert(now);
            return None;
        };
        if now.saturating_sub(*self.first.get_or_insert(now)) < WARM_UP {
            return None;
        }
        let at_panel = self.target_hz >= self.panel_hz;
        if !at_panel {
            self.at_fallback += gap;
        }
        self.window.push(work);
        self.gaps.push(gap);
        if self.window.len() < self.window_len {
            return None;
        }
        let panel_budget = Self::budget(self.panel_hz);
        let late_after = Self::budget(self.target_hz).as_secs_f64() * LATE_FACTOR;
        let late = self
            .gaps
            .iter()
            .filter(|g| g.as_secs_f64() > late_after)
            .count();
        self.window.sort_unstable();
        let pick = |w: &[Duration], q: usize| w[(w.len() * q / 100).min(w.len() - 1)];
        let p50 = pick(&self.window, 50);
        let p95 = pick(&self.window, 95);
        self.evidence = format!(
            "last {} frames: {late} missed a vsync at {:.0} Hz; own work p50 {:.2} / p95 {:.2} ms \
             against the panel's {:.2} ms",
            self.window.len(),
            self.target_hz,
            p50.as_secs_f64() * 1e3,
            p95.as_secs_f64() * 1e3,
            panel_budget.as_secs_f64() * 1e3,
        );
        let decision = if at_panel {
            #[expect(clippy::cast_precision_loss, reason = "frame counts")]
            let share = late as f64 / self.window.len() as f64;
            if share > MAX_OVER_SHARE {
                self.over_windows += 1;
            } else {
                self.over_windows = 0;
            }
            (self.over_windows >= DOWN_WINDOWS).then_some(FALLBACK_HZ)
        } else {
            if p95.as_secs_f64() <= panel_budget.as_secs_f64() * UP_HEADROOM {
                self.clear_windows += 1;
            } else {
                self.clear_windows = 0;
            }
            let probe = self.at_fallback >= self.probe_wait;
            if probe && self.clear_windows < UP_WINDOWS {
                self.evidence.push_str("; probing the panel's rate");
                self.probed_at = Some(now);
            } else {
                self.probed_at = None;
            }
            (self.clear_windows >= UP_WINDOWS || probe).then_some(self.panel_hz)
        };
        self.window.clear();
        self.gaps.clear();
        let hz = decision?;
        if hz < self.target_hz {
            // Down: a probe that could not hold doubles the next wait;
            // anything else starts the waits again.
            let failed = self
                .probed_at
                .is_some_and(|at| now.saturating_sub(at) < PROBE_FAILED_WITHIN);
            self.probe_wait = if failed {
                (self.probe_wait * 2).min(PROBE_MAX)
            } else {
                PROBE_FIRST
            };
            self.probed_at = None;
        }
        self.target_hz = hz;
        self.at_fallback = Duration::ZERO;
        self.clear_windows = 0;
        self.over_windows = 0;
        self.switches += 1;
        Some(hz)
    }

    /// The wait before the next probe, for tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn probe_wait(&self) -> Duration {
        self.probe_wait
    }
}

/// A frame's own work for [`Cadence::observe`]: `work` without the time spent
/// waiting on the display, which is the pacing itself.
///
/// - `acquire` waits for a buffer to render into.
/// - `gpu_wait` waits on the GPU, and with FIFO presentation also on a
///   swapchain image coming back from the display.
///
/// Counting either pins a lowered rate down. At 60 Hz every frame then looks
/// like it needs the whole 16.7 ms, so the rate never comes back up. With
/// MAILBOX nothing waited; with FIFO (run 43889062992) the GPU path's "work"
/// went from 3.8 ms to 13-16 ms on the 120 Hz Samsungs and dropped them to
/// 60 Hz for good.
///
/// The GPU's own busy time (`gpu_busy`, from timestamp queries) is the floor,
/// so a frame the GPU really cannot finish in budget still counts as late.
#[must_use]
pub(crate) fn own_work(
    work: Duration,
    acquire: Duration,
    gpu_wait: Duration,
    gpu_busy: Option<Duration>,
) -> Duration {
    let cpu = work.saturating_sub(acquire).saturating_sub(gpu_wait);
    gpu_busy.map_or(cpu, |busy| cpu.max(busy))
}

/// The frame budget to give the performance hint (`crate::perf_hint`): the
/// panel's while there is a panel rate to get back to, whatever the cadence
/// holds now.
///
/// Following the cadence's lowered budget closed a loop. At 60 Hz the hint
/// said 16.7 ms was fine, so Android let the clocks fall and the raster
/// took twice as long. The cadence, which goes back up only when the 95th
/// percentile fits 75% of the panel's budget, then never could. Run
/// 43870118033's SM8750 drew one leg at 120 fps with a 3.84 ms raster and
/// the next leg, which fell to 60 Hz, at 7.68 ms: the same phone, build and
/// frames.
#[must_use]
pub(crate) fn hint_budget(current: Duration, panel_hz: Option<f32>) -> Duration {
    match panel_hz {
        Some(hz) if hz.is_finite() && hz > 0.0 => current.min(Duration::from_secs_f32(1.0 / hz)),
        _ => current,
    }
}

/// Whether adaptation is switched off for this process (`VIEWW_CADENCE=panel`).
#[must_use]
pub(crate) fn disabled() -> bool {
    std::env::var("VIEWW_CADENCE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("panel"))
}

/// Ask the platform to show this window at `hz`.
///
/// Android 11+ (`ANativeWindow_setFrameRate`, looked up at run time so the
/// library still loads on older phones, where this does nothing and the
/// window's own pacing holds the rate). Returns what happened, for the log.
#[cfg(target_os = "android")]
pub(crate) fn request(window: &winit::window::Window, hz: f32) -> &'static str {
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    type SetFrameRate = unsafe extern "C" fn(*mut c_void, f32, i8) -> i32;
    type SetFrameRateWithStrategy = unsafe extern "C" fn(*mut c_void, f32, i8, i8) -> i32;
    struct Functions {
        plain: Option<SetFrameRate>,
        strategy: Option<SetFrameRateWithStrategy>,
    }
    static FUNCTIONS: OnceLock<Functions> = OnceLock::new();
    let functions = FUNCTIONS.get_or_init(|| {
        // SAFETY: `dlopen` of a system library by name, never closed.
        let library = unsafe { libc::dlopen(c"libandroid.so".as_ptr(), libc::RTLD_NOW) };
        if library.is_null() {
            return Functions {
                plain: None,
                strategy: None,
            };
        }
        // SAFETY: `library` is live and the names are NUL-terminated.
        let (plain, strategy) = unsafe {
            (
                libc::dlsym(library, c"ANativeWindow_setFrameRate".as_ptr()),
                libc::dlsym(
                    library,
                    c"ANativeWindow_setFrameRateWithChangeStrategy".as_ptr(),
                ),
            )
        };
        // SAFETY: the NDK's signatures (android/native_window.h, API 30 and 31).
        unsafe {
            Functions {
                plain: (!plain.is_null())
                    .then(|| std::mem::transmute::<*mut c_void, SetFrameRate>(plain)),
                strategy: (!strategy.is_null()).then(|| {
                    std::mem::transmute::<*mut c_void, SetFrameRateWithStrategy>(strategy)
                }),
            }
        }
    });
    let Ok(handle) = window.window_handle() else {
        return "not asked (no window handle)";
    };
    let RawWindowHandle::AndroidNdk(handle) = handle.as_raw() else {
        return "not asked (not an Android window)";
    };
    let native = handle.a_native_window.as_ptr();
    // ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT: the app's own content
    // rate, which lets the compositor pick a panel mode that divides it.
    const COMPATIBILITY_DEFAULT: i8 = 0;
    // ANATIVEWINDOW_CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS: never a visible
    // mode switch. ALWAYS was tried for one run (43912018582) on a misread:
    // the SM8850's 60 Hz there was Vieww's own cadence dropping (its log
    // says so), not the panel refusing, and the plain, seamless-only call
    // had already given it 130 fps scroll (run 43907619201). A switch that
    // is not seamless can blank the panel for a frame on some phones, a
    // price the UI never pays for a frame rate.
    const ONLY_IF_SEAMLESS: i8 = 0;
    if let Some(set) = functions.strategy {
        // SAFETY: a live `ANativeWindow*` from winit, owned by the window
        // that is borrowed for this call.
        let status = unsafe { set(native, hz, COMPATIBILITY_DEFAULT, ONLY_IF_SEAMLESS) };
        return if status == 0 {
            "asked (ANativeWindow_setFrameRateWithChangeStrategy, seamless only)"
        } else {
            "refused (ANativeWindow_setFrameRateWithChangeStrategy returned an error)"
        };
    }
    let Some(set) = functions.plain else {
        return "not asked (Android 10 or older: no ANativeWindow_setFrameRate)";
    };
    // SAFETY: as above.
    let status = unsafe { set(native, hz, COMPATIBILITY_DEFAULT) };
    if status == 0 {
        "asked (ANativeWindow_setFrameRate)"
    } else {
        "refused (ANativeWindow_setFrameRate returned an error)"
    }
}

/// Ask the platform to show this window at `hz`: nothing to ask off Android.
/// iOS follows the rate through its display link (`crate::ios`), desktops
/// present at the monitor's rate and the window paces itself.
#[cfg(not(target_os = "android"))]
pub(crate) const fn request(_window: &winit::window::Window, _hz: f32) -> &'static str {
    "not asked (no platform frame-rate request here; paced by the window)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_work_leaves_out_display_waits_but_not_gpu_time() {
        // A FIFO frame at 60 Hz: 16 ms of work, 12 of it waiting on the
        // display, the GPU busy 3 ms: 4 ms of its own.
        assert_eq!(
            own_work(ms(16.0), ms(1.0), ms(11.0), Some(ms(3.0))),
            ms(4.0)
        );
        // A GPU-bound frame: the GPU's 10 ms is the floor.
        assert_eq!(
            own_work(ms(16.0), ms(1.0), ms(11.0), Some(ms(10.0))),
            ms(10.0)
        );
        // No timestamps: the CPU side alone.
        assert_eq!(own_work(ms(16.0), ms(1.0), ms(11.0), None), ms(4.0));
        // Waits never take it below zero.
        assert_eq!(own_work(ms(2.0), ms(3.0), ms(1.0), None), Duration::ZERO);
    }

    #[test]
    fn the_hint_keeps_the_panels_budget() {
        let at_60 = Duration::from_secs_f32(1.0 / 60.0);
        let panel = Duration::from_secs_f32(1.0 / 120.0);
        assert_eq!(hint_budget(at_60, Some(120.0)), panel);
        assert_eq!(hint_budget(at_60, None), at_60);
        assert_eq!(hint_budget(panel, Some(120.0)), panel);
    }

    #[test]
    fn the_starting_rate_is_asked_for_once() {
        let mut c = Cadence::new(120.0).unwrap();
        assert_eq!(c.initial_request(), Some(120.0));
        assert_eq!(c.initial_request(), None);
    }

    fn ms(v: f64) -> Duration {
        Duration::from_secs_f64(v / 1000.0)
    }

    /// Feed `frames` frames of `work`; return every decision made. Each
    /// frame lands on the display's next vsync at the current rate that its
    /// work allows: a frame that takes 1.2 budgets is shown a vsync late.
    /// That is a display with no overlap between frames, the pessimistic
    /// case.
    fn run(
        c: &mut Cadence,
        start: &mut Duration,
        frames: usize,
        work: impl Fn(usize) -> f64,
    ) -> Vec<f32> {
        let mut out = Vec::new();
        for i in 0..frames {
            let budget = 1000.0 / f64::from(c.target_hz());
            let w = work(i);
            let vsyncs = (w / budget).ceil().max(1.0);
            *start += ms(vsyncs * budget);
            if let Some(hz) = c.observe(ms(w), *start) {
                out.push(hz);
            }
        }
        out
    }

    #[test]
    fn a_60_hz_panel_has_nothing_to_adapt() {
        assert!(Cadence::new(60.0).is_none());
        assert!(Cadence::new(59.94).is_none());
        assert!(Cadence::new(f32::NAN).is_none());
        assert!(Cadence::new(90.0).is_some());
    }

    /// Past [`WARM_UP`], where the evidence starts.
    fn warmed(c: &mut Cadence, t: &mut Duration) {
        run(c, t, 120, |_| 40.0);
        assert!(
            (c.target_hz() - c.panel_hz()).abs() < f32::EPSILON,
            "start-up is not evidence"
        );
    }

    #[test]
    fn a_slow_start_up_is_not_a_reason_to_drop() {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        // A second of 40 ms frames — a first raster, pipelines — then a
        // phone that holds its rate.
        warmed(&mut c, &mut t);
        let decisions = run(&mut c, &mut t, 600, |_| 6.0);
        assert!(decisions.is_empty(), "{decisions:?}");
    }

    #[test]
    fn a_phone_that_holds_120_stays_at_120() {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        warmed(&mut c, &mut t);
        // Every 25th frame over: 4%, inside the allowance.
        let decisions = run(
            &mut c,
            &mut t,
            1200,
            |i| if i % 25 == 0 { 9.5 } else { 6.0 },
        );
        assert!(decisions.is_empty(), "{decisions:?}");
        assert!((c.target_hz() - 120.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_a73s_ragged_120_falls_to_a_steady_60() {
        // Run 39402544760's auto pass: median 7.7 ms, 22% over 8.33 ms.
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        warmed(&mut c, &mut t);
        let decisions = run(&mut c, &mut t, 400, |i| if i % 9 < 2 { 10.0 } else { 7.5 });
        assert_eq!(decisions, vec![60.0]);
        assert_eq!(c.switches(), 1);
    }

    /// Down to 60 Hz with sustained 12 ms frames, stopping at the drop.
    fn dropped(c: &mut Cadence, t: &mut Duration) {
        warmed(c, t);
        for _ in 0..120 * 4 {
            *t += ms(2.0 * 1000.0 / 120.0);
            if c.observe(ms(12.0), *t) == Some(60.0) {
                return;
            }
        }
        panic!("sustained 12 ms frames must drop a 120 Hz panel to 60");
    }

    #[test]
    fn room_to_spare_at_60_comes_back_up() {
        // A window is the panel's frame count (120), two seconds at 60 Hz:
        // comfortably-under work is back at 120 within three of them, by the
        // fast path or the probe, whichever is first.
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        dropped(&mut c, &mut t);
        assert_eq!(run(&mut c, &mut t, 120 * 3, |_| 5.0), vec![120.0]);
        assert!(run(&mut c, &mut t, 120 * 10, |_| 5.0).is_empty());
    }

    #[test]
    fn a_probe_finds_a_rate_that_work_at_60_hz_hid() {
        // 8 ms at 60 Hz is not 25% under 8.33, so the fast path never fires;
        // at 120 Hz the same 8 ms makes every vsync. The probe finds it.
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        dropped(&mut c, &mut t);
        let decisions = run(&mut c, &mut t, 60 * 6, |_| 8.0);
        assert_eq!(decisions, vec![120.0], "a probe within about 5 s");
        let decisions = run(&mut c, &mut t, 120 * 10, |_| 8.0);
        assert!(decisions.is_empty(), "and it holds: {decisions:?}");
    }

    #[test]
    fn a_failed_probe_waits_longer_for_the_next() {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        dropped(&mut c, &mut t);
        assert_eq!(c.probe_wait(), PROBE_FIRST);
        // Really 12 ms: the probe goes up, misses, comes down.
        let decisions = run(&mut c, &mut t, 60 * 6 + 120 * 4, |_| 12.0);
        assert_eq!(decisions, vec![120.0, 60.0]);
        assert_eq!(c.probe_wait(), PROBE_FIRST * 2);
        // And backs off to the cap, never past it.
        for _ in 0..8 {
            run(&mut c, &mut t, 60 * 70 + 120 * 4, |_| 12.0);
        }
        assert_eq!(c.probe_wait(), PROBE_MAX);
    }

    #[test]
    fn one_heavy_second_is_not_a_reason_to_drop() {
        // A data load: a second of frames over budget, then work that fits.
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        warmed(&mut c, &mut t);
        let decisions = run(
            &mut c,
            &mut t,
            120 * 5,
            |i| if i < 120 { 20.0 } else { 4.0 },
        );
        assert!(decisions.is_empty(), "{decisions:?}");
        assert!((c.target_hz() - 120.0).abs() < f32::EPSILON);
    }

    /// Market Watch's schedule (load 2 s, idle 2 s, tick 1 s, jump 1 s, then
    /// scroll, live and storm 8 s each), played with frame intervals drawn
    /// from measured quantiles (p50 / p90 / p95 / p99, ms) and own work.
    fn replay(phases: &[(f64, [f64; 4], f64)]) -> (Vec<f32>, f32) {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        let mut decisions = Vec::new();
        for &(seconds, q, work) in phases {
            if seconds == 0.0 {
                t += Duration::from_secs(2); // the idle phase: no frames
                continue;
            }
            let end = t + Duration::from_secs_f64(seconds);
            let mut i = 0_u32;
            while t < end {
                // 50% at p50, 40% at p90, 5% at p95, 5% at p99.
                let k = i % 100;
                let gap = if k < 50 {
                    q[0]
                } else if k < 90 {
                    q[1]
                } else if k < 95 {
                    q[2]
                } else {
                    q[3]
                };
                // At 60 Hz the display paces every frame to 16.67 ms.
                let gap = gap.max(1000.0 / f64::from(c.target_hz()));
                t += ms(gap);
                decisions.extend(c.observe(ms(work), t));
                i += 1;
            }
        }
        (decisions, c.target_hz())
    }

    #[test]
    fn market_watch_on_phones_that_hold_120_stays_at_120() {
        let burst = [25.0, 30.0, 40.0, 60.0];
        // SM8750's CPU path, run 43879609229 (131.5 / 127.5 / 123.5 fps,
        // jank 0.3 / 0.8 / 0.1%): scroll intervals as measured; live and
        // storm at the fps they presented with the same tail.
        let sm8750_cpu = [
            (2.0, burst, 20.0),
            (0.0, burst, 0.0),
            (1.0, burst, 20.0),
            (1.0, burst, 40.0),
            (8.0, [8.334, 8.389, 8.786, 10.609], 6.0),
            (8.0, [8.334, 8.40, 8.9, 16.7], 7.0),
            (8.0, [8.334, 8.40, 8.9, 16.7], 7.5),
        ];
        let (decisions, hz) = replay(&sm8750_cpu);
        assert!(
            decisions.is_empty() && (hz - 120.0).abs() < f32::EPSILON,
            "{decisions:?}"
        );
        // SM8850's CPU path, run 43907619201's scroll at 130.3 fps.
        let sm8850_cpu_scroll = [
            (2.0, burst, 20.0),
            (0.0, burst, 0.0),
            (1.0, burst, 20.0),
            (1.0, burst, 40.0),
            (8.0, [8.334, 8.393, 9.337, 11.091], 6.3),
        ];
        let (decisions, hz) = replay(&sm8850_cpu_scroll);
        assert!(
            decisions.is_empty() && (hz - 120.0).abs() < f32::EPSILON,
            "{decisions:?}"
        );
    }

    #[test]
    fn market_watch_on_a_phone_that_cannot_hold_120_settles_at_60() {
        // Every third frame a vsync late, sustained: the A73's ragged 120.
        let burst = [25.0, 30.0, 40.0, 60.0];
        let ragged = [16.7, 16.7, 16.7, 16.7];
        let phases = [
            (2.0, burst, 20.0),
            (0.0, burst, 0.0),
            (1.0, burst, 20.0),
            (1.0, burst, 40.0),
            (8.0, [8.334, ragged[0], ragged[0], ragged[0]], 10.0),
            (8.0, [8.334, ragged[0], ragged[0], ragged[0]], 11.0),
        ];
        let (decisions, hz) = replay(&phases);
        assert_eq!(decisions.first(), Some(&60.0), "{decisions:?}");
        // Probes may go up and come back; it must end where it can hold.
        assert!((hz - 60.0).abs() < f32::EPSILON, "{decisions:?}");
    }

    #[test]
    fn evidence_carries_across_the_pauses_between_bursts() {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        warmed(&mut c, &mut t);
        // Bursts of 30 frames with 250 ms between them — taps and flicks —
        // and 3 of every 30 late (10%, twice the allowance). The window
        // fills across bursts, so the rate drops within a few of them.
        let mut decisions = Vec::new();
        for _ in 0..20 {
            decisions.extend(run(&mut c, &mut t, 30, |i| {
                if i % 10 == 0 {
                    12.0
                } else {
                    6.0
                }
            }));
            t += Duration::from_millis(250);
        }
        assert_eq!(decisions.first().copied(), Some(60.0), "{decisions:?}");
    }

    #[test]
    fn a_pause_in_the_animation_starts_the_evidence_again() {
        let mut c = Cadence::new(120.0).unwrap();
        let mut t = Duration::ZERO;
        warmed(&mut c, &mut t);
        // Half a window of slow frames, a pause, then half a window of fast
        // ones: no single stretch of work was a whole window, so no verdict.
        run(&mut c, &mut t, 60, |_| 12.0);
        t += Duration::from_secs(2);
        let decisions = run(&mut c, &mut t, 60, |_| 5.0);
        assert!(decisions.is_empty(), "{decisions:?}");
        assert!((c.target_hz() - 120.0).abs() < f32::EPSILON);
    }
}
