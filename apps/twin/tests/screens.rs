//! TW-01: the eight screens render headless, none overflows, none is blank,
//! and the morse is the morse.

use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;

use twin::{mount, pixel_runs, Motion, Screen, GROUND, SURFACE};

#[test]
fn all_eight_screens_render_without_overflow_and_are_not_blank() {
    vieww_render::overflow::forget_reported();
    let mut renderer = NativeRenderer::new();
    let mut driver = FrameDriver::new(SURFACE);
    let state = mount(&mut driver, Motion::Still);
    assert_eq!(Screen::ALL.len(), 8);
    for screen in Screen::ALL {
        state.screen.set(screen);
        driver.draw_frame();
        driver.draw_frame();
        let (pixels, _) = renderer
            .render_to_pixels(driver.scene(), SURFACE.width as u32, SURFACE.height as u32, GROUND)
            .expect("renders");
        let data = pixels.data();
        let bg = [GROUND.r, GROUND.g, GROUND.b];
        let inked = data.chunks(4).filter(|p| p[..3] != bg).count();
        assert!(inked > 4_000, "{} is not blank ({inked} px inked)", screen.name());
    }
    assert_eq!(
        vieww_render::overflow::reported(),
        0,
        "no screen overflows its box"
    );
}

#[test]
fn the_screen_signal_is_what_the_shell_shows() {
    let mut driver = FrameDriver::new(SURFACE);
    let state = mount(&mut driver, Motion::Still);
    driver.draw_frame();
    assert_eq!(state.screen.get(), Screen::Home);
    state.screen.set(Screen::Chat);
    driver.draw_frame();
    assert_eq!(state.screen.get(), Screen::Chat);
}

#[test]
fn ambient_motion_is_gated_on_the_low_tier_and_reduced_motion() {
    assert_eq!(Motion::gate(true, false), Motion::Still);
    assert_eq!(Motion::gate(false, true), Motion::Still);
    assert_eq!(Motion::gate(false, false), Motion::Ambient);
}

#[test]
fn morse_timing_is_one_three_five() {
    // "e" = dot; "t" = dash; "et" = dot, 3u gap, dash.
    assert_eq!(pixel_runs("e"), vec![(true, 1)]);
    assert_eq!(pixel_runs("t"), vec![(true, 3)]);
    assert_eq!(pixel_runs("et"), vec![(true, 1), (false, 3), (true, 3)]);
    // "i" = dot gap dot (1u); a word gap is 5u.
    assert_eq!(pixel_runs("i"), vec![(true, 1), (false, 1), (true, 1)]);
    assert_eq!(pixel_runs("e e"), vec![(true, 1), (false, 5), (true, 1)]);
}
