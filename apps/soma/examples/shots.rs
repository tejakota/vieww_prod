//! Headless PNGs of the app's screens — the same renders the app would
//! draw, without a window.
//!
//! ```console
//! cargo run -p soma --example shots -- /tmp/soma-shots
//! ```
//!
//! The reel steps the clock a sixtieth at a time, so the rig's springs
//! integrate the way they would on a device; every frame is reproducible.
//! This is also the performance receipt: each shot prints the 3D renderer's
//! own counters for the frame, so a regression in triangle or fragment
//! cost shows up beside the picture that caused it.

use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::Size;
use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;
use vieww::widget::Theme;

use soma::{build_state, root, run_search, select_topic, AppState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "soma-shots".to_owned());
    std::fs::create_dir_all(&out)?;

    // Layout overflows are the screens' bug, not this tool's.
    vieww_render::overflow::forget_reported();

    let mut renderer = NativeRenderer::new();

    // ---- the resting body: blurred, breathing ---------------------------
    let mut driver = FrameDriver::new(soma::SURFACE);
    driver.set_fonts(soma::fonts());
    let state = mount(&mut driver);
    let mut reel = Reel { at: 0.0 };

    reel.advance(&mut driver, 2.4);
    shot(&mut renderer, &mut driver, &state, &out, "1-idle-blurred")?;

    // ---- typing: the camera already following the leading hit -----------
    type_into(&state, "chest pain");
    reel.advance(&mut driver, 0.9);
    shot(&mut renderer, &mut driver, &state, &out, "2-searching")?;

    type_into(&state, "heart");
    reel.advance(&mut driver, 0.9);
    shot(&mut renderer, &mut driver, &state, &out, "3-searching-heart")?;

    // ---- a topic opens: the panel rises, the flesh goes to glass --------
    let results = state.results.get();
    if let Some(top) = results.first() {
        select_topic(&state, top.topic);
    }
    reel.advance(&mut driver, 0.35);
    shot(&mut renderer, &mut driver, &state, &out, "4-topic-arriving")?;

    reel.advance(&mut driver, 1.1);
    shot(&mut renderer, &mut driver, &state, &out, "5-topic-open")?;

    // ---- an x-ray of a specific organ, typed as a regex ------------------
    type_into(&state, "^(kidney|renal)");
    reel.advance(&mut driver, 0.8);
    shot(&mut renderer, &mut driver, &state, &out, "6-regex-preview")?;

    let results = state.results.get();
    let picked = results.first().map(|h| h.topic);
    println!("    7: hits={} picked={:?} -> {:?}", results.len(), picked, picked.map(|i| (state.db.topics[i].title, state.db.topics[i].organ)));
    if let Some(top) = results.first() {
        select_topic(&state, top.topic);
    }
    reel.advance(&mut driver, 1.4);
    shot(&mut renderer, &mut driver, &state, &out, "7-kidney-xray")?;

    println!("done: {out}");

    // ---- the frame-cost receipt ------------------------------------------
    // Measured while the camera is actually flying — a settled tree costs
    // nothing, which is the point, but the number that matters is the one
    // the flight pays. Retarget the rig and time the crossing.
    soma::select_topic(&state, brain_index(&state));
    let start = std::time::Instant::now();
    let frames = 90;
    let mut drew = 0;
    for i in 0..frames {
        let before = state.pose.get().camera.position;
        reel.advance(&mut driver, 1.0 / 60.0);
        let after = state.pose.get().camera.position;
        if before != after {
            drew += 1;
        }
        if i == 0 || i == 30 || i == 89 {
            println!("    frame {i}: camera now {:?}", after);
        }
    }
    let elapsed = start.elapsed().as_secs_f32();
    println!("    frames that moved the camera: {drew}/{frames}");
    println!(
        "camera-flight frame cost (draw + 3D render + blur, 402x874 logical): \
         {:.1} ms — {:.0} fps ceiling",
        elapsed / frames as f32 * 1000.0,
        frames as f32 / elapsed
    );
    Ok(())
}

/// Mount the app the way the platform does, returning the state so the
/// script can drive it.
fn mount(driver: &mut FrameDriver) -> Rc<AppState> {
    let runtime = driver.elements().runtime().clone();
    let state = build_state(&runtime);
    driver.tickers().add(&state.rig);
    state.panel_scroll.attach(driver.tickers());
    driver.set_root(root(&state));
    state
}

/// Type into the search field the way the widget would report it.
fn type_into(state: &Rc<AppState>, text: &str) {
    let mut value = state.query.get();
    value.text = text.to_owned();
    state.query.set(value);
    run_search(state);
}

/// The brain's topic index, for the flight the receipt times — far enough
/// from the kidneys that the crossing cannot be a no-op.
fn brain_index(state: &Rc<AppState>) -> usize {
    state
        .db
        .topics
        .iter()
        .position(|t| t.kind == 'B' && t.title == "Brain")
        .expect("the corpus carries a Brain anatomy page")
}

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
    state: &Rc<AppState>,
    out: &str,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let size: Size = soma::SURFACE;
    let (pixels, _) = renderer.render_to_pixels(
        driver.scene(),
        size.width as u32,
        size.height as u32,
        soma::GROUND,
    )?;
    let image = image::RgbaImage::from_raw(
        size.width as u32,
        size.height as u32,
        pixels.data().to_vec(),
    )
    .ok_or("invalid RGBA frame dimensions")?;
    let path = format!("{out}/{name}.png");
    image.save(&path)?;

    // The 3D renderer's own account of the frame it just drew.
    let stats = *state.stats.borrow();
    let pose = state.pose.get();
    println!(
        "  {path} — objects {} · tris {} · fragments {} · blur {:.2} · xray {:.2} · panel {:.2}",
        stats.objects, stats.triangles, stats.fragments, pose.blur, pose.xray, pose.panel
    );
    Ok(())
}
