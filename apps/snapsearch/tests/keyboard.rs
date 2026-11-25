//! A11Y-02: a keyboard alone reaches the library's core control (the search
//! button), Enter activates it, and the focus ring shows (keyboard-driven).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::{Color, KeyEvent, LogicalKey, NamedKey, Size};
use vieww::paint::native::NativeRenderer;
use vieww::FrameDriver;

use snapsearch::photo;

// Needs `--features native` (the rasteriser): cargo test --release --features native --test keyboard

const W: f32 = 390.0;
const H: f32 = 844.0;

fn pixels(r: &mut NativeRenderer, d: &FrameDriver) -> Vec<u8> {
    r.render_to_pixels(d.scene(), W as u32, H as u32, Color::WHITE)
        .expect("renders")
        .0
        .data()
        .to_vec()
}

#[test]
fn tab_reaches_the_search_button_enter_opens_the_sheet_and_a_ring_shows() {
    std::env::set_var("SNAPSEARCH_PHOTOS", "/nonexistent-snapsearch-photos");
    let mut driver = FrameDriver::new(Size::new(W, H));
    let slot = Rc::new(RefCell::new(None));
    snapsearch::app::build_root(&mut driver, &slot);
    let state = slot.borrow().clone().expect("state");
    state.photos.set(photo::sample_library(12));
    let mut now = Duration::ZERO;
    for _ in 0..40 {
        now += Duration::from_millis(50);
        driver.draw_frame_at(now);
    }
    assert!(!state.sheet_open.peek(), "the sheet starts closed");

    let mut renderer = NativeRenderer::new();
    let baseline = pixels(&mut renderer, &driver);
    let mut ring_seen = false;
    for _ in 0..24 {
        driver.handle_key(&KeyEvent::down(LogicalKey::Named(NamedKey::Tab), Duration::ZERO));
        now += Duration::from_millis(50);
        driver.draw_frame_at(now);
        // Keyboard-driven focus draws a ring the baseline frame lacks.
        ring_seen |= pixels(&mut renderer, &driver) != baseline;
        driver.handle_key(&KeyEvent::down(LogicalKey::Named(NamedKey::Enter), Duration::ZERO));
        now += Duration::from_millis(50);
        driver.draw_frame_at(now);
        if state.sheet_open.peek() {
            break;
        }
    }
    assert!(ring_seen, "a keyboard-driven focus ring was drawn");
    assert!(state.sheet_open.peek(), "Enter on a tabbed control opened the search sheet");
}
