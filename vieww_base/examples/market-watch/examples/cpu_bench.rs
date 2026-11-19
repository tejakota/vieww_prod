//! The CPU path's raster on the comparison schedule, at a phone's size and
//! density, timed and fingerprinted: each phase's median and p90 retained
//! raster, and one hash over every byte of every frame it drew.
//!
//! ```console
//! VIEWW_RASTER_THREADS=1 cargo run --release -p market-watch --example cpu_bench -- [--frames 120] [--runs 5] [--json OUT.json]
//! ```
//!
//! `--phase scroll|live|storm` plays only the initial load and that phase:
//! what `ci/check/raster-instructions.sh` counts under callgrind, one phase at
//! a time (the hash then covers only those frames).
//!
//! `--json` also writes the numbers as one JSON object — the machine, the
//! walk, and per phase the median, p90 and byte hash — which is what CI's
//! machine legs compare and report (`ci/jobs/compare-machines.toml`).
//!
//! The hash is the point as much as the times: a speed-up to the rasterizer
//! must leave it unchanged (`docs/FIDELITY.md` §1), and this checks that on
//! the very frames the phone draws — the list, the sparklines, the scroll's
//! carries — not only on the fixture gallery. `VIEWW_RASTER_THREADS=1` is
//! the phone's walk (Medium and High tiers are serial).
//!
//! Times are this machine's, a development aid: the phone run is the
//! measurement.

use std::time::Instant;

use market_watch::{Load, Model, Screen};
use vieww_foundation::{Color, EdgeInsets, Rect, Size, ViewMetrics};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;

/// FNV-1a over the frame's bytes, folded into the running hash.
fn fold(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

fn main() {
    let mut frames = 120usize;
    let mut runs = 5usize;
    let mut json: Option<String> = None;
    let mut only: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => {
                frames = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--frames N")
            }
            "--runs" => runs = args.next().and_then(|v| v.parse().ok()).expect("--runs N"),
            "--json" => json = Some(args.next().expect("--json PATH")),
            "--phase" => only = Some(args.next().expect("--phase scroll|live|storm")),
            _ => {}
        }
    }
    let dpr = 2.75_f32;
    let size = Size::new(393.0, 851.0);
    let (w, h) = (
        (size.width * dpr).round() as u32,
        (size.height * dpr).round() as u32,
    );
    let surface = Rect::new(0.0, 0.0, w as f32, h as f32);
    let viewport = Screen::viewport(size);
    let full = [
        (Load::Load(2_000), 1),
        (Load::Scroll, frames),
        (Load::Live, frames),
        (Load::Storm, frames),
    ];
    let schedule: Vec<(Load, usize)> = match only.as_deref() {
        None => full.to_vec(),
        Some(name) => {
            let wanted = match name {
                "scroll" => Load::Scroll,
                "live" => Load::Live,
                "storm" => Load::Storm,
                other => panic!("--phase {other}: scroll, live or storm"),
            };
            vec![
                full[0],
                full.iter()
                    .copied()
                    .find(|(l, _)| *l == wanted)
                    .expect("in the schedule"),
            ]
        }
    };
    // Per phase, every run's samples together; the hash must agree run to run.
    let mut times: Vec<Vec<f64>> = vec![Vec::new(); schedule.len()];
    let mut hashes: Vec<Option<u64>> = vec![None; schedule.len()];
    for _ in 0..runs {
        let mut driver = FrameDriver::new(size);
        let model = Model::new(driver.elements().runtime(), 0);
        driver.set_root(Screen::new(&model, size));
        driver.set_view_metrics(ViewMetrics {
            size,
            device_pixel_ratio: dpr,
            safe_area: EdgeInsets::default(),
            view_insets: EdgeInsets::default(),
        });
        driver.draw_frame();
        let mut renderer = NativeRenderer::new();
        for (i, &(load, n)) in schedule.iter().enumerate() {
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            for f in 0..n {
                if f == 0 {
                    load.enter(&model, viewport);
                }
                if load.continuous() {
                    load.frame(&model, viewport);
                }
                driver.draw_frame();
                let scene = driver.scene().scaled(dpr);
                let damage = driver.damage().scaled(dpr, surface);
                let t = Instant::now();
                renderer
                    .render_retained_in_place(&scene, &damage, w, h, Color::WHITE)
                    .expect("retained frame");
                times[i].push(t.elapsed().as_secs_f64() * 1e3);
                fold(&mut hash, renderer.last_frame());
            }
            match hashes[i] {
                None => hashes[i] = Some(hash),
                Some(seen) => assert_eq!(seen, hash, "{load:?}: a run drew different bytes"),
            }
        }
    }
    let mut phases = Vec::new();
    for (i, &(load, _)) in schedule.iter().enumerate() {
        let v = &mut times[i];
        v.sort_by(f64::total_cmp);
        let (median, p90, hash) = (v[v.len() / 2], v[v.len() * 9 / 10], hashes[i].unwrap_or(0));
        println!(
            "{:<12} raster med {median:>6.2} ms  p90 {p90:>6.2} ms  bytes {hash:016x}",
            format!("{load:?}"),
        );
        let name = match load {
            Load::Load(_) => "load",
            Load::Scroll => "scroll",
            Load::Live => "live",
            Load::Storm => "storm",
            _ => "other",
        };
        phases.push(format!(
            "{{\"phase\":\"{name}\",\"median_ms\":{median:.3},\"p90_ms\":{p90:.3},\"hash\":\"{hash:016x}\"}}"
        ));
    }
    if let Some(path) = json {
        // Hand-written, like every other line this example prints: the
        // fields are numbers and fixed words, nothing that needs escaping.
        let walk = std::env::var("VIEWW_RASTER_THREADS").unwrap_or_else(|_| "default".into());
        let cores = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
        let text = format!(
            "{{\"os\":\"{}\",\"arch\":\"{}\",\"cores\":{cores},\"walk\":\"{}\",\"size\":\"{w}x{h}\",\"frames\":{frames},\"runs\":{runs},\"phases\":[{}]}}\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            walk.replace('"', ""),
            phases.join(",")
        );
        std::fs::write(&path, text).expect("writing --json");
    }
}
