//! Headless PNGs of both screens — the same renders the app would draw, without
//! a window.
//!
//! ```console
//! cargo run -p u --example shots -- /tmp/u-shots
//! ```
//!
//! The reel steps the clock a sixtieth at a time rather than jumping, so the
//! tickers (the clock, the burst decay, the route transition) integrate the
//! way they would on a device. Any frame is reproducible: home at 0.5 s, the
//! training burst a fifth of a second later, settings arriving and settled.

use std::time::Duration;

use vieww::foundation::Size;
use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "u-shots".to_owned());
    std::fs::create_dir_all(&out)?;

    // A layout overflow is a bug in the screens, not in this tool; forgetting
    // them here keeps the panic from eating the picture that shows it.
    vieww_render::overflow::forget_reported();

    let mut driver = FrameDriver::new(u::SURFACE);
    driver.use_system_fonts();
    let shared = u::mount(&mut driver);
    driver.set_root(u::root(&shared));

    let mut renderer = NativeRenderer::new();
    let mut reel = Reel { at: 0.0 };

    // Home, settled.
    reel.advance(&mut driver, 0.6);
    shot(&mut renderer, &mut driver, &out, "1-home")?;

    // The training burst: every cell firing at once, then easing back.
    u::take(&shared).train();
    reel.advance(&mut driver, 0.22);
    shot(&mut renderer, &mut driver, &out, "2-home-burst")?;

    // Settings, caught mid-arrival and then at rest.
    let state = u::take(&shared);
    state.nav.push(u::settings_route(shared.clone()));
    reel.advance(&mut driver, 0.1);
    shot(&mut renderer, &mut driver, &out, "3-settings-arriving")?;
    reel.advance(&mut driver, 1.4);
    shot(&mut renderer, &mut driver, &out, "4-settings")?;

    // The same screen, scrolled to the bottom: the form really scrolls, and
    // the button that acts is reachable.
    state.scroll.drag(-900.0);
    reel.advance(&mut driver, 0.05);
    shot(&mut renderer, &mut driver, &out, "5-settings-scrolled")?;

    let overflows = vieww_render::overflow::reported();
    println!("U shots -> {}", std::fs::canonicalize(&out)?.display());
    println!("layout overflows: {overflows}");
    Ok(())
}

/// The film's clock, advanced only forwards.
struct Reel {
    at: f32,
}

impl Reel {
    fn advance(&mut self, driver: &mut FrameDriver, seconds: f32) {
        let target = self.at + seconds;
        while self.at < target {
            self.at = (self.at + 1.0 / 60.0).min(target);
            driver.draw_frame_at(Duration::from_secs_f32(self.at));
        }
    }
}

fn shot(
    renderer: &mut NativeRenderer,
    driver: &mut FrameDriver,
    out: &str,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let size: Size = u::SURFACE;
    let (pixels, _) = renderer.render_to_pixels(
        driver.scene(),
        size.width as u32,
        size.height as u32,
        u::palette::GROUND,
    )?;
    let image = image::RgbaImage::from_raw(
        size.width as u32,
        size.height as u32,
        pixels.data().to_vec(),
    )
    .ok_or("invalid RGBA frame dimensions")?;
    let path = format!("{out}/{name}.png");
    image.save(&path)?;
    println!("  {path}");
    Ok(())
}
