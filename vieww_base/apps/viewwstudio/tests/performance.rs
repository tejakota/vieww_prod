//! ST-09 inside the studio: the performance overlay over the previewed
//! screen, and "Attribute This Frame" on the real preview pane with the real
//! Say counter in it.

use std::time::Duration;

use vieww_foundation::{Size, TargetPlatform};
use vieww_render::FrameDriver;
use viewwstudio::loaded::Preview;
use viewwstudio::ui::preview::{PreviewPane, PERF_OVERLAY};
use viewwstudio::Studio;

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

fn setup() -> (FrameDriver, Studio) {
    let mut driver = FrameDriver::new(Size::new(900.0, 900.0));
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    studio
        .preview_screen
        .set(Some(Preview::from_widget(counter::screen())));
    driver.set_root(PreviewPane {
        studio: studio.clone(),
    });
    driver.draw_frame();
    (driver, studio)
}

fn overlay_samples(driver: &mut FrameDriver) -> Option<Vec<Duration>> {
    let elements = driver.elements();
    let found = elements.find_all(PERF_OVERLAY);
    assert!(found.len() <= 1, "one overlay at most");
    found.first().map(|e| {
        e.widget()
            .downcast_ref::<vieww_widget::PerformanceOverlay>()
            .expect("the overlay widget")
            .samples()
            .to_vec()
    })
}

#[test]
fn the_overlay_shows_the_frames_it_is_given_and_only_while_on() {
    let (mut driver, studio) = setup();
    let budget = Duration::from_micros(16_667);
    let samples = vec![Duration::from_millis(6), Duration::from_millis(21)];

    studio.note_frames(samples.clone(), budget);
    assert!(studio.perf_samples.peek().is_empty(), "ignored while off");
    assert_eq!(overlay_samples(&mut driver), None);

    studio.toggle_perf_overlay();
    studio.note_frames(samples.clone(), budget);
    driver.draw_frame_at(Duration::from_millis(16));
    assert_eq!(
        overlay_samples(&mut driver),
        Some(samples.clone()),
        "the frame hook's samples reach the graph"
    );

    let next = vec![Duration::from_millis(9)];
    studio.note_frames(next.clone(), budget);
    driver.draw_frame_at(Duration::from_millis(32));
    assert_eq!(overlay_samples(&mut driver), Some(next), "and follow it");

    studio.toggle_perf_overlay();
    driver.draw_frame_at(Duration::from_millis(48));
    assert_eq!(overlay_samples(&mut driver), None, "off takes it away");
}

#[test]
fn attributing_a_frame_breaks_the_screen_down_and_annotates_the_rows() {
    let (mut driver, studio) = setup();
    assert!(!studio.run_attribution(&mut driver), "nothing asked for");

    studio.request_attribution();
    driver.draw_frame_at(Duration::from_millis(16));
    assert!(
        studio.run_attribution(&mut driver),
        "the frame carries it out"
    );
    assert!(!studio.run_attribution(&mut driver), "once");

    let attribution = studio.attribution.borrow().clone().expect("a result");
    let screen = attribution.screen.clone().expect("the screen alone");
    assert!(
        screen.glyph_commands >= 3,
        "the heading, the label and the button's text: {screen:?}"
    );
    assert!(screen.total_ms >= screen.floor_ms, "{screen:?}");
    assert!(
        !attribution.rows.is_empty(),
        "the Inspector's rows were measured"
    );

    // The row that draws "Add one" carries a cost, and it is that text's
    // one glyph run.
    let rows = studio.tree_snapshot.borrow().clone();
    let add_one = rows
        .iter()
        .find(|(_, n)| {
            n.properties
                .iter()
                .any(|(k, v)| *k == "text" && v.contains("Add one"))
        })
        .expect("the button's label is a row");
    let cost = attribution.row(add_one.1.id).expect("and it was measured");
    assert_eq!(cost.commands, 1, "{cost:?}");

    // ST-06/ST-22 on the real path: with the Inspector open, the screen is
    // still mounted above it, so its tree and its kept values are read.
    assert!(
        rows.iter().all(|(_, n)| !n
            .properties
            .iter()
            .any(|(_, v)| v.contains("Generated Rust"))),
        "the rows are the screen's, not the studio's chrome"
    );
    let count = studio
        .preview_fields
        .borrow()
        .iter()
        .flat_map(|(_, _, f)| f.iter())
        .find(|(k, _)| k == "count")
        .map(|(_, v)| v.clone());
    assert_eq!(count.as_deref(), Some("0"));
}

#[test]
fn attributing_needs_a_rendered_screen() {
    let mut driver = FrameDriver::new(Size::new(600.0, 600.0));
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    studio.request_attribution();
    assert!(
        !studio.run_attribution(&mut driver),
        "refused, with a reason"
    );
    assert!(studio.attribution.borrow().is_none());
}
