//! Font-metric calibration: renders known glyphs at known box origins so a
//! script can measure where the ink actually lands, and derives the
//! baseline/cap constants the absolute SVG layout needs.
//!
//! ```console
//! cargo run -p u --example calib -- /tmp/u-calib
//! ```

use std::time::Duration;

use vieww::foundation::Size;
use vieww::prelude::*;
use vieww::FrameDriver;
use vieww_foundation::{Color, FontWeight, TextStyle};
use vieww_paint::native::NativeRenderer;
use vieww_widget::Theme;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "u-calib".to_owned());
    std::fs::create_dir_all(&out)?;
    vieww_render::overflow::forget_reported();

    let mut renderer = NativeRenderer::new();
    let mut driver = FrameDriver::new(Size::new(420.0, 920.0));
    driver.set_fonts(u::fonts());

    // Mid-gray ground: ink is anything darker or brighter than the ground.
    let ground = Color::rgb(128, 128, 128);

    // "H" has a flat cap top and baseline bottom, no descender — the cleanest
    // probe for cap height and baseline. Boxes are 2.2*size tall so the glyph
    // never clips, placed at exact multiples of 40 in y, 40 in x.
    let sizes: [f32; 13] = [
        7.5, 8.5, 9.0, 9.5, 10.0, 10.5, 11.0, 11.5, 12.0, 15.0, 20.0, 24.0, 30.0,
    ];
    let mut stack = Stack::new().fit(StackFit::Expand);
    for (i, size) in sizes.iter().enumerate() {
        let x = 20.0 + (i as f32 % 5.0) * 80.0;
        let y = 20.0 + (i as f32 / 5.0).floor() * 300.0;
        stack = stack.push(
            Positioned::new()
                .left(x)
                .top(y)
                .child(
                    Text::new("H").style(
                        TextStyle::new(*size)
                            .weight(FontWeight::Regular)
                            .color(Color::rgb(255, 255, 255)),
                    ),
                ),
        );
        println!("H size {size:5} at box origin ({x},{y})");
        // A second row: the same with Bold, offset by 150 y.
        stack = stack.push(
            Positioned::new()
                .left(x)
                .top(y + 150.0)
                .child(
                    Text::new("H").style(
                        TextStyle::new(*size)
                            .weight(FontWeight::Bold)
                            .color(Color::rgb(255, 255, 255)),
                    ),
                ),
        );
    }

    driver.set_root(Theme::new(u::palette::theme()).child(stack));
    driver.draw_frame_at(Duration::from_secs_f32(0.1));

    let size = Size::new(420.0, 920.0);
    let _ = size;
    let (pixels, _) = {
        let scene = driver.scene();
        let (data, _) = renderer.render_to_pixels(scene, 420, 920, ground)?;
        (data, ())
    };
    let image = image::RgbaImage::from_raw(420, 920, pixels.data().to_vec())
        .ok_or("bad frame")?;
    image.save(format!("{out}/calib.png"))?;
    println!("wrote {out}/calib.png");
    Ok(())
}
