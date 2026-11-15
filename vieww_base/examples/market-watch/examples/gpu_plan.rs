//! What the GPU path is *asked* to do per frame, with no GPU: the schedule at
//! a phone's size and density, each frame planned and turned into the ops a
//! backend would execute — so a number from a phone can be read against how
//! much of the frame was redrawn, carried, and overdrawn.
//!
//! ```console
//! cargo run --release -p market-watch --example gpu_plan -- [--size 393x851] [--dpr 2.75]
//! ```
use std::time::Instant;

use market_watch::{schedule, Load, Model, Screen};
use vieww_foundation::{Color, EdgeInsets, Rect, Size, ViewMetrics};
use vieww_gpu::{PixelRect, Planner};
use vieww_hal::scene_exec::{frame_passes, Retained, SceneOp};
use vieww_render::FrameDriver;

fn med(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let mut size = Size::new(393.0, 851.0);
    let mut dpr = 2.75_f32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => {
                let s = args.next().expect("--size WxH");
                let (w, h) = s.split_once('x').expect("--size WxH");
                size = Size::new(w.parse().expect("w"), h.parse().expect("h"));
            }
            "--dpr" => dpr = args.next().expect("--dpr N").parse().expect("dpr"),
            _ => {}
        }
    }
    let (pw, ph) = (
        (size.width * dpr).round() as u32,
        (size.height * dpr).round() as u32,
    );
    let screen = f64::from(pw) * f64::from(ph);
    let surface = Rect::new(0.0, 0.0, pw as f32, ph as f32);
    let viewport = Screen::viewport(size);
    let mut driver = FrameDriver::new(size);
    driver.set_view_metrics(ViewMetrics {
        size,
        device_pixel_ratio: dpr,
        safe_area: EdgeInsets::default(),
        view_insets: EdgeInsets::default(),
    });
    let model = Model::new(driver.elements().runtime(), 0);
    driver.elements().set_root(Screen::new(&model, size));
    driver.draw_frame();
    let mut planner = Planner::new();
    let mut retained: Option<Retained> = None;
    // `MW_FRAMES=60` shortens the continuous phases, for a run under a profiler.
    let cap: usize = std::env::var("MW_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    println!("{pw}x{ph} @ {dpr}");
    println!(
        "{:<15} {:>5} {:>8} {:>8} {:>7} {:>8} {:>6} {:>8} {:>8} {:>8} {:>6} {:>6} {:>8}",
        "phase",
        "frms",
        "plan ms",
        "verts",
        "full%",
        "redraw%",
        "p90%",
        "bounds%",
        "carry%",
        "ovrdraw",
        "runs",
        "posts",
        "idx drawn"
    );
    for (name, load, seconds) in schedule(false) {
        let frames = if load.continuous() {
            ((seconds * 60.0) as usize).min(cap)
        } else {
            1
        };
        let (
            mut plan_ms,
            mut verts,
            mut redraw,
            mut bounds,
            mut carry,
            mut over,
            mut runs,
            mut posts,
            mut idx,
        ) = (
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let mut full = 0usize;
        for f in 0..frames {
            if f == 0 {
                load.enter(&model, viewport);
            }
            if load.continuous() {
                load.frame(&model, viewport);
            }
            driver.draw_frame();
            if load == Load::Idle {
                continue;
            }
            let logical = driver.damage();
            let physical = logical.scaled(dpr, surface);
            let damage: Option<Vec<PixelRect>> = if physical.is_everything() {
                None
            } else {
                Some(
                    physical
                        .regions()
                        .iter()
                        .map(|r| r.intersect(surface))
                        .filter(|r| !r.is_empty())
                        .map(|r| {
                            let p = PixelRect::round_out(r);
                            PixelRect {
                                x0: p.x0 - 2,
                                y0: p.y0 - 2,
                                x1: p.x1 + 2,
                                y1: p.y1 + 2,
                            }
                        })
                        .collect(),
                )
            };
            let carried: f64 = physical
                .copies()
                .iter()
                .map(|c| f64::from(c.src.intersect(surface).area()))
                .sum();
            let scene = driver.scene().scaled(dpr);
            let t = Instant::now();
            let plan = planner.plan(&scene, pw as f32, ph as f32);
            plan_ms.push(t.elapsed().as_secs_f64() * 1e3);
            assert!(
                plan.is_complete(),
                "incomplete plan: {:?}",
                plan.unsupported
            );
            let exec = frame_passes(&plan, pw, ph, Color::WHITE, damage.as_deref(), retained);
            retained = Some(exec.retained);
            verts.push(plan.vertices.len() as f64);
            let (mut area, mut n_runs, mut n_posts, mut n_idx) = (0.0_f64, 0.0, 0.0, 0.0);
            for op in &exec.ops {
                match op {
                    SceneOp::DrawRun {
                        first_index,
                        index_count,
                        scissor,
                        ..
                    } => {
                        n_runs += 1.0;
                        n_idx += f64::from(*index_count);
                        let ix = &plan.indices[*first_index as usize..][..*index_count as usize];
                        for tri in ix.as_chunks::<3>().0 {
                            let p: Vec<[f32; 2]> = tri
                                .iter()
                                .map(|&i| plan.vertices[i as usize].position)
                                .collect();
                            let a = 0.5
                                * ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                                    - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]))
                                    .abs();
                            let x0 = p
                                .iter()
                                .map(|q| q[0])
                                .fold(f32::MAX, f32::min)
                                .max(scissor.x0 as f32);
                            let x1 = p
                                .iter()
                                .map(|q| q[0])
                                .fold(f32::MIN, f32::max)
                                .min(scissor.x1 as f32);
                            let y0 = p
                                .iter()
                                .map(|q| q[1])
                                .fold(f32::MAX, f32::min)
                                .max(scissor.y0 as f32);
                            let y1 = p
                                .iter()
                                .map(|q| q[1])
                                .fold(f32::MIN, f32::max)
                                .min(scissor.y1 as f32);
                            let bb = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
                            area += f64::from(a.min(bb));
                        }
                    }
                    SceneOp::Post { scissors, .. } => {
                        n_posts += 1.0;
                        for s in scissors {
                            area += f64::from(s.width()) * f64::from(s.height());
                        }
                    }
                    _ => {}
                }
            }
            match &exec.frame_clip {
                None => {
                    full += 1;
                    redraw.push(100.0);
                    bounds.push(100.0);
                }
                Some(clip) => {
                    let px: f64 = clip
                        .iter()
                        .map(|r| f64::from(r.width()) * f64::from(r.height()))
                        .sum();
                    redraw.push(100.0 * px / screen);
                    let b = clip.iter().fold(None::<PixelRect>, |b, r| {
                        Some(b.map_or(*r, |b| PixelRect {
                            x0: b.x0.min(r.x0),
                            y0: b.y0.min(r.y0),
                            x1: b.x1.max(r.x1),
                            y1: b.y1.max(r.y1),
                        }))
                    });
                    bounds.push(b.map_or(0.0, |b| {
                        100.0 * f64::from(b.width()) * f64::from(b.height()) / screen
                    }));
                }
            }
            carry.push(100.0 * carried / screen);
            over.push(area / screen);
            runs.push(n_runs);
            posts.push(n_posts);
            idx.push(n_idx);
        }
        // The tail has its own column: a list that repaints 40% of the screen
        // one frame in ten has a fine median.
        println!(
            "{name:<15} {frames:>5} {:>8.2} {:>8.0} {:>6.0}% {:>7.1}% {:>5.1}% {:>7.1}% {:>7.1}% {:>8.2} {:>6.0} {:>6.0} {:>8.0}",
            med(&mut plan_ms), med(&mut verts), 100.0 * full as f64 / frames as f64,
            med(&mut redraw), redraw.get(redraw.len() * 9 / 10).copied().unwrap_or(0.0), med(&mut bounds), med(&mut carry), med(&mut over), med(&mut runs),
            med(&mut posts), med(&mut idx)
        );
    }
}
