//! Market Watch, headless: the comparison schedule on the CPU rasterizer, with
//! no display, no GPU and no window.
//!
//! ```console
//! cargo run --release -p market-watch -- OUT_DIR [--thermal] [--size 720x1000]
//! ```
//!
//! Plays [`market_watch::schedule`] at a nominal 60 frames a second: a
//! continuous phase is `seconds × 60` frames, an event phase one frame, and an
//! idle phase checks that nothing is rebuilt. Every frame is timed in two
//! parts — build (rebuild + layout + flatten into a scene) and raster (the
//! damaged scene regions into the renderer's retained pixels) — so the numbers
//! say where a frame's cost is. This is the same `render_retained_in_place`
//! path Android uses, rather than a full-surface raster that hides the value
//! of repaint boundaries and damage tracking.
//!
//! A development tool: the phone run (`examples/android.rs`) is the comparison.
//! Writes `OUT_DIR/vieww-desktop.jsonl` (one JSON object per phase),
//! `OUT_DIR/vieww-<phase>.png` at the event phases, and a table on stdout.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use market_watch::{schedule, summary, Load, Model, Screen};
use vieww_foundation::{Color, Size};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;

const HZ: f32 = 60.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut out: PathBuf = "shots".into();
    let mut thermal = false;
    let mut size = Size::new(720.0, 1000.0);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--thermal" => thermal = true,
            "--size" => {
                let s = args.next().expect("--size WxH");
                let (w, h) = s.split_once('x').expect("--size WxH");
                size = Size::new(w.parse().expect("width"), h.parse().expect("height"));
            }
            other => out = other.into(),
        }
    }
    std::fs::create_dir_all(&out).expect("output directory");
    let (w, h) = (size.width as u32, size.height as u32);
    let viewport = Screen::viewport(size);

    let mut driver = FrameDriver::new(size);
    let mut renderer = NativeRenderer::new();
    let model = Model::new(driver.elements().runtime(), 0);
    driver.elements().set_root(Screen::new(&model, size));
    driver.draw_frame();

    let mut jsonl = String::new();
    println!(
        "{:<15} {:>6} {:>22} {:>22} {:>9} {:>9} {:>9}",
        "phase", "frames", "build ms med/p95/max", "raster ms med/p95/max", "fps@cpu", "rebuilt%", "damage%"
    );
    for (name, load, seconds) in schedule(thermal) {
        let frames = if load.continuous() { (seconds * HZ) as usize } else { 1 };
        let rebuilds_before = driver.scene_rebuilds();
        let mut build = Vec::with_capacity(frames);
        let mut raster = Vec::with_capacity(frames);
        let mut damaged = Vec::with_capacity(frames);
        let mut built = 0usize;
        let mut total = 0usize;
        let wall = Instant::now();
        for f in 0..frames {
            if f == 0 {
                load.enter(&model, viewport);
            }
            if load.continuous() {
                load.frame(&model, viewport);
            }
            let t = Instant::now();
            driver.draw_frame();
            build.push(t.elapsed().as_secs_f64() * 1e3);
            let s = driver.flatten_stats();
            built += s.built;
            total += s.total();
            if load == Load::Idle {
                continue; // nothing to raster: the point is that nothing ran
            }
            let t = Instant::now();
            let damage = driver.damage();
            damaged.push(f64::from(
                100.0 * damage.covered_area() / damage.surface().area().max(1.0),
            ));
            renderer
                .render_retained_in_place(driver.scene(), damage, w, h, Color::WHITE)
                .expect("rasterising damaged regions through vieww's retained renderer");
            raster.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let wall = wall.elapsed().as_secs_f64();
        let idle_rebuilds = driver.scene_rebuilds() - rebuilds_before;
        if matches!(load, Load::Load(_) | Load::Tick | Load::Jump) {
            let (png, _) = renderer.render_to_png(driver.scene(), w, h, Color::WHITE).expect("png");
            std::fs::write(out.join(format!("vieww-{name}.png")), png).expect("writing png");
        }
        let (bm, b95, b99, bmax) = summary(&build);
        let (rm, r95, r99, rmax) = summary(&raster);
        // What a CPU-bound loop would sustain on this machine: frames over
        // the time build + raster took, not over the 60 Hz the script assumes.
        let work: f64 = build.iter().sum::<f64>() + raster.iter().sum::<f64>();
        let fps = if work > 0.0 { frames as f64 / (work / 1e3) } else { 0.0 };
        let rebuilt = if total > 0 { 100.0 * built as f64 / total as f64 } else { 0.0 };
        let damage = if damaged.is_empty() { 0.0 } else { damaged.iter().sum::<f64>() / damaged.len() as f64 };
        println!(
            "{name:<15} {frames:>6} {:>22} {:>22} {fps:>9.1} {rebuilt:>8.1}% {damage:>8.1}%{}",
            format!("{bm:.2}/{b95:.2}/{bmax:.2}"),
            if raster.is_empty() { "-".into() } else { format!("{rm:.2}/{r95:.2}/{rmax:.2}") },
            if load == Load::Idle { format!("  (scene rebuilds while idle: {idle_rebuilds})") } else { String::new() }
        );
        let _ = writeln!(
            jsonl,
            "{{\"framework\":\"vieww\",\"leg\":\"desktop-cpu\",\"phase\":\"{name}\",\"rows\":{},\"frames\":{frames},\
\"build_ms\":[{bm:.3},{b95:.3},{b99:.3},{bmax:.3}],\"raster_ms\":[{rm:.3},{r95:.3},{r99:.3},{rmax:.3}],\
\"work_fps\":{fps:.2},\"rebuilt_pct\":{rebuilt:.2},\"damage_pct\":{damage:.2},\"idle_rebuilds\":{idle_rebuilds},\"wall_s\":{wall:.3}}}",
            model.rows()
        );
    }
    std::fs::write(out.join("vieww-desktop.jsonl"), jsonl).expect("writing jsonl");
    println!("wrote {}", out.display());
}
