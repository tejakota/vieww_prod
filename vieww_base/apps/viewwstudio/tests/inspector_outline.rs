//! The Inspector as an outline (ST-06), through the real pieces: a deep
//! guest tree laid out by a real `FrameDriver` and captured the way the
//! studio captures it, then folded, filtered, multi-selected and walked
//! with keys through the studio's own key dispatcher, and the pane itself
//! drawn by the real shell with the inspector tab open.

use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{Color, EdgeInsets, KeyEvent, Modifiers, NamedKey, Size, TargetPlatform};
use vieww_render::FrameDriver;
use vieww_widget::prelude::*;
use vieww_widget::Container;
use viewwstudio::state::RightTab;
use viewwstudio::{Shell, Studio};

/// Three cards, each with a title and two lines: a tree several levels
/// deep with repeated names, which is what a flat list makes hard to read.
fn deep_guest() -> WidgetNode {
    let card = |title: &str| -> WidgetNode {
        Container::new()
            .padding(EdgeInsets::all(8.0))
            .color(Color::WHITE)
            .child(Flex::column().children(children![
                Text::new(title.to_owned()),
                Text::new("first line"),
                Text::new("second line"),
            ]))
            .into()
    };
    Flex::column()
        .children(children![card("Alpha"), card("Beta"), card("Gamma")])
        .into()
}

fn captured_studio() -> Studio {
    let mut guest = FrameDriver::new(Size::new(400.0, 600.0));
    guest.set_root(deep_guest());
    guest.draw_frame();
    let studio = Studio::new(&Runtime::new()).with_host(TargetPlatform::Linux);
    studio.right_open.set(true);
    studio.right_tab.set(RightTab::Inspector);
    studio.capture_tree(&mut guest);
    assert!(
        studio.tree_snapshot.borrow().len() > 10,
        "a deep tree was captured"
    );
    studio
}

fn key(k: NamedKey) -> KeyEvent {
    KeyEvent::named(k, Duration::from_millis(10))
}

fn first_with_children(studio: &Studio, depth: usize) -> usize {
    studio
        .tree_snapshot
        .borrow()
        .iter()
        .position(|(d, n)| *d == depth && n.children > 0)
        .expect("a row with children at that depth")
}

#[test]
fn folding_a_row_hides_its_subtree_and_unfolding_brings_it_back() {
    let studio = captured_studio();
    let all = studio.inspector_rows().len();
    assert_eq!(
        all,
        studio.tree_snapshot.borrow().len(),
        "nothing folded: every row shows"
    );
    let row = first_with_children(&studio, 1);
    studio.inspector_toggle(row);
    let folded = studio.inspector_rows();
    assert!(
        folded.len() < all,
        "folding hid rows ({} of {all})",
        folded.len()
    );
    assert!(folded.contains(&row), "the folded row itself stays");
    studio.inspector_toggle(row);
    assert_eq!(studio.inspector_rows().len(), all);
}

#[test]
fn a_fold_survives_a_new_capture() {
    let studio = captured_studio();
    let row = first_with_children(&studio, 1);
    studio.inspector_toggle(row);
    let folded = studio.inspector_rows().len();
    // The guest renders again (a new tree, new ids): the fold is by path.
    let mut guest = FrameDriver::new(Size::new(400.0, 600.0));
    guest.set_root(deep_guest());
    guest.draw_frame();
    studio.tree_snapshot.borrow_mut().clear();
    studio.capture_tree(&mut guest);
    assert_eq!(studio.inspector_rows().len(), folded);
}

#[test]
fn the_filter_keeps_matches_and_their_ancestors() {
    let studio = captured_studio();
    // The padding rows: one per card, each two levels down.
    let name = "RenderPadding";
    studio.set_inspector_filter("padding".to_owned());
    let shown = studio.inspector_rows();
    let rows = studio.tree_snapshot.borrow();
    assert!(!shown.is_empty());
    assert!(
        shown.contains(&0),
        "the root stays as every match's ancestor"
    );
    assert!(shown.len() < rows.len());
    assert!(shown.iter().any(|&i| rows[i].1.name == name));
}

#[test]
fn the_arrow_keys_walk_the_tree_through_the_real_dispatcher() {
    let studio = captured_studio();
    let host = TargetPlatform::Linux;
    // Before a row is clicked the arrows are the editor's.
    assert!(
        !viewwstudio::ui::shortcuts::dispatch(&studio, &key(NamedKey::ArrowDown), host)
            || studio.inspect_selected.get().is_none()
    );
    studio.inspector_click(0, false, false);
    assert_eq!(studio.inspect_selected.get(), Some(0));
    assert!(viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &key(NamedKey::ArrowDown),
        host
    ));
    assert_eq!(studio.inspect_selected.get(), Some(1));
    // ← on an open row with children folds it (row 1 is the first card).
    let row = first_with_children(&studio, 1);
    assert_eq!(studio.inspect_selected.get(), Some(row));
    let before = studio.inspector_rows().len();
    assert!(viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &key(NamedKey::ArrowLeft),
        host
    ));
    assert!(studio.inspector_rows().len() < before, "← folded the row");
    assert!(viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &key(NamedKey::ArrowRight),
        host
    ));
    assert_eq!(studio.inspector_rows().len(), before, "→ unfolded it");
    assert!(viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &key(NamedKey::ArrowRight),
        host
    ));
    assert_eq!(
        studio.inspect_selected.get(),
        Some(row + 1),
        "→ on an open row goes to its first child"
    );
    assert!(viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &key(NamedKey::End),
        host
    ));
    assert_eq!(
        studio.inspect_selected.get(),
        Some(studio.tree_snapshot.borrow().len() - 1)
    );
    // A click in the editor hands the arrows back.
    studio.select_projected(vieww_foundation::TextSelection::collapsed(0));
    let at = studio.inspect_selected.get();
    let _ = viewwstudio::ui::shortcuts::dispatch(&studio, &key(NamedKey::ArrowUp), host);
    assert_eq!(
        studio.inspect_selected.get(),
        at,
        "the Inspector no longer takes the arrows"
    );
}

#[test]
fn modifier_clicks_build_a_selection_with_combined_bounds() {
    let studio = captured_studio();
    let host = TargetPlatform::Linux;
    studio.inspector_click(1, false, false);
    // ⌘/Ctrl held (as the key dispatcher records it), then a click adds.
    let _ = viewwstudio::ui::shortcuts::dispatch(
        &studio,
        &KeyEvent::named(NamedKey::Control, Duration::from_millis(5))
            .with_modifiers(Modifiers::CONTROL),
        host,
    );
    let m = studio.held_modifiers.get();
    studio.inspector_click(3, m.meta() || m.control(), m.shift());
    assert_eq!(studio.inspect_selection.get().len(), 2);
    studio.inspector_click(6, false, true);
    assert!(
        studio.inspect_selection.get().len() >= 4,
        "Shift-click selected the range"
    );
    let b = studio.inspector_selection_bounds().expect("bounds");
    assert!(b.width() > 0.0 && b.height() > 0.0);
}

#[test]
fn the_shell_draws_the_outline() {
    let mut driver = FrameDriver::new(Size::new(1440.0, 900.0));
    viewwstudio::install(&mut driver);
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    studio.right_open.set(true);
    studio.right_tab.set(RightTab::Inspector);
    let mut guest = FrameDriver::new(Size::new(400.0, 600.0));
    guest.set_root(deep_guest());
    guest.draw_frame();
    studio.capture_tree(&mut guest);
    driver.set_root(Shell {
        studio: studio.clone(),
    });
    driver.draw_frame();
    let count = |driver: &FrameDriver| {
        let tree = driver.renders();
        tree.describe_subtree(tree.root().expect("root"), 100_000)
            .iter()
            .filter(|(_, n)| {
                n.properties
                    .iter()
                    .any(|(_, v)| v.contains('\u{25be}') || v.contains('\u{25b8}'))
            })
            .count()
    };
    let arrows = count(&driver);
    assert!(
        arrows > 0,
        "rows with children are drawn with a disclosure arrow"
    );
    let row = first_with_children(&studio, 1);
    studio.inspector_toggle(row);
    driver.draw_frame();
    assert!(
        count(&driver) < arrows,
        "a folded row's children (and their arrows) are not drawn"
    );
}
