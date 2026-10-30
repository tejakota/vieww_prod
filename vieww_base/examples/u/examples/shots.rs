//! Headless PNGs of the app's screens — the same renders the app would
//! draw, without a window.
//!
//! ```console
//! cargo run -p u --example shots -- /tmp/u-shots
//! ```
//!
//! Four mounts, four lives. The opening runs on a volatile log, so its
//! renders never depend on the machine drawing them; the sketch complete
//! and the daily rhythm run on logs replayed from fixture files written into
//! a temporary directory. Those fixtures are also the only test the log's
//! loader has: if [`u::events::Log::at`] could not fold those lines back
//! into state, the completion shot would show the sketch starting over and
//! the daily shot would show the first sketch again — the very bug this
//! build exists without. The workspace runs on a scratch engine that keeps
//! nothing, and brings the plan's own worked example — the move to another
//! city — so the assessment's every section has something honest to show.
//!
//! The reel steps the clock a sixtieth at a time rather than jumping, so the
//! tickers (the clock, the burst decay, the route transition) integrate the
//! way they would on a device. Any frame is reproducible: the opening
//! assembling at 1.2 s, the wordmark up at 2.8 s, the room arrived at 4.9 s,
//! the sheet arriving and the burst a fifth of a second later, settings
//! arriving and settled.

use std::time::Duration;

use vieww::foundation::Size;
use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;
use vieww_widget::Theme;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "u-shots".to_owned());
    std::fs::create_dir_all(&out)?;

    // A layout overflow is a bug in the screens, not in this tool; forgetting
    // them here keeps the panic from eating the picture that shows it.
    vieww_render::overflow::forget_reported();

    let mut renderer = NativeRenderer::new();

    // ---- the opening, and the room it becomes, on a log that keeps nothing -
    let mut driver = FrameDriver::new(u::SURFACE);
    driver.set_fonts(u::fonts());
    let shared = u::mount(&mut driver);
    driver.set_root(Theme::new(u::palette::theme()).child(u::root(&shared)));
    let mut reel = Reel { at: 0.0 };

    reel.advance(&mut driver, 1.2);
    shot(&mut renderer, &mut driver, &out, "1-opening-strands")?;

    reel.advance(&mut driver, 1.6);
    shot(&mut renderer, &mut driver, &out, "2-opening-wordmark")?;

    reel.advance(&mut driver, 2.1);
    shot(&mut renderer, &mut driver, &out, "3-home")?;

    // Answering, through the sheet: the field fires behind the scrim, the
    // choice is noted back, and the next question is already on the card.
    u::take(&shared).nav.push(u::ask_route(shared.clone()));
    reel.advance(&mut driver, 0.5);
    shot(&mut renderer, &mut driver, &out, "4-ask-arriving")?;

    u::take(&shared).answer(true);
    reel.advance(&mut driver, 0.22);
    shot(&mut renderer, &mut driver, &out, "5-ask-burst")?;

    u::take(&shared).nav.pop();
    reel.advance(&mut driver, 0.5);
    shot(&mut renderer, &mut driver, &out, "6-home-noted")?;

    // ---- the sketch complete, replayed from a log ------------------------
    // Twelve answers on one day, and no `ack`: the once-ever card between
    // the two phases, standing in the sheet that opened for the twelfth.
    let sketch: Vec<String> = (0..12).map(|ask| answer_line(19_000, ask, true)).collect();
    let mut driver = FrameDriver::new(u::SURFACE);
    driver.set_fonts(u::fonts());
    let shared = u::mount_with(&mut driver, fixture(&sketch)?, u::engine::Engine::scratch());
    driver.set_root(Theme::new(u::palette::theme()).child(u::root(&shared)));
    let mut reel = Reel { at: 0.0 };

    reel.advance(&mut driver, 4.9);
    u::take(&shared).nav.push(u::ask_route(shared.clone()));
    reel.advance(&mut driver, 0.5);
    shot(&mut renderer, &mut driver, &out, "7-sketch-complete")?;

    // ---- the daily rhythm, replayed from a log ---------------------------
    // The sketch on one day, acknowledged, then three answers on the next:
    // the day's counter is three of four, the lifetime count is fifteen, and
    // the name and voice came back through the same fold.
    let mut daily = sketch;
    daily.push("ack".to_owned());
    daily.push("name Teja Kota".to_owned());
    daily.push("tone 2".to_owned());
    daily.push(answer_line(19_001, 0, true));
    daily.push(answer_line(19_001, 1, false));
    daily.push(answer_line(19_001, 2, true));
    let mut driver = FrameDriver::new(u::SURFACE);
    driver.set_fonts(u::fonts());
    let shared = u::mount_with(&mut driver, fixture(&daily)?, u::engine::Engine::scratch());
    driver.set_root(Theme::new(u::palette::theme()).child(u::root(&shared)));
    let mut reel = Reel { at: 0.0 };

    reel.advance(&mut driver, 4.9);
    shot(&mut renderer, &mut driver, &out, "8-home-daily")?;

    // ---- settings, on that same replayed state ---------------------------
    let state = u::take(&shared);
    state.nav.push(u::settings_route(shared.clone()));
    reel.advance(&mut driver, 0.1);
    shot(&mut renderer, &mut driver, &out, "9-settings-arriving")?;
    reel.advance(&mut driver, 1.4);
    shot(&mut renderer, &mut driver, &out, "10-settings")?;

    // The same screen, scrolled to the bottom: the form really scrolls, and
    // the line that signs it is reachable.
    state.scroll.drag(-900.0);
    reel.advance(&mut driver, 0.05);
    shot(&mut renderer, &mut driver, &out, "11-settings-scrolled")?;

    // ---- settings at the reference's own state ----------------------------
    // A fresh log: Teja, warm, intuitive, four a day — the state the
    // reference draws — so the studio can be diffed against it directly.
    let mut driver = FrameDriver::new(u::SURFACE);
    driver.set_fonts(u::fonts());
    let shared = u::mount_with(&mut driver, fixture(&["name Teja".to_owned()])?, u::engine::Engine::scratch());
    driver.set_root(Theme::new(u::palette::theme()).child(u::root(&shared)));
    let mut reel = Reel { at: 0.0 };
    reel.advance(&mut driver, 4.9);
    u::take(&shared).nav.push(u::settings_route(shared.clone()));
    reel.advance(&mut driver, 1.5);
    shot(&mut renderer, &mut driver, &out, "12-settings-reference-state")?;

    // ---- the decision workspace, on a scratch engine ----------------------
    // The plan's own worked example (§2.3): the move to another city, its
    // two live options, what matters, the lease. The engine keeps nothing —
    // these renders show the room, not a life — and the assessment is the
    // deterministic baseline's own read of it.
    let mut driver = FrameDriver::new(u::SURFACE);
    driver.set_fonts(u::fonts());
    let shared = u::mount(&mut driver);
    driver.set_root(Theme::new(u::palette::theme()).child(u::root(&shared)));
    let mut reel = Reel { at: 0.0 };
    reel.advance(&mut driver, 4.9);

    let state = u::take(&shared);
    state.set_situation(text(
        "I'm considering moving to another city, but I'm not sure whether \
         the change is worth it.",
    ));
    state.set_choices(text("stay here\nmove to the coast"));
    state.set_wants(text("growth\nthe people close to me"));
    state.set_bounds(text("the lease until June"));
    state.nav.push(u::workspace_route(shared.clone()));
    reel.advance(&mut driver, 0.35);
    shot(&mut renderer, &mut driver, &out, "13-workspace-arriving")?;
    reel.advance(&mut driver, 0.5);
    shot(&mut renderer, &mut driver, &out, "14-workspace")?;

    // The assessment: every section the §2.4 contract asks for, on the same
    // scroll the person would read it on.
    state.assess();
    reel.advance(&mut driver, 0.25);
    shot(&mut renderer, &mut driver, &out, "15-workspace-assessed")?;

    // The verdict row and the read, at the bottom of the same scroll.
    state.work.drag(-900.0);
    reel.advance(&mut driver, 0.05);
    shot(&mut renderer, &mut driver, &out, "16-workspace-read")?;

    // Home again: the decision is the strip now — the memory's own words,
    // beside the briefcase, where the noted answer stood before any
    // decision existed.
    state.nav.pop();
    reel.advance(&mut driver, 0.5);
    shot(&mut renderer, &mut driver, &out, "17-home-after-decision")?;

    println!("U shots -> {}", std::fs::canonicalize(&out)?.display());
    println!("layout overflows: {}", vieww_render::overflow::reported());
    Ok(())
}

/// One editing value from a plain string — what a person typed.
fn text(words: &str) -> vieww_foundation::TextEditingValue {
    vieww_foundation::TextEditingValue::new(words.to_owned())
}

/// One `answer` line for a fixture: the verb, the day, the question, the
/// choice.
fn answer_line(day: u64, ask: usize, choice: bool) -> String {
    format!("answer {day} {ask} {}", if choice { 'a' } else { 'b' })
}

/// A log replayed from lines written into a temporary file — the loader's
/// only test, and the fixture that makes these renders reproducible.
fn fixture(lines: &[String]) -> Result<u::events::Log, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("u-shots-{}-events.log", std::process::id()));
    let mut text = String::from("u-log 1\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&path, text)?;
    Ok(u::events::Log::at(path))
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
