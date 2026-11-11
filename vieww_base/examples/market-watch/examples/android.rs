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
//! lines:
//!
//! ```text
//! __MW__ {"fw":"vieww","ev":"phase","phase":"live2k","t":12345.6}
//! __MW__ {"fw":"vieww","ev":"stats","phase":"live2k","frames":478,"work_ms":[..],"interval_ms":[..],"jank":3}
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
    use vieww_platform_winit::{App, AndroidApp, FrameLog};
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
        /// The display's frame budget, from the first second of intervals.
        budget_ms: f64,
        done: bool,
    }

    impl Run {
        fn t(&self) -> f64 {
            self.start.elapsed().as_secs_f64() * 1e3
        }

        /// Close the phase that just ended: one stats line.
        fn flush(&mut self) {
            let Some(i) = self.current else { return };
            let name = &self.phases[i].0;
            let (wm, w95, w99, wmax) = summary(&self.work);
            let (im, i95, i99, imax) = summary(&self.interval);
            let jank = self.interval.iter().filter(|&&v| v > self.budget_ms * 1.5).count();
            emit(&format!(
                "{{\"fw\":\"vieww\",\"ev\":\"stats\",\"phase\":\"{name}\",\"frames\":{},\
\"work_ms\":[{wm:.3},{w95:.3},{w99:.3},{wmax:.3}],\"interval_ms\":[{im:.3},{i95:.3},{i99:.3},{imax:.3}],\
\"jank\":{jank},\"budget_ms\":{:.2},\"t\":{:.1}}}",
                self.work.len(),
                self.budget_ms,
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
                    emit(&format!("{{\"fw\":\"vieww\",\"ev\":\"done\",\"t\":{:.1}}}", self.t()));
                    return;
                }
                self.current = Some(target);
                let (name, load, _) = self.phases[target].clone();
                emit(&format!("{{\"fw\":\"vieww\",\"ev\":\"phase\",\"phase\":\"{name}\",\"t\":{:.1}}}", self.t()));
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
            if self.done || self.current.is_none() {
                return;
            }
            if let Some(w) = log.work_samples().last() {
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
        }
    }

    pub(crate) fn main(android: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("vieww"),
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
