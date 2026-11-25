//! Headless PNGs of the eight screens:
//! `cargo run --example shots -- /tmp/twin-shots`

use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;

use twin::{mount, Motion, Screen, SURFACE};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "twin-shots".to_owned());
    std::fs::create_dir_all(&out)?;
    vieww_render::overflow::forget_reported();
    let mut renderer = NativeRenderer::new();
    let mut driver = FrameDriver::new(SURFACE);
    let state = mount(&mut driver, Motion::Still);
    for screen in Screen::ALL {
        state.screen.set(screen);
        driver.draw_frame();
        driver.draw_frame();
        let (pixels, _) = renderer.render_to_pixels(
            driver.scene(),
            SURFACE.width as u32,
            SURFACE.height as u32,
            twin::GROUND,
        )?;
        let image = image::RgbaImage::from_raw(
            SURFACE.width as u32,
            SURFACE.height as u32,
            pixels.data().to_vec(),
        )
        .ok_or("bad frame")?;
        image.save(format!("{out}/{}.png", screen.name()))?;
    }
    println!("done: {out}");
    Ok(())
}
