//! The screen flow, driven headlessly.
//!
//! These build a real `ElementTree` and drive it the way the app does, so they
//! check the wiring rather than the pixels: that the button opens the picker,
//! that confirming starts a render, that the render replaces the grid, and that
//! tapping a finished tile opens the comparison.
//!
//! `Inline` is the spawner throughout, so a render has completed by the time
//! `start_render` returns and the test never sleeps or polls a clock. That the
//! same code works with a real thread pool is the point of vieww taking `Spawn`
//! as a trait rather than owning an executor.

use std::rc::Rc;
use std::sync::Arc;

use upxcale::photos::{self, Library};
use upxcale::screens::landing::{landing_route, Landing};
use upxcale::state::{routes, AppState};

use vieww_asset::{AssetBundle, DirectoryBundle};
use vieww_element::{ElementTree, NavigatorController};
use vieww_foundation::task::{FrameWaker, Inline, NoWaker, Spawn};
use vieww_widget::prelude::*;

/// Build the app's state against the real asset bundle, with no window.
fn app() -> (ElementTree, AppState) {
    let mut tree = ElementTree::new();
    let runtime = tree.runtime().clone();

    let bundle: Rc<dyn AssetBundle> = Rc::new(DirectoryBundle::at(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets"
    )));
    let library = Rc::new(Library::new(bundle));

    let nav = NavigatorController::new(
        &runtime,
        Route::new("bootstrap", Rc::new(|_| SizedBox::shrink().into())),
    );
    let spawner: Rc<dyn Spawn> = Rc::new(Inline);
    let waker: Arc<dyn FrameWaker> = Arc::new(NoWaker);
    let state = AppState::new(runtime, library, nav.clone(), spawner, waker);

    nav.replace(landing_route(Landing {
        state: state.clone(),
    }));
    tree.set_root(Landing {
        state: state.clone(),
    });
    (tree, state)
}

#[test]
fn the_app_opens_on_a_full_grid() {
    let (_tree, state) = app();
    assert_eq!(
        state.grid.peek().len(),
        photos::CATALOGUE.len(),
        "the landing screen should open showing every photograph"
    );
    assert!(
        state.grid.peek().iter().all(|item| !item.upscaled),
        "nothing is upscaled before the user asks for it"
    );
    assert_eq!(
        state.nav.current_name().as_deref(),
        Some(routes::LANDING),
        "no popup is up on open — the picker is behind the Render button"
    );
}

#[test]
fn every_catalogue_photo_decodes() {
    let (_tree, state) = app();
    for photo in photos::CATALOGUE {
        let image = state
            .library
            .source(photo)
            .unwrap_or_else(|error| panic!("{}: {error}", photo.asset_path()));
        assert!(
            image.width() > 0 && image.height() > 0,
            "{} decoded to nothing",
            photo.slug
        );
    }
}

#[test]
fn picking_then_confirming_upscales_and_swaps_the_grid() {
    let (mut tree, state) = app();

    // The Render button pushes the picker.
    state
        .nav
        .push(upxcale::screens::landing::picker_route(state.clone()));
    assert_eq!(state.nav.current_name().as_deref(), Some(routes::PICKER));

    state.toggle_pick("a2");
    state.toggle_pick("a7");
    assert_eq!(state.pick_count(), 2);

    // Confirm.
    assert!(
        state.start_render(),
        "two picked photos should start a render"
    );
    state
        .nav
        .replace(upxcale::screens::landing::progress_route(state.clone()));

    // `Inline` ran the batch inside `start_render`; one drain finishes it.
    assert!(state.poll_render(), "the batch should complete in one poll");

    let grid = state.grid.peek();
    assert_eq!(grid.len(), 2, "the grid should now hold only the results");
    assert!(grid.iter().all(|item| item.upscaled));
    assert!(
        grid.iter().all(|item| item.fresh),
        "freshly rendered tiles should be marked for the glow"
    );
    assert_eq!(
        state.nav.current_name().as_deref(),
        Some(routes::LANDING),
        "finishing the render pops the progress route"
    );
    assert_eq!(
        state.pick_count(),
        0,
        "the selection is cleared after a run"
    );

    // The results are real and bigger than their sources.
    for item in &grid {
        let photo = item.photo().expect("a catalogue photo");
        let source = state.library.source(photo).expect("source");
        let result = state.library.result(photo).expect("result");
        assert_eq!(result.width(), source.width() * photos::UPSCALE_FACTOR);
        assert_eq!(result.height(), source.height() * photos::UPSCALE_FACTOR);
    }

    // The glow is transient.
    state.settle_grid();
    assert!(state.grid.peek().iter().all(|item| !item.fresh));

    // And the whole tree still builds in every one of those states.
    tree.rebuild_pending();
    assert!(tree.debug_tree().contains("PhotoGrid"));
}

#[test]
fn confirming_with_nothing_picked_does_nothing() {
    let (_tree, state) = app();
    state.clear_picks();
    assert!(
        !state.start_render(),
        "an empty selection must not start a render or push a route"
    );
    assert!(!state.is_rendering());
}

#[test]
fn the_compare_sheet_opens_on_a_rendered_photo() {
    let (mut tree, state) = app();

    state.toggle_pick("a1");
    assert!(state.start_render());
    assert!(state.poll_render());

    let photo = photos::by_id("a1").expect("a1");
    state.open_compare(photo.id);
    state
        .nav
        .push(upxcale::screens::landing::compare_route(&state));

    assert_eq!(state.nav.current_name().as_deref(), Some(routes::COMPARE));
    assert_eq!(state.comparing_photo().map(|p| p.id), Some("a1"));
    assert!((state.divider.peek() - 0.5).abs() < f32::EPSILON);

    // The divider clamps rather than escaping the stage.
    state.set_divider(-3.0);
    assert!((state.divider.peek() - 0.0).abs() < f32::EPSILON);
    state.set_divider(9.0);
    assert!((state.divider.peek() - 1.0).abs() < f32::EPSILON);

    tree.rebuild_pending();
    assert!(tree.debug_tree().contains("CompareSheet"));
}

#[test]
fn progress_reports_real_work() {
    let (_tree, state) = app();
    state.toggle_pick("a3");
    state.toggle_pick("a4");
    assert!(state.start_render());

    let before = state.progress.peek();
    assert_eq!(before.photos_total, 2);
    assert_eq!(before.stages_total, 4, "two stages per photograph");
    assert_eq!(before.stages_done, 0);
    assert!((before.fraction() - 0.0).abs() < f32::EPSILON);

    assert!(state.poll_render());

    let after = state.progress.peek();
    assert_eq!(after.stages_done, after.stages_total);
    assert_eq!(after.photos_done, 2);
    assert!((after.fraction() - 1.0).abs() < f32::EPSILON);
    assert_eq!(after.percent(), 100);
}

/// A dismissed picker leaves nothing behind — the bug the prototype had, where
/// the grid kept the selection after the popup closed.
#[test]
fn dismissing_the_picker_clears_the_selection() {
    let (_tree, state) = app();
    state
        .nav
        .push(upxcale::screens::landing::picker_route(state.clone()));
    state.toggle_pick("a6");
    assert_eq!(state.pick_count(), 1);

    state.clear_picks();
    state.nav.pop();

    assert_eq!(state.pick_count(), 0);
    assert_eq!(state.nav.current_name().as_deref(), Some(routes::LANDING));
    assert!(
        state.grid.peek().iter().all(|item| !item.upscaled),
        "dismissing the picker changes nothing about the grid"
    );
}

/// UP-01: the compare sheet's baseline comes from the worker, so opening the
/// sheet resamples nothing on the UI thread; and Save runs off it, says so
/// in the toast, and says how it ended.
#[test]
fn compare_needs_no_ui_thread_resample_and_save_reports_back() {
    let dir = std::env::temp_dir().join(format!("upxcale-up01-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    upxcale::export::set_export_dir(dir.clone());

    let (_tree, state) = app();
    state.toggle_pick("a2");
    assert!(state.start_render());
    assert!(state.poll_render());

    let photo = photos::by_id("a2").expect("a2");
    let baseline = state
        .library
        .baseline_cached(photo)
        .expect("the worker supplied the baseline");
    let result = state.library.result(photo).expect("result");
    assert_eq!((baseline.width(), baseline.height()), (result.width(), result.height()));

    state.open_compare("a2");
    // UX-02: with a share sheet registered, the saved PNG is offered to it.
    let sheet = std::rc::Rc::new(vieww_foundation::mobile::HeadlessShare::new());
    state.set_share(sheet.clone());
    assert!(state.save_current());
    // `Inline` ran the encode already; the toast is folded in on the next poll.
    state.poll_save();
    let message = state.toast.peek().expect("a toast after saving");
    assert!(message.starts_with("Saved to "), "{message}");
    let shared = sheet.shared();
    assert_eq!(shared.len(), 1, "the saved photo went to the share sheet");
    let (name, bytes) = shared[0].file.clone().expect("as a file");
    assert!(name.ends_with(".png") && bytes.starts_with(b"\x89PNG"), "{name}");
    assert!(std::fs::read_dir(&dir).unwrap().count() >= 1, "a file was written");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_baseline_cache_keeps_to_its_tier_budget() {
    let (_tree, state) = app();
    state.library.set_cache_budget(2);
    for id in ["a1", "a2", "a3", "a4"] {
        let photo = photos::by_id(id).expect("catalogue photo");
        let image = state.library.source(photo).expect("source");
        state.library.adopt_with_baseline(photo, image.clone(), image);
    }
    let held = ["a1", "a2", "a3", "a4"]
        .iter()
        .filter(|id| state.library.baseline_cached(photos::by_id(id).unwrap()).is_some())
        .count();
    assert_eq!(held, 2, "only the newest two baselines stay");
    // Results are the user's work and are never evicted.
    assert!(photos::by_id("a1").is_some_and(|p| state.library.is_upscaled(p)));
}
