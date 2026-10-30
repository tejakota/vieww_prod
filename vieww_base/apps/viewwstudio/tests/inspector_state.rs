//! The inspector shows the previewed screen's state (ST-22): the real
//! Say-generated counter (the codegen fixture) mounted under the real
//! `GuestRoot`, captured the way the studio captures it, tapped through
//! the driver's real pointer path, and captured again.

use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{Offset, PointerEvent, PointerId, Size, TargetPlatform};
use vieww_render::FrameDriver;
use viewwstudio::state::RightTab;
use viewwstudio::ui::preview::GuestRoot;
use viewwstudio::Studio;

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

fn count_of(studio: &Studio) -> Option<String> {
    studio
        .preview_fields
        .borrow()
        .iter()
        .flat_map(|(_, _, fields)| fields.iter())
        .find(|(name, _)| name == "count")
        .map(|(_, v)| v.clone())
}

/// Where the text `label` was laid out, in the root's coordinates.
fn center_of_text(driver: &FrameDriver, label: &str) -> Offset {
    let tree = driver.renders();
    let rows = tree.describe_subtree(tree.root().expect("root"), 10_000);
    let mut origin = vec![Offset::ZERO; rows.len()];
    for i in 0..rows.len() {
        let parent = viewwstudio::outline::parent(&rows[..], i);
        let base = parent.map_or(Offset::ZERO, |p| origin[p]);
        origin[i] = Offset::new(base.dx + rows[i].1.offset.dx, base.dy + rows[i].1.offset.dy);
    }
    let i = rows
        .iter()
        .position(|(_, n)| n.properties.iter().any(|(_, v)| v.contains(label)))
        .unwrap_or_else(|| panic!("no text {label:?} on screen"));
    Offset::new(
        origin[i].dx + rows[i].1.size.width / 2.0,
        origin[i].dy + rows[i].1.size.height / 2.0,
    )
}

#[test]
fn a_tap_in_the_preview_shows_up_in_the_inspector() {
    let mut driver = FrameDriver::new(Size::new(400.0, 700.0));
    let studio = Studio::new(&Runtime::new()).with_host(TargetPlatform::Linux);
    studio.right_open.set(true);
    studio.right_tab.set(RightTab::Inspector);
    driver.set_root(GuestRoot {
        child: vieww_widget::WidgetNode::new(counter::screen()),
    });
    driver.draw_frame();
    studio.capture_tree(&mut driver);
    assert_eq!(
        count_of(&studio).as_deref(),
        Some("0"),
        "the kept value is shown"
    );
    let generation = studio.tree_generation.get();

    let at = center_of_text(&driver, "Add one");
    driver.handle_pointer(&PointerEvent::down(
        PointerId(1),
        at,
        Duration::from_millis(10),
    ));
    driver.draw_frame_at(Duration::from_millis(10));
    driver.handle_pointer(&PointerEvent::up(
        PointerId(1),
        at,
        Duration::from_millis(40),
    ));
    driver.draw_frame_at(Duration::from_millis(40));
    driver.draw_frame_at(Duration::from_millis(60));

    studio.capture_tree(&mut driver);
    assert_eq!(
        count_of(&studio).as_deref(),
        Some("1"),
        "the tap reached the state and the inspector"
    );
    assert_ne!(
        studio.tree_generation.get(),
        generation,
        "the pane is woken to redraw"
    );
}

#[test]
fn a_screen_that_keeps_nothing_shows_no_state() {
    use vieww_widget::prelude::*;
    let mut driver = FrameDriver::new(Size::new(400.0, 700.0));
    let studio = Studio::new(&Runtime::new()).with_host(TargetPlatform::Linux);
    studio.right_open.set(true);
    studio.right_tab.set(RightTab::Inspector);
    driver.set_root(GuestRoot {
        child: Text::new("static").into(),
    });
    driver.draw_frame();
    studio.capture_tree(&mut driver);
    assert!(studio.preview_fields.borrow().is_empty());
}
