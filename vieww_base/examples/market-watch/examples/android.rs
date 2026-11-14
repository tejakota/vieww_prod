//! Market Watch on a phone: the comparison schedule, timed from inside the app.
//!
//! ```console
//! cargo apk build -p market-watch --example android --profile dist --target aarch64-linux-android
//! adb shell setprop debug.mw.gpu 1     # the planned GPU path (default: CPU rasterizer)
//! adb shell setprop debug.mw.thermal 0 # skip the 60 s thermal pass
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
//! __MW__ {"fw":"vieww","ev":"stats","phase":"live2k","frames":478,"work_ms":[..],"interval_ms":[..],"jank":3}
//! __MW__ {"fw":"vieww","ev":"stages","phase":"live2k","frames":478,"build_ms":[p50,p90,max],..,"gpu_blit_ms":[..],"budget_ms_end":16.7}
//! __MW__ {"fw":"vieww","ev":"touch","events":212,"latency_ms":[p50,p95,p99,worst],"unanswered":0,"t":…}
//! __MW__ {"fw":"vieww","ev":"done","t":…}
//! ```

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

    fn emit(line: &str) {
        log::info!("__MW__ {line}");
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
        /// The budget now, which the cadence controller may have lowered.
        budget_now_ms: f64,
        /// The display's frame budget, from the first presented frame.
        budget_ms: f64,
        done: bool,
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
            let (im, i95, i99, imax) = summary(&self.interval);
            let jank = self
                .interval
                .iter()
                .filter(|&&v| v > self.budget_ms * 1.5)
                .count();
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"stats\",\"phase\":\"{name}\",\"frames\":{},\\
\"work_ms\":[{wm:.3},{w95:.3},{w99:.3},{wmax:.3}],\"interval_ms\":[{im:.3},{i95:.3},{i99:.3},{imax:.3}],\\
\"jank\":{jank},\"budget_ms\":{:.2},\"t\":{:.1}}}",
                self.work.len(),
                self.budget_ms,
                self.t()
            ));
            let s = std::mem::take(&mut self.split);
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"stages\",\"phase\":\"{name}\",\"frames\":{},\\
\"build_ms\":{},\"raster_ms\":{},\"gpu_cpu_ms\":{},\"fence_ms\":{},\"acquire_ms\":{},\\
\"present_ms\":{},\"gpu_ms\":{},\"budget_ms_end\":{:.2},\"scale_ms\":{},\"plan_ms\":{},\\
\"gpu_draw_ms\":{},\"gpu_layers_ms\":{},\"gpu_filters_ms\":{},\"gpu_shadows_ms\":{},\"gpu_blit_ms\":{},\"t\":{:.1}}}",
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
            self.work.clear();
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
                    emit(&format!(
                        "{{\"fw\":\"vieww\",\"ev\":\"done\",\"t\":{:.1}}}",
                        self.t()
                    ));
                    return;
                }
                self.current = Some(target);
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
            if self.done || self.current.is_none() {
                return;
            }
            // One sample per presented frame: the hook runs between frames,
            // and the frame count says whether one was presented since the
            // last call. The stage sample is its own condition — a frame
            // with no stage breakdown still counts in `work`.
            if log.frames() > self.seen_frames {
                self.seen_frames = log.frames();
                if let Some(w) = log.work_samples().last() {
                    if let Some(st) = log.last_stages() {
                        self.split.record(&st, *w);
                    }
                    self.work.push(w.as_secs_f64() * 1e3);
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
        let gpu = prop("debug.mw.gpu") == "1";
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
            interval: Vec::new(),
            last_present: None,
            split: Split::default(),
            seen_frames: 0,
            touches_seen: 0,
            touch_emitted: None,
            budget_now_ms: 0.0,
            budget_ms: 0.0,
            done: false,
        }));

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
        });
        match result {
            Ok(report) => log::info!("market-watch stopped: {report}"),
            Err(error) => log::error!("market-watch failed: {error}"),
        }
    }
}
