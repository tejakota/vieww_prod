//! The device matrix (ST-21): one screen on several devices side by side,
//! mounted through the real preview pane, with a tap in one frame carried
//! to the others. The screen is the real Say-generated counter (the codegen
//! fixture), built in process.

use std::time::Duration;

use vieww_foundation::{Offset, PointerEvent, PointerId, Size, TargetPlatform};
use vieww_render::FrameDriver;
use viewwstudio::loaded::Preview;
use viewwstudio::ui::preview::{PreviewPane, MATRIX_FRAME};
use viewwstudio::Studio;

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

fn setup() -> (FrameDriver, Studio) {
    let mut driver = FrameDriver::new(Size::new(1800.0, 900.0));
    // The studio's signals live in the driver's runtime, as in `main.rs`,
    // so a change to the matrix rebuilds the pane.
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    studio
        .preview_screen
        .set(Some(Preview::from_widget(counter::screen())));
    studio.toggle_device_matrix();
    driver.set_root(PreviewPane {
        studio: studio.clone(),
    });
    driver.draw_frame();
    (driver, studio)
}

/// Each frame's kept `count`, in frame order.
fn counts(driver: &mut FrameDriver) -> Vec<Option<String>> {
    let elements = driver.elements();
    elements
        .find_all(MATRIX_FRAME)
        .iter()
        .map(|f| {
            elements
                .snapshot_states(f.id())
                .into_iter()
                .find_map(|(_, v)| v.contains("count").then_some(v))
        })
        .collect()
}

/// Every place the text `label` was laid out, in the root's coordinates.
///
/// The device frames are scaled by `Fitted` (its child is laid out at the
/// device's full size and drawn into a smaller box), so the walk carries a
/// scale as well as an origin.
fn centers_of_text(driver: &FrameDriver, label: &str) -> Vec<Offset> {
    let tree = driver.renders();
    let rows = tree.describe_subtree(tree.root().expect("root"), 100_000);
    // For each row: where its own coordinates start, and how much one of
    // its units measures on screen.
    let mut origin = vec![Offset::ZERO; rows.len()];
    let mut unit = vec![1.0f32; rows.len()];
    for i in 0..rows.len() {
        let (o, k) = match viewwstudio::outline::parent(&rows[..], i) {
            None => (Offset::ZERO, 1.0),
            Some(p) => {
                let (pn, n) = (&rows[p].1, &rows[i].1);
                let k = if pn.name == "RenderFitted" && n.size.width > 0.0 {
                    unit[p] * pn.size.width / n.size.width
                } else {
                    unit[p]
                };
                (
                    Offset::new(
                        origin[p].dx + n.offset.dx * unit[p],
                        origin[p].dy + n.offset.dy * unit[p],
                    ),
                    k,
                )
            }
        };
        origin[i] = o;
        unit[i] = k;
    }
    rows.iter()
        .enumerate()
        .filter(|(_, (_, n))| n.properties.iter().any(|(_, v)| v.contains(label)))
        .map(|(i, (_, n))| {
            let k = unit[i];
            Offset::new(
                origin[i].dx + n.size.width * k / 2.0,
                origin[i].dy + n.size.height * k / 2.0,
            )
        })
        .collect()
}

fn tap(driver: &mut FrameDriver, at: Offset, t: u64) {
    driver.handle_pointer(&PointerEvent::down(
        PointerId(1),
        at,
        Duration::from_millis(t),
    ));
    driver.draw_frame_at(Duration::from_millis(t));
    driver.handle_pointer(&PointerEvent::up(
        PointerId(1),
        at,
        Duration::from_millis(t + 30),
    ));
    driver.draw_frame_at(Duration::from_millis(t + 30));
    driver.draw_frame_at(Duration::from_millis(t + 50));
}

#[test]
fn three_devices_render_the_same_screen() {
    let (mut driver, studio) = setup();
    assert_eq!(studio.preview_matrix.peek().len(), 3);
    let elements = driver.elements();
    assert_eq!(elements.find_all(MATRIX_FRAME).len(), 3, "one frame each");
    assert_eq!(
        centers_of_text(&driver, "Add one").len(),
        3,
        "the screen is laid out in every frame"
    );
}

#[test]
fn a_tap_in_one_frame_shows_in_the_others() {
    let (mut driver, studio) = setup();
    studio.sync_matrix_states(&mut driver);
    let before = counts(&mut driver);
    assert_eq!(before.len(), 3);
    assert!(before.iter().all(|c| c == &before[0]), "{before:?}");

    let buttons = centers_of_text(&driver, "Add one");
    tap(&mut driver, buttons[0], 10);
    let tapped = counts(&mut driver);
    assert_ne!(tapped[0], before[0], "the tap reached the first frame");
    assert_eq!(tapped[1], before[1], "and only the first, before syncing");

    assert_eq!(studio.sync_matrix_states(&mut driver), 2);
    driver.draw_frame_at(Duration::from_millis(100));
    let synced = counts(&mut driver);
    assert!(synced.iter().all(|c| c == &tapped[0]), "{synced:?}");
    assert_eq!(studio.sync_matrix_states(&mut driver), 0, "settled");

    // And from the last frame back to the first.
    let buttons = centers_of_text(&driver, "Add one");
    tap(&mut driver, buttons[2], 200);
    let again = counts(&mut driver);
    assert_ne!(again[2], synced[2]);
    assert_eq!(studio.sync_matrix_states(&mut driver), 2);
    driver.draw_frame_at(Duration::from_millis(300));
    let all = counts(&mut driver);
    assert!(all.iter().all(|c| c == &again[2]), "{all:?}");
}

#[test]
fn removing_and_adding_devices() {
    let (mut driver, studio) = setup();
    studio.matrix_add_device();
    driver.draw_frame_at(Duration::from_millis(16));
    assert_eq!(driver.elements().find_all(MATRIX_FRAME).len(), 4);
    studio.matrix_remove_device(0);
    studio.matrix_remove_device(0);
    driver.draw_frame_at(Duration::from_millis(32));
    // Two left: still a matrix.
    assert_eq!(driver.elements().find_all(MATRIX_FRAME).len(), 2);
    studio.toggle_device_matrix();
    driver.draw_frame_at(Duration::from_millis(48));
    assert_eq!(driver.elements().find_all(MATRIX_FRAME).len(), 0, "closed");
}
