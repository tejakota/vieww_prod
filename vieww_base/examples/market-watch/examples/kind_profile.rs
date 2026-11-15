//! Where a damaged frame's raster time goes, by kind of command — the
//! development view of `NativeRenderer::profile_kinds`, played over the
//! comparison's schedule.
//!
//! ```console
//! cargo run --release -p market-watch --example kind_profile
//! ```
use market_watch::{Load, Model, Screen};
use vieww_foundation::{Color, EdgeInsets, Rect, Size, ViewMetrics};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;

fn main() {
    let dpr = 2.75_f32;
    let size = Size::new(393.0, 851.0);
    let (w, h) = (
        (size.width * dpr).round() as u32,
        (size.height * dpr).round() as u32,
    );
    let surface = Rect::new(0.0, 0.0, w as f32, h as f32);
    let viewport = Screen::viewport(size);
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
    renderer.profile_kinds(true);
    for (load, frames) in [
        (Load::Load(2_000), 1),
        (Load::Scroll, 40),
        (Load::Live, 40),
        (Load::Storm, 40),
    ] {
        let mut totals = Vec::new();
        for f in 0..frames {
            if f == 0 {
                load.enter(&model, viewport);
            }
            if load.continuous() {
                load.frame(&model, viewport);
            }
            driver.draw_frame();
            let (scene, damage) = (
                driver.scene().scaled(dpr),
                driver.damage().scaled(dpr, surface),
            );
            renderer
                .render_retained_in_place(&scene, &damage, w, h, Color::WHITE)
                .expect("frame");
            if let Some(kinds) = renderer.last_kind_times() {
                totals.push(kinds);
            }
        }
        let n = totals.len().max(1) as f64;
        let avg = |pick: fn(&vieww_paint::native::RasterKindTimes) -> f64| {
            totals.iter().map(&pick).sum::<f64>() / n
        };
        println!("{load:?} over {frames} frames (ms avg per frame):");
        type Pick = fn(&vieww_paint::native::RasterKindTimes) -> f64;
        let kinds: [(&str, Pick); 7] = [
            ("rects", |t| t.rects.as_secs_f64() * 1e3),
            ("paths", |t| t.paths.as_secs_f64() * 1e3),
            ("text", |t| t.text.as_secs_f64() * 1e3),
            ("images", |t| t.images.as_secs_f64() * 1e3),
            ("shadows", |t| t.shadows.as_secs_f64() * 1e3),
            ("layers", |t| t.layers.as_secs_f64() * 1e3),
            ("composite", |t| t.composite.as_secs_f64() * 1e3),
        ];
        for (name, pick) in kinds {
            println!("  {name:<10} {:6.3}", avg(pick));
        }
    }
}
