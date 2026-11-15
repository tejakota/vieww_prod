//! A scroll drawn by carrying and repainting only the damage shows the same
//! picture as the same frames drawn in full — at 1:1, and at the 2.75 device
//! pixels per logical one of the phone the comparison runs on, where the
//! damage is `Damage::scaled` and the carried block's rim is fractional.
//!
//! The damage this checks is the damage the GPU path redraws and carries by
//! (`NativeRenderer::present_planned`), so a region it leaves out is a stale
//! pixel on the phone on either rasterizer.

use market_watch::{Load, Model, Screen};
use vieww_foundation::{Color, EdgeInsets, Rect, Size, ViewMetrics};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;

/// How far a channel of a carried pixel may sit from the repainted one. A
/// carried frame and a repainted one are not byte-identical today, at either
/// density: a few to a few hundred pixels a frame differ by up to 3/255
/// (measured here, before and after the 2.75x carry was restored). That is
/// rounding; a row left behind or a strip not repainted is text on the wrong
/// background, off by most of the range.
const ROUNDING: u8 = 8;

fn play(dpr: f32) {
    let size = Size::new(393.0, 851.0);
    let (w, h) = (
        (size.width * dpr).round() as u32,
        (size.height * dpr).round() as u32,
    );
    let surface = Rect::new(0.0, 0.0, w as f32, h as f32);
    let viewport = Screen::viewport(size);
    let mut driver = FrameDriver::new(size);
    let model = Model::new(driver.elements().runtime(), 0);
    // Through `set_root`, not `elements().set_root`, and **before** the
    // metrics: a metrics change with nothing mounted reaches nothing, which
    // `FrameDriver::set_view_metrics` refuses loudly.
    driver.set_root(Screen::new(&model, size));
    driver.set_view_metrics(ViewMetrics {
        size,
        device_pixel_ratio: dpr,
        safe_area: EdgeInsets::default(),
        view_insets: EdgeInsets::default(),
    });
    driver.draw_frame();
    let mut retained = NativeRenderer::new();
    let mut full = NativeRenderer::new();
    let mut carried = 0usize;
    let mut partial = 0usize;
    let mut frame = 0usize;
    for (load, frames) in [
        (Load::Load(2_000), 1),
        (Load::Scroll, 40),
        (Load::Live, 40),
        (Load::Storm, 10),
        (Load::Jump, 1),
        (Load::Scroll, 20),
    ] {
        for f in 0..frames {
            if f == 0 {
                load.enter(&model, viewport);
            }
            if load.continuous() {
                load.frame(&model, viewport);
            }
            driver.draw_frame();
            frame += 1;
            let (scene, damage) = if dpr == 1.0 {
                (driver.scene().clone(), driver.damage().clone())
            } else {
                (
                    driver.scene().scaled(dpr),
                    driver.damage().scaled(dpr, surface),
                )
            };
            carried += usize::from(!damage.copies().is_empty());
            partial += usize::from(!damage.is_everything());
            retained
                .render_retained_in_place(&scene, &damage, w, h, Color::WHITE)
                .expect("retained frame");
            full.render_in_place(&scene, w, h, Color::WHITE)
                .expect("full frame");
            let (a, b) = (retained.last_frame(), full.last_frame());
            assert_eq!(a.len(), b.len());
            if a != b {
                let (mut worst, mut count, mut at) = (0u8, 0usize, 0usize);
                for (i, (p, q)) in a
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(b.as_chunks::<4>().0)
                    .enumerate()
                {
                    let d = p
                        .iter()
                        .zip(q)
                        .map(|(x, y)| x.abs_diff(*y))
                        .max()
                        .unwrap_or(0);
                    if d > 0 {
                        count += 1;
                    }
                    if d > worst {
                        worst = d;
                        at = i;
                    }
                }
                assert!(
                    worst <= ROUNDING,
                    "dpr {dpr}, frame {frame} ({load:?} #{f}): {count} px differ from the full frame, \
                     worst by {worst} at ({}, {}); damage: {damage}",
                    at % w as usize,
                    at / w as usize
                );
            }
        }
    }
    // The point of the damage is that a scroll carries: if it stopped — as it
    // had at 2.75x, where every scroll frame was a full repaint — this test
    // would otherwise pass by comparing a full frame with itself.
    assert!(carried >= 80, "dpr {dpr}: only {carried} frames carried");
    assert!(partial >= 100, "dpr {dpr}: only {partial} partial frames");
}

#[test]
fn a_scroll_drawn_by_its_damage_shows_the_full_frames_at_1x() {
    play(1.0);
}

#[test]
fn a_scroll_drawn_by_its_damage_shows_the_full_frames_at_2_75x() {
    play(2.75);
}
