//! A11Y-02: the compare sheet's divider is a keyboard control — Tab reaches
//! it, the arrows move it, Home and End jump to the ends — and it tells a
//! screen reader it is a slider and where it stands.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use upxcale::photos;
use upxcale::screens::landing::compare_sheet::{divider_key, CompareSheet};
use vieww::foundation::{KeyEvent, LogicalKey, NamedKey, Size};
use vieww::FrameDriver;

fn down(k: NamedKey) -> KeyEvent {
    KeyEvent::down(LogicalKey::Named(k), Duration::ZERO)
}

fn sheet(divider: f32, moved: Rc<Cell<Option<f32>>>) -> CompareSheet {
    CompareSheet {
        photo: photos::by_id("a1").expect("a1"),
        before: None,
        after: None,
        baseline: None,
        divider,
        on_divider: Rc::new(move |t| moved.set(Some(t))),
        on_close: Rc::new(|| {}),
        on_save: Rc::new(|| {}),
    }
}

#[test]
fn tab_reaches_the_divider_and_the_arrows_move_it() {
    let moved = Rc::new(Cell::new(None));
    let mut driver = FrameDriver::new(Size::new(390.0, 844.0));
    driver.set_root(sheet(0.5, moved.clone()));
    driver.draw_frame();
    // Tab through the sheet until an arrow moves the divider (the close
    // button comes first in some orders; a dozen stops is plenty).
    for _ in 0..12 {
        driver.handle_key(&down(NamedKey::Tab));
        driver.draw_frame();
        driver.handle_key(&down(NamedKey::ArrowRight));
        if moved.get().is_some() {
            break;
        }
    }
    let t = moved.get().expect("an arrow reached the divider");
    assert!((t - 0.55).abs() < 1e-4, "ArrowRight is one 5% step: {t}");

    let semantics = format!("{:?}", driver.semantics());
    assert!(semantics.contains("Slider"), "the divider is announced as a slider");
    assert!(semantics.contains("50% upscaled"), "with its value");
}

#[test]
fn the_slider_keys_and_only_those() {
    let key = |k| divider_key(0.5, &down(k));
    assert_eq!(key(NamedKey::ArrowLeft), Some(0.45));
    assert_eq!(key(NamedKey::PageUp), Some(0.75));
    assert_eq!(key(NamedKey::Home), Some(0.0));
    assert_eq!(key(NamedKey::End), Some(1.0));
    assert_eq!(divider_key(0.98, &down(NamedKey::ArrowRight)), Some(1.0), "clamped");
    assert_eq!(key(NamedKey::Tab), None, "Tab still moves focus");
    assert_eq!(key(NamedKey::Enter), None);
}
