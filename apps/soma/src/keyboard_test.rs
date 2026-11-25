//! A11Y-02 (a unit test, not `tests/`: under `panic = "abort"` in the release
//! profile an integration test links a second copy of the foundation crate).
//!
//! A11Y-02: a keyboard alone reaches soma's camera controls, activates one,
//! and shows the focus ring while it does.

use std::time::Duration;

use vieww::foundation::{KeyEvent, LogicalKey, NamedKey, Offset, PointerEvent, PointerId};
use vieww::FrameDriver;
use vieww_paint::native::NativeRenderer;

use crate as soma;

fn key(driver: &mut FrameDriver, k: NamedKey) {
    driver.handle_key(&KeyEvent::down(LogicalKey::Named(k), Duration::ZERO));
    driver.draw_frame();
    driver.draw_frame();
}

fn pixels(r: &mut NativeRenderer, d: &FrameDriver) -> Vec<u8> {
    let s = soma::SURFACE;
    r.render_to_pixels(d.scene(), s.width as u32, s.height as u32, soma::GROUND)
        .expect("renders")
        .0
        .data()
        .to_vec()
}

#[test]
fn tab_reaches_a_camera_button_enter_activates_it_and_a_ring_shows() {
    let mut driver = FrameDriver::new(soma::SURFACE);
    driver.set_fonts(soma::fonts());
    let state = soma::mount(&mut driver);
    for _ in 0..3 {
        driver.draw_frame();
    }
    let mut renderer = NativeRenderer::new();
    assert!(!state.rig.borrow().camera_is_user(), "the camera starts at rest");

    // Tab until Enter moves the camera: some stop on the way is a camera
    // button. Twelve stops is far more than the screen has.
    let mut ring_seen = false;
    let mut activated = false;
    for _ in 0..12 {
        key(&mut driver, NamedKey::Tab);
        let with_ring = pixels(&mut renderer, &driver);
        // A press elsewhere ends keyboard modality and hides the ring...
        driver.handle_pointer(&PointerEvent::down(PointerId(1), Offset::new(2.0, 2.0), Duration::ZERO));
        driver.draw_frame();
        driver.draw_frame();
        let without = pixels(&mut renderer, &driver);
        ring_seen |= with_ring != without;
        // ...so Tab again brings the focus back and Enter acts on it.
        key(&mut driver, NamedKey::Tab);
        key(&mut driver, NamedKey::Enter);
        if state.rig.borrow().camera_is_user() {
            activated = true;
            break;
        }
    }
    assert!(ring_seen, "the keyboard focus ring is drawn on some control");
    assert!(activated, "Tab + Enter reaches and presses a camera button");
}
