//! Per-app device runs (CI-01): any app, launched by a phone farm, measures
//! itself through a fixed scenario and narrates it in Market Watch's protocol.
//!
//! # What a run is
//!
//! Firebase Test Lab launches an app as a *Game Loop* test
//! (`com.google.intent.action.TEST_LOOP`, with a `scenario` number). An app
//! that hands its [`App`] to `Farm::from_launch` (Android) then plays the scenario on
//! its own and exits:
//!
//! | phase | what happens | what it measures |
//! |---|---|---|
//! | (start) | the first frame | cold start: process start to first present |
//! | `scroll` | synthetic finger strokes through the middle of the screen | frame work, presented fps, jank while scrolling |
//! | `sheet` | the app's own sheet opens, then closes (`Farm::sheet`) | the same, over a modal's entrance and exit |
//! | `idle` | nothing | the frames an idle screen still presents (should be ~0) |
//!
//! Scenario 1 runs on the CPU rasteriser, scenario 2 on the GPU path
//! ([`App::prefer_gpu`]). The tier, memory (PSS) and per-phase CPU time are
//! read by the app, because Test Lab has no shell.
//!
//! # The protocol
//!
//! One logcat line per event, `__MW__ {json}`, exactly the lines
//! `examples/market-watch/firebase/to_farm.py` and `compare_report.py`
//! already read: `device`, `start`, `phase`, `stats`, `proc`, `first_frame`,
//! `quality`, `mem`, `done`. So the per-app report, `compare.json` and the
//! scorecard (`tools/make_scorecard.py`) need no reader of their own.
//!
//! # Not a launcher run
//!
//! Started any other way (the home screen, `adb shell am start`)
//! `Farm::from_launch` (Android) returns `None` and the app runs as it always does.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vieww_render::element::Animation;
use vieww_render::foundation::{Offset, PointerEvent, PointerId};
use vieww_render::FrameDriver;
use vieww_widget::animation::Tween;

use crate::stats::FrameLog;

/// What a phase does to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseKind {
    /// Synthetic finger strokes; the loop is kept drawing.
    Scroll,
    /// The app's sheet opens at the start and closes halfway.
    Sheet,
    /// Nothing at all: the loop may sleep, and should.
    Idle,
}

/// One step of the scenario.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phase {
    pub name: &'static str,
    pub kind: PhaseKind,
    pub seconds: f32,
}

/// The scenario every app plays (DESIGN of CI-01: cold start, scroll, open a
/// sheet, 10 s idle). About 22 s with the settle, well inside Test Lab's
/// per-phone timeout, and twice per phone (CPU, GPU).
#[must_use]
pub fn app_schedule() -> Vec<Phase> {
    vec![
        Phase {
            name: "scroll",
            kind: PhaseKind::Scroll,
            seconds: 6.0,
        },
        Phase {
            name: "sheet",
            kind: PhaseKind::Sheet,
            seconds: 4.0,
        },
        Phase {
            name: "idle",
            kind: PhaseKind::Idle,
            seconds: 10.0,
        },
    ]
}

/// p50, p95, p99 and max of `samples` (zeros for none).
#[must_use]
pub fn summary(samples: &[f64]) -> (f64, f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a phase holds far fewer than 2^52 frames"
    )]
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    (at(0.5), at(0.95), at(0.99), v[v.len() - 1])
}

/// A synthetic finger: where it is and which way it is going.
#[derive(Debug, Clone, Copy)]
struct Stroke {
    from: Offset,
    at: Offset,
}

/// The scenario as it runs: a state machine the frame hooks drive.
///
/// Platform-free, so a headless test can play it against a `FrameDriver`:
/// the Android half (`Farm`) only supplies the clock thread, the sink and
/// the way out.
pub struct FarmRun {
    start: Instant,
    phases: Vec<Phase>,
    /// The phase the clock has moved to; `current` catches up before a frame.
    target: Arc<AtomicUsize>,
    current: Option<usize>,
    phase_started: Option<Instant>,
    keepalive: Option<Animation<f32>>,
    work: Vec<f64>,
    wait: Vec<f64>,
    interval: Vec<f64>,
    last_present: Option<Instant>,
    seen_frames: u64,
    budget_ms: f64,
    phase_ticks: Option<u64>,
    first_frame_emitted: bool,
    sheet_closed: bool,
    stroke: Option<Stroke>,
    strokes: u32,
    done: bool,
    emit: Box<dyn FnMut(&str)>,
    sheet: Option<Box<dyn FnMut(bool)>>,
    finish: Option<Box<dyn FnMut()>>,
}

impl std::fmt::Debug for FarmRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FarmRun")
            .field("current", &self.current)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

/// A JSON number, or `null` for a reading the device refused.
fn num<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| "null".to_owned(), |v| v.to_string())
}

impl FarmRun {
    /// A run of `phases`, narrating through `emit` (one JSON object per call,
    /// without the `__MW__ ` prefix).
    #[must_use]
    pub fn new(phases: Vec<Phase>, emit: impl FnMut(&str) + 'static) -> Self {
        Self {
            start: Instant::now(),
            phases,
            target: Arc::new(AtomicUsize::new(0)),
            current: None,
            phase_started: None,
            keepalive: None,
            work: Vec::new(),
            wait: Vec::new(),
            interval: Vec::new(),
            last_present: None,
            seen_frames: 0,
            budget_ms: 0.0,
            phase_ticks: None,
            first_frame_emitted: false,
            sheet_closed: false,
            stroke: None,
            strokes: 0,
            done: false,
            emit: Box::new(emit),
            sheet: None,
            finish: None,
        }
    }

    /// The app's sheet: called with `true` to open it, `false` to close it.
    #[must_use]
    pub fn with_sheet(mut self, sheet: impl FnMut(bool) + 'static) -> Self {
        self.sheet = Some(Box::new(sheet));
        self
    }

    /// What to do once the scenario is over (end the Game Loop test).
    #[must_use]
    pub fn with_finish(mut self, finish: impl FnMut() + 'static) -> Self {
        self.finish = Some(Box::new(finish));
        self
    }

    /// The handle the clock moves: phase `i` starts when it reads `i`, and
    /// the run ends at `phases.len()`.
    #[must_use]
    pub fn clock(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.target)
    }

    /// Each phase's length, for the clock.
    #[must_use]
    pub fn seconds(&self) -> Vec<f32> {
        self.phases.iter().map(|p| p.seconds).collect()
    }

    /// Whether the scenario has finished.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.done
    }

    fn t(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1e3
    }

    fn say(&mut self, line: &str) {
        (self.emit)(line);
    }

    /// Close the phase that just ended: its `stats` and `proc` lines.
    fn flush(&mut self) {
        let Some(i) = self.current else { return };
        let name = self.phases[i].name;
        let (wm, w95, w99, wmax) = summary(&self.work);
        let (tm, t95, t99, tmax) = summary(&self.wait);
        let (im, i95, i99, imax) = summary(&self.interval);
        let budget = self.budget_ms;
        let jank = self
            .interval
            .iter()
            .filter(|&&v| budget > 0.0 && v > budget * 1.5)
            .count();
        let line = format!(
            "{{\"fw\":\"vieww\",\"ev\":\"stats\",\"phase\":\"{name}\",\"frames\":{},\
\"work_ms\":[{wm:.3},{w95:.3},{w99:.3},{wmax:.3}],\"wait_ms\":[{tm:.3},{t95:.3},{t99:.3},{tmax:.3}],\
\"interval_ms\":[{im:.3},{i95:.3},{i99:.3},{imax:.3}],\"jank\":{jank},\"budget_ms\":{budget:.2},\"t\":{:.1}}}",
            self.work.len(),
            self.t()
        );
        self.say(&line);
        let ticks = proc_self::cpu_ticks();
        let delta = match (self.phase_ticks, ticks) {
            (Some(a), Some(b)) => Some(b.saturating_sub(a)),
            _ => None,
        };
        let line = format!(
            "{{\"fw\":\"vieww\",\"ev\":\"proc\",\"phase\":\"{name}\",\"cpu_ticks\":{},\"rss_kb\":{},\"batt_temp_dC\":{},\"t\":{:.1}}}",
            num(delta),
            num(proc_self::rollup_kb("Rss:")),
            num(proc_self::battery_temp_dc()),
            self.t()
        );
        self.say(&line);
        self.work.clear();
        self.wait.clear();
        self.interval.clear();
    }

    /// Before every frame: follow the clock, then apply this frame's input.
    pub fn before(&mut self, driver: &mut FrameDriver) {
        if self.done {
            return;
        }
        let target = self.target.load(Ordering::Acquire);
        if self.current != Some(target) {
            self.flush();
            self.end_stroke(driver);
            if let (Some(i), false) = (self.current, self.sheet_closed) {
                if self.phases[i].kind == PhaseKind::Sheet {
                    self.set_sheet(false);
                }
            }
            if target >= self.phases.len() {
                self.keepalive = None;
                self.done = true;
                let line = format!(
                    "{{\"fw\":\"vieww\",\"ev\":\"mem\",\"pss_kb\":{},\"rss_kb\":{},\"t\":{:.1}}}",
                    num(proc_self::rollup_kb("Pss:")),
                    num(proc_self::rollup_kb("Rss:")),
                    self.t()
                );
                self.say(&line);
                let line = format!("{{\"fw\":\"vieww\",\"ev\":\"done\",\"t\":{:.1}}}", self.t());
                self.say(&line);
                if let Some(finish) = self.finish.as_mut() {
                    finish();
                }
                return;
            }
            self.current = Some(target);
            self.phase_started = Some(Instant::now());
            self.phase_ticks = proc_self::cpu_ticks();
            self.last_present = None;
            self.sheet_closed = false;
            let phase = self.phases[target];
            let line = format!(
                "{{\"fw\":\"vieww\",\"ev\":\"phase\",\"phase\":\"{}\",\"t\":{:.1}}}",
                phase.name,
                self.t()
            );
            self.say(&line);
            // Scroll and sheet keep the loop drawing so every frame is a
            // sample; idle must be free to sleep — that is what it measures.
            self.keepalive = (phase.kind != PhaseKind::Idle).then(|| {
                let a = driver.animation(Tween::new(0.0_f32, 1.0), Duration::from_secs(1));
                a.repeat(true, Duration::ZERO);
                a
            });
            if phase.kind == PhaseKind::Sheet {
                self.set_sheet(true);
            }
        }
        let Some(i) = self.current else { return };
        let phase = self.phases[i];
        match phase.kind {
            PhaseKind::Scroll => self.stroke_step(driver),
            PhaseKind::Sheet => {
                let half = Duration::from_secs_f32(phase.seconds / 2.0);
                if !self.sheet_closed && self.phase_started.is_some_and(|t| t.elapsed() >= half) {
                    self.set_sheet(false);
                }
            }
            PhaseKind::Idle => {}
        }
    }

    fn set_sheet(&mut self, open: bool) {
        if !open {
            self.sheet_closed = true;
        }
        if let Some(sheet) = self.sheet.as_mut() {
            sheet(open);
        }
    }

    /// One frame of the synthetic finger: down, a run of moves, up, and the
    /// next stroke. Half a screen per stroke in 0.4 s or so, four strokes one
    /// way and four back, so a list of any length keeps moving and never
    /// sits against an end.
    fn stroke_step(&mut self, driver: &mut FrameDriver) {
        let r = driver.surface();
        let (w, h) = (r.width(), r.height());
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let up = (self.strokes / 4).is_multiple_of(2);
        let now = self.start.elapsed();
        let finger = PointerId(7);
        match self.stroke {
            None => {
                let y = if up { h * 0.75 } else { h * 0.25 };
                let from = Offset::new(w * 0.5, y);
                driver.handle_pointer(&PointerEvent::down(finger, from, now));
                self.stroke = Some(Stroke { from, at: from });
            }
            Some(Stroke { from, at }) => {
                let step = h * 0.045 * if up { -1.0 } else { 1.0 };
                let next = Offset::new(at.dx, at.dy + step);
                driver.handle_pointer(&PointerEvent::moved(finger, at, next, now));
                if (next.dy - from.dy).abs() >= h * 0.5 {
                    driver.handle_pointer(&PointerEvent::up(finger, next, now));
                    self.stroke = None;
                    self.strokes += 1;
                } else {
                    self.stroke = Some(Stroke { from, at: next });
                }
            }
        }
    }

    /// Lift a finger still down when a phase ends, so no gesture leaks into
    /// the next one.
    fn end_stroke(&mut self, driver: &mut FrameDriver) {
        if let Some(Stroke { at, .. }) = self.stroke.take() {
            driver.handle_pointer(&PointerEvent::up(PointerId(7), at, self.start.elapsed()));
        }
    }

    /// After every frame hook call: the first-frame line once, then one
    /// sample per newly presented frame.
    pub fn presented(&mut self, log: &FrameLog) {
        if !self.first_frame_emitted && log.frames() > 0 {
            self.first_frame_emitted = true;
            let line = format!(
                "{{\"fw\":\"vieww\",\"ev\":\"first_frame\",\"since_process_ms\":{},\"t\":{:.1}}}",
                num(proc_self::since_process_start_ms().map(|ms| format!("{ms:.1}"))),
                self.t()
            );
            self.say(&line);
            // The tier and the raster policy these frames ran with: the
            // scorecard files the run under the tier this line names.
            let line = format!(
                "{{\"fw\":\"vieww\",\"ev\":\"quality\",\"line\":\"{}\",\"t\":{:.1}}}",
                crate::cache_budgets_status().replace('"', "\\\""),
                self.t()
            );
            self.say(&line);
        }
        if self.budget_ms == 0.0 {
            self.budget_ms = log.budget().as_secs_f64() * 1e3;
        }
        if self.done || self.current.is_none() || log.frames() <= self.seen_frames {
            return;
        }
        self.seen_frames = log.frames();
        if let Some(w) = log.work_samples().last() {
            self.work.push(w.as_secs_f64() * 1e3);
            if let Some(wait) = log.wait_samples().last() {
                self.wait.push(wait.as_secs_f64() * 1e3);
            }
        }
        let now = Instant::now();
        if let Some(prev) = self.last_present.replace(now) {
            self.interval.push((now - prev).as_secs_f64() * 1e3);
        }
    }
}

/// `/proc/self` readings: what a device runner reads over `adb` and a farm
/// app must read for itself. `None` wherever the platform has no `/proc` or
/// refuses the read.
mod proc_self {
    fn stat_fields() -> Option<Vec<u64>> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let rest = stat.rsplit_once(") ")?.1;
        Some(
            rest.split_whitespace()
                .map(|f| f.parse().unwrap_or(0))
                .collect(),
        )
    }

    /// utime + stime so far, in clock ticks.
    pub(super) fn cpu_ticks() -> Option<u64> {
        let f = stat_fields()?;
        Some(f.get(11)? + f.get(12)?)
    }

    /// One `kB` line of `/proc/self/smaps_rollup` (`Pss:`, `Rss:`).
    pub(super) fn rollup_kb(key: &str) -> Option<u64> {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
        text.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    /// The battery temperature in tenths of a degree, where an app may read it.
    pub(super) fn battery_temp_dc() -> Option<i64> {
        std::fs::read_to_string("/sys/class/power_supply/battery/temp")
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Milliseconds since this process started (Test Lab's launch is cold,
    /// so at the first frame this is the cold start). Both clocks are the
    /// boot-time clock, read directly: `/proc/uptime` is not readable by an
    /// app on every Android release.
    #[cfg(any(target_os = "android", target_os = "linux"))]
    pub(super) fn since_process_start_ms() -> Option<f64> {
        #[repr(C)]
        struct Timespec {
            tv_sec: i64,
            tv_nsec: i64,
        }
        extern "C" {
            fn clock_gettime(clock: i32, out: *mut Timespec) -> i32;
        }
        const CLOCK_BOOTTIME: i32 = 7;
        let start_ticks = *stat_fields()?.get(19)?;
        let mut now = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid, writable `timespec` for the call.
        if unsafe { clock_gettime(CLOCK_BOOTTIME, &mut now) } != 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a boot time is far below 2^52 ns"
        )]
        let up_s = now.tv_sec as f64 + now.tv_nsec as f64 / 1e9;
        #[expect(clippy::cast_precision_loss, reason = "ticks since boot are small")]
        let started = start_ticks as f64 / 100.0;
        Some(((up_s - started) * 1e3).max(0.0))
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    pub(super) fn since_process_start_ms() -> Option<f64> {
        None
    }
}

/// A Game Loop launch, and the scenario it asked for.
#[cfg(target_os = "android")]
pub struct Farm {
    android: crate::AndroidApp,
    scenario: i32,
    app: &'static str,
    sheet: Option<Box<dyn FnMut(bool)>>,
}

#[cfg(target_os = "android")]
impl std::fmt::Debug for Farm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Farm")
            .field("app", &self.app)
            .field("scenario", &self.scenario)
            .finish_non_exhaustive()
    }
}

#[cfg(target_os = "android")]
impl Farm {
    /// `Some` when Test Lab launched this activity as a Game Loop test;
    /// `None` for every other launch.
    #[must_use]
    pub fn from_launch(android: &crate::AndroidApp, app: &'static str) -> Option<Self> {
        let scenario = game_loop::scenario(android)?;
        Some(Self {
            android: android.clone(),
            scenario,
            app,
            sheet: None,
        })
    }

    /// The app's sheet for the `sheet` phase: `true` opens it, `false`
    /// closes it. An app without one leaves this unset and the phase
    /// measures a still screen.
    #[must_use]
    pub fn sheet(mut self, sheet: impl FnMut(bool) + 'static) -> Self {
        self.sheet = Some(Box::new(sheet));
        self
    }

    /// Install the scenario on `app`: the hooks, the clock, the path the
    /// scenario names (1 CPU, 2 GPU), and the exit once it is done.
    #[must_use]
    pub fn install(self, app: crate::App) -> crate::App {
        use std::cell::RefCell;
        use std::rc::Rc;
        use vieww_render::foundation::task::FrameWaker as _;

        fn emit(line: &str) {
            log::info!("__MW__ {line}");
        }
        let gpu = self.scenario == 2;
        let prop = |name: &str| {
            std::process::Command::new("/system/bin/getprop")
                .arg(name)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('"', ""))
                .unwrap_or_default()
        };
        let soc = {
            let s = prop("ro.soc.model");
            if s.is_empty() {
                prop("ro.board.platform")
            } else {
                s
            }
        };
        emit(&format!(
            "{{\"fw\":\"vieww\",\"ev\":\"device\",\"app\":\"{}\",\"scenario\":{},\"model\":\"{}\",\"android\":\"{}\",\"soc\":\"{soc}\"}}",
            self.app,
            self.scenario,
            prop("ro.product.model"),
            prop("ro.build.version.release"),
        ));
        let phases = app_schedule();
        emit(&format!(
            "{{\"fw\":\"vieww\",\"ev\":\"start\",\"app\":\"{}\",\"path\":\"{}\",\"phases\":{}}}",
            self.app,
            if gpu { "gpu" } else { "cpu" },
            phases.len()
        ));
        let android = self.android.clone();
        let mut run = FarmRun::new(phases, emit).with_finish(move || game_loop::finish(&android));
        if let Some(sheet) = self.sheet {
            run.sheet = Some(sheet);
        }
        let clock = run.clock();
        let seconds = run.seconds();
        let run = Rc::new(RefCell::new(run));

        let waker = app.waker();
        std::thread::spawn(move || {
            // Let the first frame and start-up settle before the first phase.
            std::thread::sleep(Duration::from_millis(1500));
            waker.wake();
            for (i, s) in seconds.iter().enumerate() {
                // Wake at the half too: the sheet closes there, and an
                // otherwise idle loop would not ask for the frame that does it.
                std::thread::sleep(Duration::from_secs_f32(*s / 2.0));
                waker.wake();
                std::thread::sleep(Duration::from_secs_f32(*s / 2.0));
                clock.store(i + 1, Ordering::Release);
                waker.wake();
            }
        });
        let (before, after) = (Rc::clone(&run), run);
        let app = app
            .before_frame(move |driver: &mut FrameDriver| before.borrow_mut().before(driver))
            .on_frame(move |log: &FrameLog| after.borrow_mut().presented(log));
        if gpu {
            app.prefer_gpu()
        } else {
            app
        }
    }
}

#[cfg(target_os = "android")]
mod game_loop {
    use jni::objects::{JObject, JString};
    use jni::{jni_sig, jni_str};

    /// The `scenario` extra of a `TEST_LOOP` launch, or `None`.
    pub(super) fn scenario(android: &crate::AndroidApp) -> Option<i32> {
        if android.vm_as_ptr().is_null() || android.activity_as_ptr().is_null() {
            return None;
        }
        // SAFETY: both pointers belong to the running activity and are not
        // kept past this call — the contract `deep_links.rs` relies on.
        let vm = unsafe { jni::JavaVM::from_raw(android.vm_as_ptr().cast()) };
        let activity_ptr = android.activity_as_ptr();
        let found = vm.attach_current_thread(|env| -> Result<Option<i32>, jni::errors::Error> {
            let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
            let intent = env
                .call_method(
                    &activity,
                    jni_str!("getIntent"),
                    jni_sig!("()Landroid/content/Intent;"),
                    &[],
                )?
                .l()?;
            if intent.is_null() {
                return Ok(None);
            }
            let action = env
                .call_method(
                    &intent,
                    jni_str!("getAction"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            if action.is_null() {
                return Ok(None);
            }
            let action = env.cast_local::<JString>(action)?.try_to_string(env)?;
            if action != "com.google.intent.action.TEST_LOOP" {
                return Ok(None);
            }
            let key = env.new_string("scenario")?;
            let n = env
                .call_method(
                    &intent,
                    jni_str!("getIntExtra"),
                    jni_sig!("(Ljava/lang/String;I)I"),
                    &[
                        jni::objects::JValue::Object(&key),
                        jni::objects::JValue::Int(0),
                    ],
                )?
                .i()?;
            Ok(Some(n))
        });
        found.ok().flatten()
    }

    /// End the test the way Test Lab expects (`Activity.finish()`), then the
    /// process a few seconds later so the next scenario starts cold.
    pub(super) fn finish(android: &crate::AndroidApp) {
        if !android.vm_as_ptr().is_null() && !android.activity_as_ptr().is_null() {
            // SAFETY: as in `scenario`.
            let vm = unsafe { jni::JavaVM::from_raw(android.vm_as_ptr().cast()) };
            let activity_ptr = android.activity_as_ptr();
            let _ = vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
                let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
                env.call_method(&activity, jni_str!("finish"), jni_sig!("()V"), &[])?;
                Ok(())
            });
        }
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            std::process::exit(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn the_summary_is_p50_p95_p99_max() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(summary(&v), (51.0, 95.0, 99.0, 100.0));
        assert_eq!(summary(&[]), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn the_app_scenario_is_scroll_sheet_idle() {
        let s = app_schedule();
        let names: Vec<_> = s.iter().map(|p| p.name).collect();
        assert_eq!(names, ["scroll", "sheet", "idle"]);
        assert!(s[2].seconds >= 10.0, "a 10 s idle (CI-01)");
    }

    /// The run against a real driver: phases in order, the sheet opened and
    /// closed, the synthetic finger delivered, and the protocol's lines.
    #[test]
    fn a_run_plays_its_phases_and_narrates_them() {
        use vieww_render::foundation::Size;
        use vieww_render::widget::prelude::*;

        let lines = Rc::new(RefCell::new(Vec::<String>::new()));
        let sheet = Rc::new(RefCell::new(Vec::<bool>::new()));
        let taps = Rc::new(std::cell::Cell::new(0_u32));
        let (l, s, t) = (lines.clone(), sheet.clone(), taps.clone());
        let phases = vec![
            Phase {
                name: "scroll",
                kind: PhaseKind::Scroll,
                seconds: 0.0,
            },
            Phase {
                name: "sheet",
                kind: PhaseKind::Sheet,
                seconds: 0.0,
            },
            Phase {
                name: "idle",
                kind: PhaseKind::Idle,
                seconds: 0.0,
            },
        ];
        let mut run = FarmRun::new(phases, move |line| l.borrow_mut().push(line.to_owned()))
            .with_sheet(move |open| s.borrow_mut().push(open));

        let mut driver = FrameDriver::new(Size::new(400.0, 800.0));
        driver.set_root(
            GestureDetector::new()
                .on_drag_update(move |_| t.set(t.get() + 1))
                .child(SizedBox::expand()),
        );
        driver.draw_frame();
        let clock = run.clock();
        for phase in 0..=3 {
            clock.store(phase, Ordering::Release);
            for _ in 0..12 {
                run.before(&mut driver);
                driver.draw_frame();
            }
        }
        assert!(run.is_done());
        assert!(taps.get() > 0, "the synthetic finger dragged");
        assert_eq!(*sheet.borrow(), [true, false], "opened, then closed");
        let all = lines.borrow().join("\n");
        for phase in ["scroll", "sheet", "idle"] {
            assert!(
                all.contains(&format!("\"ev\":\"phase\",\"phase\":\"{phase}\"")),
                "{all}"
            );
            assert!(
                all.contains(&format!("\"ev\":\"stats\",\"phase\":\"{phase}\"")),
                "{all}"
            );
        }
        assert!(
            all.contains("\"ev\":\"mem\"") && all.ends_with("}") && all.contains("\"ev\":\"done\"")
        );
        for line in lines.borrow().iter() {
            assert!(!line.contains('\n'), "one line per event");
        }
    }
}
