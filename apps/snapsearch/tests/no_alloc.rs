//! SS-03: no build closure allocates pixels. The counter is per thread and
//! builds run on this one; the test has a binary of its own so nothing else
//! shares its thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::Size;
use vieww::render::FrameDriver;

use snapsearch::embed::local::images_generated;
use snapsearch::photo;

#[test]
fn ambient_steps_and_tile_turnover_allocate_no_pixels() {
    std::env::set_var("SNAPSEARCH_PHOTOS", "/nonexistent-snapsearch-photos");
    let mut driver = FrameDriver::new(Size::new(390.0, 844.0));
    let slot = Rc::new(RefCell::new(None));
    snapsearch::app::build_root(&mut driver, &slot);
    let state = slot.borrow().clone().expect("state");
    state.photos.set(photo::sample_library(48));
    let mut now = Duration::ZERO;
    for _ in 0..120 {
        now += Duration::from_millis(16);
        driver.draw_frame_at(now);
    }
    let before = images_generated();
    // 700 ambient steps (~98 s): past every tile's first turnover (220-460
    // steps), so the stand-ins are shown, not only the drift.
    for _ in 0..700 * 9 {
        now += Duration::from_millis(16);
        driver.draw_frame_at(now);
    }
    assert_eq!(images_generated(), before, "a build made bitmaps");
}
