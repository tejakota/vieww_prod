//! SS-01..05 acceptance: the library screen is not rebuilt by the ambient
//! clock, a long library builds only what is near the viewport, and nothing
//! allocates photos on an idle frame.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::Size;
use vieww::render::FrameDriver;

use snapsearch::photo;

fn driver_with(count: usize) -> (FrameDriver, snapsearch::state::AppState, Duration) {
    // No photo directory: the generated set, deterministic and offline.
    std::env::set_var("SNAPSEARCH_PHOTOS", "/nonexistent-snapsearch-photos");
    let mut driver = FrameDriver::new(Size::new(390.0, 844.0));
    let slot = Rc::new(RefCell::new(None));
    snapsearch::app::build_root(&mut driver, &slot);
    let state = slot.borrow().clone().expect("state");
    state.photos.set(photo::sample_library(count));
    let mut now = Duration::ZERO;
    for _ in 0..6 {
        now += Duration::from_millis(16);
        driver.draw_frame_at(now);
    }
    (driver, state, now)
}

fn count(tree: &str, needle: &str) -> usize {
    tree.matches(needle).count()
}

#[test]
fn a_four_hundred_photo_library_builds_only_the_tiles_near_the_viewport() {
    let (mut driver, _state, _) = driver_with(400);
    let tree = driver.elements().debug_tree();
    let tiles = count(&tree, "Ambient");
    // 390x844 phone: two columns, ~3 tiles per column per screen, a viewport
    // of overscan each side — far fewer than 400.
    assert!(tiles > 0, "something is on screen");
    assert!(tiles < 60, "built {tiles} tiles of 400");
}

#[test]
fn scrolling_moves_the_built_window_rather_than_growing_it() {
    let (mut driver, state, mut now) = driver_with(400);
    let at_top = count(&driver.elements().debug_tree(), "Ambient");
    state.scroll.jump_to(20_000.0);
    for _ in 0..8 {
        now += Duration::from_millis(16);
        state.sync_scroll();
        driver.draw_frame_at(now);
    }
    let deep = count(&driver.elements().debug_tree(), "Ambient");
    assert!(deep > 0 && deep < 60, "built {deep} tiles deep in the list");
    assert!(deep <= at_top + 20, "the window did not grow: {at_top} -> {deep}");
}

#[test]
fn an_idle_library_does_not_rebuild_the_screen_when_the_clock_ticks() {
    let (mut driver, state, mut now) = driver_with(48);
    // Ambient motion off: the screen is genuinely idle, as under reduce
    // motion or on a low tier.
    driver.set_accessibility(vieww::foundation::Accessibility::new(true, false, 1.0));
    for _ in 0..10 {
        now += Duration::from_millis(16);
        driver.draw_frame_at(now);
    }
    let before = driver.elements().debug_tree();
    for _ in 0..30 {
        state.advance_clock();
        now += Duration::from_millis(140);
        driver.draw_frame_at(now);
    }
    assert_eq!(
        before,
        driver.elements().debug_tree(),
        "30 clock ticks changed the tree of an idle screen"
    );
}
