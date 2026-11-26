//! Market Watch on a phone: the comparison schedule, timed from inside the app.
//!
//! ```console
//! cargo apk build -p market-watch --example android --profile dist --target aarch64-linux-android
//! adb shell setprop debug.mw.gpu 1     # the planned GPU path (default: CPU rasterizer)
//! adb shell setprop debug.mw.thermal 0 # skip the 60 s thermal pass
//! adb shell setprop debug.mw.threads 0 # pin the raster walk serial (A/B leg)
//! adb shell setprop debug.mw.profile 1 # per-kind raster times in the stages line
//! ```
//!
//! On launch it plays [`market_watch::schedule`]: phases are timed by a clock
//! thread, which wakes the event loop at each boundary, so an idle phase really
//! is idle — nothing asks for a frame until it ends. Continuous phases hold a
//! repeating animation, which keeps the loop drawing at the display's rate,
//! and apply their change in `before_frame`, once per frame.
//!
//! Prints to logcat (tag `vieww`), one line per event, in the protocol the
//! device runner and `compare_report.py` read. The Valdi app prints the same
//! `phase`/`stats`/`done` lines; the `stages` and `touch` lines are the ones
//! only the app being measured from the inside can print:
//!
//! ```text
//! __MW__ {"fw":"vieww","ev":"phase","phase":"live2k","t":12345.6}
//! __MW__ {"fw":"vieww","ev":"stats","phase":"live2k","frames":478,"work_ms":[..],"wait_ms":[..],"interval_ms":[..],"jank":3}
//! __MW__ {"fw":"vieww","ev":"stages","phase":"live2k","frames":478,"build_ms":[p50,p90,max],..,"gpu_blit_ms":[..],"budget_ms_end":16.7,"kinds":{"rects":[..],"paths":[..],"text":[..],..},"t":…}
//! __MW__ {"fw":"vieww","ev":"touch","events":212,"latency_ms":[p50,p95,p99,worst],"unanswered":0,"t":…}
//! __MW__ {"fw":"vieww","ev":"done","t":…}
//! ```
//!
//! `work_ms` and `wait_ms` are the two halves of a frame's wall clock:
//! what the application did, and what the display made it wait (the image
//! acquire and the flight-slot fence). The 60Hz Pixel 9's published "work
//! p50 18.8ms" in the continuous phases was 5ms of work and 13.8ms of
//! standing in line for an image; a table that cannot say which half is
//! which is a table every later decision argues from a number that was
//! never effort. Old runs without `wait_ms` parse as before — a reader
//! that does not know the field skips it as one token.
//!
//! **Firebase Test Lab** launches it as a Game Loop test instead
//! (`com.google.intent.action.TEST_LOOP`, see `firebase/`): scenario 1 is the
//! CPU rasterizer, scenario 2 the GPU path, both with the thermal pass,
//! and the app exits once the schedule is done so the test ends there.
//!
//! The `kinds` half of the `stages` line is there only when
//! `debug.mw.profile=1` asked for it (`VIEWW_RASTER_PROFILE`): the raster
//! stage split by command kind, the same p50/p90/max shape — where a heavy
//! frame's raster time actually goes, which a stage total alone cannot say.

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: vieww_platform_winit::AndroidApp) {
    app::main(android);
}

#[cfg(target_os = "android")]
mod app {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use market_watch::{schedule, summary, Load, Model, Screen};
    use vieww_animation::Tween;
    use vieww_element::Animation;
    use vieww_foundation::task::FrameWaker;
    use vieww_foundation::Size;
    use vieww_platform_winit::native::FrameStages;
    use vieww_platform_winit::{AndroidApp, App, FrameLog};
    use vieww_render::FrameDriver;

    fn prop(name: &str) -> String {
        std::process::Command::new("/system/bin/getprop")
            .arg(name)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    }

    /// The Game Loop scenario Firebase Test Lab launched this activity with,
    /// or `None` for any other launch (the launcher, the device runner's
    /// `am start`). Read from the activity's intent: an app started by Test
    /// Lab has no shell to set the `debug.mw.*` properties from.
    fn game_loop_scenario(android: &AndroidApp) -> Option<i32> {
        use jni::objects::{JObject, JString};
        use jni::JavaVM;
        if android.vm_as_ptr().is_null() || android.activity_as_ptr().is_null() {
            return None;
        }
        // SAFETY: both pointers come from the running activity, which
        // outlives this call on its own main thread.
        let vm = unsafe { JavaVM::from_raw(android.vm_as_ptr().cast()) }.ok()?;
        let mut env = vm.attach_current_thread_permanently().ok()?;
        let activity = unsafe { JObject::from_raw(android.activity_as_ptr().cast()) };
        let intent = env
            .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
            .and_then(|v| v.l())
            .ok()?;
        if intent.is_null() {
            return None;
        }
        let action = env
            .call_method(&intent, "getAction", "()Ljava/lang/String;", &[])
            .and_then(|v| v.l())
            .ok()?;
        if action.is_null() {
            return None;
        }
        let action: String = env.get_string(&JString::from(action)).ok()?.into();
        if action != "com.google.intent.action.TEST_LOOP" {
            return None;
        }
        let name = env.new_string("scenario").ok()?;
        let scenario = env
            .call_method(
                &intent,
                "getIntExtra",
                "(Ljava/lang/String;I)I",
                &[(&name).into(), 0.into()],
            )
            .and_then(|v| v.i())
            .ok()?;
        Some(scenario)
    }

    /// End a Game Loop launch the way Test Lab expects: `Activity.finish()`.
    /// The process is then ended a few seconds later regardless, so the next
    /// scenario starts from a fresh one.
    fn finish_game_loop(android: &AndroidApp) {
        use jni::objects::JObject;
        use jni::JavaVM;
        if !android.vm_as_ptr().is_null() && !android.activity_as_ptr().is_null() {
            // SAFETY: as in `game_loop_scenario`.
            if let Ok(vm) = unsafe { JavaVM::from_raw(android.vm_as_ptr().cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_permanently() {
                    let activity = unsafe { JObject::from_raw(android.activity_as_ptr().cast()) };
                    let _ = env.call_method(&activity, "finish", "()V", &[]);
                }
            }
        }
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(3));
            std::process::exit(0);
        });
    }

    fn emit(line: &str) {
        log::info!("__MW__ {line}");
    }

    /// The fields of `/proc/self/stat` after the command name, which may
    /// hold spaces: field 3 (`state`) is index 0 here.
    fn stat_fields() -> Option<Vec<u64>> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let rest = stat.rsplit_once(") ")?.1;
        Some(
            rest.split_whitespace()
                .map(|f| f.parse().unwrap_or(0))
                .collect(),
        )
    }

    /// This process's CPU time so far (utime + stime), in clock ticks.
    ///
    /// The device runner reads the same counter from `adb shell`; on Test
    /// Lab there is no shell, so the app reads its own and narrates the
    /// per-phase delta (`proc` events).
    fn cpu_ticks() -> Option<u64> {
        let f = stat_fields()?;
        Some(f.get(11)? + f.get(12)?)
    }

    /// Milliseconds since this process was started — Test Lab's launch is a
    /// cold one, so at the first presented frame this is the cold start.
    fn since_process_start_ms() -> Option<f64> {
        // `starttime` is in clock ticks since boot, on the boot-time clock
        // (suspend included), and CLK_TCK is 100 on every Android ABI. The
        // "now" it is compared with must be on the same clock:
        // `CLOCK_BOOTTIME`, read directly. `/proc/uptime` is not readable by
        // an app on every Android release (the 2026-11 Redmi and Pixel runs
        // both returned null from it), and the clock call cannot be refused.
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
        // SAFETY: `now` is a valid, writable `timespec` for the call's duration.
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

    /// One `kB` line of `/proc/self/smaps_rollup` (`Pss:`, `Rss:`).
    fn rollup_kb(key: &str) -> Option<u64> {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
        text.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    /// The battery's temperature in tenths of a degree, where the device
    /// lets an app read it (most do; some SELinux policies do not).
    fn battery_temp_dc() -> Option<i64> {
        std::fs::read_to_string("/sys/class/power_supply/battery/temp")
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// A JSON number, or `null` for a reading the device refused.
    fn num<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map_or_else(|| "null".to_owned(), |v| v.to_string())
    }

    /// A phase's frames, split by where their time went — the `stages` line's
    /// samples, recorded once per presented frame.
    ///
    /// **Per field, not per frame**: the p50 of one stage and the p50 of
    /// another can come from different frames, which is the right number
    /// for "which stage is typically the big one" and the wrong one for
    /// "what does a typical frame cost" — the `stats` line's `work_ms`
    /// answers that. A GPU frame fills the whole set; a CPU frame fills
    /// only `build` and `raster` (the app layer records no stage breakdown
    /// on the CPU present path), so a phase on the CPU rasterizer reports
    /// the stages it has and zero elsewhere.
    #[derive(Default)]
    struct Split {
        /// The frame's work outside every renderer stage: the build, the
        /// layout, the app's own code — the share of a frame no renderer
        /// stage accounts for.
        build: Vec<f64>,
        /// The CPU rasterizer's share of the frame.
        raster: Vec<f64>,
        /// The CPU half of a GPU frame: atlas uploads, command building,
        /// the submit call.
        gpu_cpu: Vec<f64>,
        /// Blocked on a fence — this frame's, or the previous frame's when
        /// they overlap, which is exactly the pipelining question.
        fence: Vec<f64>,
        acquire: Vec<f64>,
        present: Vec<f64>,
        /// The GPU's own time for the frame, by timestamp query — the GPU's
        /// clock, not this thread's.
        gpu: Vec<f64>,
        /// `Scene::scaled`, lifting an off-1x frame to device pixels.
        scale: Vec<f64>,
        /// `vieww_gpu::Planner::plan` — the geometry pass the planner cache
        /// work is measured by.
        plan: Vec<f64>,
        g_draw: Vec<f64>,
        g_layers: Vec<f64>,
        g_filters: Vec<f64>,
        g_shadows: Vec<f64>,
        g_blit: Vec<f64>,
        /// The raster stage by kind of command, filled only when
        /// `VIEWW_RASTER_PROFILE=1` turned the renderer's kind timing on —
        /// see [`Split::record`]. `kinds_on` says any sample arrived, so a
        /// profiled run and an unprofiled one print the same line shape
        /// minus the `kinds` field.
        kinds_on: bool,
        k_rects: Vec<f64>,
        k_paths: Vec<f64>,
        k_text: Vec<f64>,
        k_images: Vec<f64>,
        k_shadows: Vec<f64>,
        k_layers: Vec<f64>,
        k_composite: Vec<f64>,
    }

    impl Split {
        fn record(&mut self, st: &FrameStages, work: Duration) {
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            self.build.push((ms(work) - ms(st.total())).max(0.0));
            self.raster.push(ms(st.cpu_raster));
            self.gpu_cpu.push(ms(st.gpu.total() - st.gpu.wait));
            self.fence.push(ms(st.gpu.wait));
            self.acquire.push(ms(st.acquire));
            self.present.push(ms(st.present));
            self.scale.push(ms(st.scale));
            self.plan.push(ms(st.plan));
            if let Some(kinds) = &st.cpu_kinds {
                self.kinds_on = true;
                self.k_rects.push(ms(kinds.rects));
                self.k_paths.push(ms(kinds.paths));
                self.k_text.push(ms(kinds.text));
                self.k_images.push(ms(kinds.images));
                self.k_shadows.push(ms(kinds.shadows));
                self.k_layers.push(ms(kinds.layers));
                self.k_composite.push(ms(kinds.composite));
            }
            if let Some(passes) = &st.gpu_passes {
                self.gpu.push(ms(passes.total()));
                self.g_draw.push(ms(passes.draw));
                self.g_layers.push(ms(passes.layers));
                self.g_filters.push(ms(passes.filters));
                self.g_shadows.push(ms(passes.shadows));
                self.g_blit.push(ms(passes.blit));
            }
        }
    }

    /// The `stages` line's summary of one field's samples: `[p50, p90, max]`.
    /// The tail has its own slot because the stage that grows in the *worst*
    /// frame is not always the stage that grows in a typical one.
    fn p50_90_max(samples: &[f64]) -> String {
        if samples.is_empty() {
            return "[0,0,0]".to_owned();
        }
        let mut v = samples.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        format!("[{:.2},{:.2},{:.2}]", at(0.5), at(0.9), v[v.len() - 1])
    }

    /// Where the schedule is, shared by the hooks.
    struct Run {
        start: Instant,
        phases: Vec<(String, Load, f32)>,
        /// The phase the clock thread has moved to; `current` catches up.
        target: Arc<AtomicUsize>,
        current: Option<usize>,
        model: Option<Model>,
        viewport: f32,
        keepalive: Option<Animation<f32>>,
        work: Vec<f64>,
        /// The same frames' display waits — the acquire and the fence wait —
        /// that `work` had subtracted, so the stats line can say both halves.
        /// The 60Hz Pixel 9's published "work p50 18.8ms" was 5ms of work
        /// and 13.8ms of standing in line for an image; a table that cannot
        /// say which half is which is a table every later decision argues
        /// from a number that was never effort.
        wait: Vec<f64>,
        interval: Vec<f64>,
        last_present: Option<Instant>,
        /// This phase's stage samples — see [`Split`].
        split: Split,
        /// `FrameLog::frames()` at the last sample, so a hook call with no
        /// new presented frame since is not counted twice.
        seen_frames: u64,
        /// Touch events seen at the last `touch` line, and when it went out.
        touches_seen: u64,
        touch_emitted: Option<Instant>,
        /// The renderer's route line, once emitted.
        route_emitted: Option<String>,
        /// The budget now, which the cadence controller may have lowered.
        budget_now_ms: f64,
        /// The display's frame budget, from the first presented frame.
        budget_ms: f64,
        /// CPU ticks at the current phase's start (`proc` events).
        phase_ticks: Option<u64>,
        /// Whether the first presented frame has been narrated.
        first_frame_emitted: bool,
        done: bool,
        /// A Game Loop launch: finish once the schedule is done, which is how
        /// Test Lab learns the test is over.
        exit_when_done: Option<AndroidApp>,
    }

    impl Run {
        fn t(&self) -> f64 {
            self.start.elapsed().as_secs_f64() * 1e3
        }

        /// Close the phase that just ended: one stats line, then the same
        /// frames split by where their time went.
        fn flush(&mut self) {
            let Some(i) = self.current else { return };
            let name = &self.phases[i].0;
            let (wm, w95, w99, wmax) = summary(&self.work);
            let (wtm, wt95, wt99, wtmax) = summary(&self.wait);
            let (im, i95, i99, imax) = summary(&self.interval);
            let jank = self
                .interval
                .iter()
                .filter(|&&v| v > self.budget_ms * 1.5)
                .count();
            // One line per event, through and through: a message the app's
            // logger splits over several logcat entries (an escape pair and
            // a raw newline did exactly that — the 2026-11-14 farm run's
            // events.jsonl holds the phase and done lines, and every stats,
            // stages and touch line was left behind in a dump as fragments
            // no reader could reassemble) never reaches a follower that
            // greps `__MW__`, whatever it carries.
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"stats\",\"phase\":\"{name}\",\"frames\":{},\
\"work_ms\":[{wm:.3},{w95:.3},{w99:.3},{wmax:.3}],\"wait_ms\":[{wtm:.3},{wt95:.3},{wt99:.3},{wtmax:.3}],\"interval_ms\":[{im:.3},{i95:.3},{i99:.3},{imax:.3}],\"jank\":{jank},\"budget_ms\":{:.2},\"t\":{:.1}}}",
                self.work.len(),
                self.budget_ms,
                self.t()
            ));
            let s = std::mem::take(&mut self.split);
            // The kinds fragment: printed only by a profiled run, after the
            // first frame taught the renderer to time its commands
            // (`debug.mw.profile=1`), so every other consumer of the line —
            // the runner's grep, the report's parser — sees the stages it
            // always saw. JSON, like the rest of the line, so a reader that
            // does not know the field skips it as one token.
            let kinds = if s.kinds_on {
                format!(
                    ",\"kinds\":{{\"rects\":{},\"paths\":{},\"text\":{},\
\"images\":{},\"shadows\":{},\"layers\":{},\"composite\":{}}}",
                    p50_90_max(&s.k_rects),
                    p50_90_max(&s.k_paths),
                    p50_90_max(&s.k_text),
                    p50_90_max(&s.k_images),
                    p50_90_max(&s.k_shadows),
                    p50_90_max(&s.k_layers),
                    p50_90_max(&s.k_composite)
                )
            } else {
                String::new()
            };
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"stages\",\"phase\":\"{name}\",\"frames\":{},\
\"build_ms\":{},\"raster_ms\":{},\"gpu_cpu_ms\":{},\"fence_ms\":{},\"acquire_ms\":{},\
\"present_ms\":{},\"gpu_ms\":{},\"budget_ms_end\":{:.2},\"scale_ms\":{},\"plan_ms\":{},\
\"gpu_draw_ms\":{},\"gpu_layers_ms\":{},\"gpu_filters_ms\":{},\"gpu_shadows_ms\":{},\"gpu_blit_ms\":{}{kinds},\"t\":{:.1}}}",
                s.build.len(),
                p50_90_max(&s.build),
                p50_90_max(&s.raster),
                p50_90_max(&s.gpu_cpu),
                p50_90_max(&s.fence),
                p50_90_max(&s.acquire),
                p50_90_max(&s.present),
                p50_90_max(&s.gpu),
                self.budget_now_ms,
                p50_90_max(&s.scale),
                p50_90_max(&s.plan),
                p50_90_max(&s.g_draw),
                p50_90_max(&s.g_layers),
                p50_90_max(&s.g_filters),
                p50_90_max(&s.g_shadows),
                p50_90_max(&s.g_blit),
                self.t()
            ));
            // What the device runner measures from `adb shell` and Test
            // Lab gives no shell for: the phase's CPU time, the resident
            // set and the battery temperature at its end, read by the app.
            let ticks = cpu_ticks();
            let delta = match (self.phase_ticks, ticks) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a)),
                _ => None,
            };
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"proc\",\"phase\":\"{name}\",\"cpu_ticks\":{},\"rss_kb\":{},\"batt_temp_dC\":{},\"t\":{:.1}}}",
                num(delta),
                num(rollup_kb("Rss:")),
                num(battery_temp_dc()),
                self.t()
            ));
            self.work.clear();
            self.wait.clear();
            self.interval.clear();
        }

        /// Called before every frame: move to the clock's phase, then apply
        /// this frame's change.
        fn before(&mut self, driver: &mut FrameDriver) {
            if self.done {
                return;
            }
            let target = self.target.load(Ordering::Acquire);
            if self.current != Some(target) {
                self.flush();
                if target >= self.phases.len() {
                    self.keepalive = None;
                    self.done = true;
                    // Memory after the run, the device runner's `dumpsys
                    // meminfo` PSS, read from the kernel's own rollup.
                    emit(&format!(
                        "{{\"fw\":\"vieww\",\"ev\":\"mem\",\"pss_kb\":{},\"rss_kb\":{},\"t\":{:.1}}}",
                        num(rollup_kb("Pss:")),
                        num(rollup_kb("Rss:")),
                        self.t()
                    ));
                    emit(&format!(
                        "{{\"fw\":\"vieww\",\"ev\":\"done\",\"t\":{:.1}}}",
                        self.t()
                    ));
                    if let Some(android) = &self.exit_when_done {
                        finish_game_loop(android);
                    }
                    return;
                }
                self.current = Some(target);
                self.phase_ticks = cpu_ticks();
                let (name, load, _) = self.phases[target].clone();
                emit(&format!(
                    "{{\"fw\":\"vieww\",\"ev\":\"phase\",\"phase\":\"{name}\",\"t\":{:.1}}}",
                    self.t()
                ));
                // Only continuous phases keep the loop running; between
                // events the loop is free to sleep, and an idle phase must.
                self.keepalive = load.continuous().then(|| {
                    let a = driver.animation(Tween::new(0.0_f32, 1.0), Duration::from_secs(1));
                    a.repeat(true, Duration::ZERO);
                    a
                });
                self.last_present = None; // an interval across a boundary is not this phase's
                if let Some(model) = &self.model {
                    load.enter(model, self.viewport);
                }
            }
            if let (Some(i), Some(model)) = (self.current, &self.model) {
                let load = self.phases[i].1;
                if load.continuous() {
                    load.frame(model, self.viewport);
                }
            }
        }

        fn presented(&mut self, log: &FrameLog) {
            // Touch-to-screen latency, whenever a person has touched the
            // screen: launch the app, swipe the list by hand, and read
            // `__MW__ {"ev":"touch",...}` lines — input to the frame that
            // showed it, the number a finger feels. At most one line every
            // two seconds, so a long swipe does not bury the phases.
            if let Some(r) = log.input_latency() {
                let due = self
                    .touch_emitted
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
                if r.total_events > self.touches_seen && r.samples > 0 && due {
                    self.touches_seen = r.total_events;
                    self.touch_emitted = Some(Instant::now());
                    let ms = |d: Duration| d.as_secs_f64() * 1e3;
                    emit(&format!(
                        "{{\"fw\":\"vieww\",\"ev\":\"touch\",\"events\":{},\"latency_ms\":[{:.1},{:.1},{:.1},{:.1}],\"unanswered\":{},\"t\":{:.1}}}",
                        r.total_events, ms(r.p50), ms(r.p95), ms(r.p99), ms(r.worst), r.unanswered, self.t()
                    ));
                }
            }
            // Cold start: process start to the first presented frame.
            if !self.first_frame_emitted && log.frames() > 0 {
                self.first_frame_emitted = true;
                emit(&format!(
                    "{{\"fw\":\"vieww\",\"ev\":\"first_frame\",\"since_process_ms\":{},\"t\":{:.1}}}",
                    num(since_process_start_ms().map(|ms| format!("{ms:.1}"))),
                    self.t()
                ));
            }
            if self.done || self.current.is_none() {
                return;
            }
            // One sample per presented frame: the hook runs between frames,
            // and the frame count says whether one was presented since the
            // last call. The stage sample is its own condition — a frame
            // with no stage breakdown still counts in `work`.
            if log.frames() > self.seen_frames {
                self.seen_frames = log.frames();
                // The route the renderer took, once it has taken one — the
                // one-line answer to "which path did these numbers come
                // through", which the 2026-11-15 Redmi run left as a guess
                // from the stages.
                // Again whenever it changes: the route is measured on the
                // phone now (`VIEWW_GPU_DIRECT` unset), so a run can start
                // direct and move to blit, and the numbers after the move
                // came through the other route.
                if let Some(route) = vieww_platform_winit::native::gpu_route_status() {
                    if self.route_emitted.as_deref() != Some(route.as_str()) {
                        self.route_emitted = Some(route.clone());
                        emit(&format!(
                            "{{\"fw\":\"vieww\",\"ev\":\"route\",\"route\":\"{route}\",\"t\":{:.1}}}",
                            self.t()
                        ));
                    }
                }
                if let Some(w) = log.work_samples().last() {
                    if let Some(st) = log.last_stages() {
                        self.split.record(&st, *w);
                    }
                    self.work.push(w.as_secs_f64() * 1e3);
                    // The waits that work had subtracted (acquire + fence);
                    // the same frame's sample, so the halves line up.
                    if let Some(wait) = log.wait_samples().last() {
                        self.wait.push(wait.as_secs_f64() * 1e3);
                    }
                }
                let now = Instant::now();
                if let Some(prev) = self.last_present.replace(now) {
                    let ms = (now - prev).as_secs_f64() * 1e3;
                    self.interval.push(ms);
                }
                if self.budget_ms == 0.0 {
                    self.budget_ms = log.budget().as_secs_f64() * 1e3;
                }
                self.budget_now_ms = log.budget().as_secs_f64() * 1e3;
            }
        }
    }

    pub(crate) fn main(android: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("vieww"),
        );
        let scenario = game_loop_scenario(&android);
        let gpu = match scenario {
            Some(n) => n == 2,
            None => prop("debug.mw.gpu") == "1",
        };
        if let Some(n) = scenario {
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"device\",\"scenario\":{n},\"model\":\"{}\",\"android\":\"{}\",\"soc\":\"{}\"}}",
                prop("ro.product.model").replace('"', ""),
                prop("ro.build.version.release").replace('"', ""),
                {
                    let soc = prop("ro.soc.model");
                    if soc.is_empty() { prop("ro.board.platform") } else { soc }
                }
                .replace('"', ""),
            ));
        }
        // `adb shell setprop debug.mw.direct 0` keeps the GPU pass on the
        // blit route (`VIEWW_GPU_DIRECT=0`), for an A/B of the two routes on
        // one phone: an app started by the launcher has no environment to
        // set, and the two routes do different work — direct redraws every
        // pixel of every frame in one pass, blit redraws and carries only
        // the damage through the intermediate — so which is cheaper is the
        // phone's to say. Set before the first window, which is when the
        // switch is read; the `route` event says what was taken.
        // Unset, the renderer measures both routes on this phone and keeps
        // the faster (`vieww_platform_winit`'s `gpu_route`); `1` pins direct.
        match prop("debug.mw.direct").as_str() {
            "0" => std::env::set_var("VIEWW_GPU_DIRECT", "0"),
            "1" => std::env::set_var("VIEWW_GPU_DIRECT", "1"),
            _ => {}
        }
        // The raster walk's worker count, as the comparison's same-unit A/B
        // (`vieww-cpuserial` pins it back to one): unset leaves the device
        // tier's default (`quality::raster_threads_for`), `0` is the serial
        // walk, a number is that many workers. Set before the first window
        // because the rasterizer reads the environment once, at its first
        // fill — the same once-per-process moment `debug.mw.direct` aims at.
        // The value arrives quoted when the runner clears it
        // (`setprop name ""` is refused bare), so quotes are trimmed first.
        match prop("debug.mw.threads").trim_matches('"').parse::<usize>() {
            Ok(0) | Ok(1) => std::env::set_var("VIEWW_RASTER_THREADS", "1"),
            Ok(workers) => std::env::set_var("VIEWW_RASTER_THREADS", workers.to_string()),
            Err(_) => {}
        }
        // Per-kind raster timings in the `stages` line (`kinds`): two clock
        // reads a command, so off unless a run wants the breakdown — the
        // runs that judge the walk read the stage totals, the runs that
        // decide what to optimise next read the kinds.
        if prop("debug.mw.profile").trim_matches('"') == "1" {
            std::env::set_var("VIEWW_RASTER_PROFILE", "1");
        }
        // The thermal pass runs on Test Lab too (about a minute, well inside
        // the job's 6-minute timeout): it is the only sustained-load number
        // the free farm can give.
        let thermal = prop("debug.mw.thermal") != "0";
        let phases = schedule(thermal);
        emit(&format!(
            "{{\"fw\":\"vieww\",\"ev\":\"start\",\"path\":\"{}\",\"phases\":{}}}",
            if gpu { "gpu" } else { "cpu" },
            phases.len()
        ));

        let target = Arc::new(AtomicUsize::new(0));
        let run = Rc::new(RefCell::new(Run {
            start: Instant::now(),
            phases: phases.clone(),
            target: target.clone(),
            current: None,
            model: None,
            viewport: 0.0,
            keepalive: None,
            work: Vec::new(),
            wait: Vec::new(),
            interval: Vec::new(),
            last_present: None,
            split: Split::default(),
            seen_frames: 0,
            touches_seen: 0,
            touch_emitted: None,
            route_emitted: None,
            budget_now_ms: 0.0,
            budget_ms: 0.0,
            phase_ticks: None,
            first_frame_emitted: false,
            done: false,
            exit_when_done: scenario.is_some().then(|| android.clone()),
        }));
        // Which raster policy this process actually took — the tier's thread
        // request, the cache budgets, any override — is emitted from the
        // build callback (see below), after the window has applied the
        // tier's policy: reading it any earlier would freeze the raster
        // walk's worker request at the core count, before the platform had
        // its say (`process_raster_thread_request` reads once).

        let app = App::new().title("Vieww Market Watch");
        // The clock: sleeps through each phase, then moves the target on and
        // wakes the loop, which may be asleep (idle) or busy (continuous).
        let waker = app.waker();
        let seconds: Vec<f32> = phases.iter().map(|p| p.2).collect();
        std::thread::spawn(move || {
            // Let the first frame and the start-up settle before load2k.
            std::thread::sleep(Duration::from_millis(1500));
            waker.wake();
            for (i, s) in seconds.iter().enumerate() {
                std::thread::sleep(Duration::from_secs_f32(*s));
                target.store(i + 1, Ordering::Release);
                waker.wake();
            }
        });

        let before = run.clone();
        let after = run.clone();
        let build = run.clone();
        let app = app
            .before_frame(move |driver: &mut FrameDriver| before.borrow_mut().before(driver))
            .on_frame(move |log: &FrameLog| after.borrow_mut().presented(log));
        let app = if gpu { app.prefer_gpu() } else { app };
        let result = app.run_android(android, move |driver| {
            let r = driver.surface();
            let size = Size::new(r.width(), r.height());
            // Starts empty: the schedule's first phase loads the rows, so the
            // load is measured rather than hidden in start-up.
            let model = Model::new(driver.elements().runtime(), 0);
            driver.set_root(Screen::new(&model, size));
            let mut run = build.borrow_mut();
            run.viewport = Screen::viewport(size);
            run.model = Some(model);
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"surface\",\"w\":{:.0},\"h\":{:.0},\"t\":{:.1}}}",
                size.width,
                size.height,
                run.t()
            ));
            // The window exists, so the tier's raster policy has been applied
            // (`apply_cache_budgets`, at window creation): this is the first
            // moment the run's own statement of its thread request and cache
            // budgets is both final and safe to ask for — asking any earlier
            // would read the walk's request before the platform set it. One
            // line, before the numbers that judge it, so a report never has
            // to guess whether a run's frames walked serially. The status
            // line is free text; any quote in it is escaped, keeping this
            // one JSON value.
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"quality\",\"line\":\"{}\",\"t\":{:.1}}}",
                vieww_platform_winit::cache_budgets_status().replace('"', "\\\""),
                run.t()
            ));
        });
        match result {
            Ok(report) => log::info!("market-watch stopped: {report}"),
            Err(error) => log::error!("market-watch failed: {error}"),
        }
    }
}
