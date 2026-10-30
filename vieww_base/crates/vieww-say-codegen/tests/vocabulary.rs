//! ST-20's vocabulary, end to end: each fixture's generated Rust compiles
//! against vieww (the `include!`s below) and behaves when it is mounted in a
//! real frame driver, tapped and dragged through the real pointer path.

use std::time::Duration;

use vieww::foundation::{Offset, PointerEvent, PointerId, Size};
use vieww::render::FrameDriver;

/// One fixture as a module. The generated types are private to it, so the
/// module mounts itself and reads its own state for the tests.
macro_rules! fixture {
    ($name:ident, $file:literal $(, $item:item)*) => {
        #[allow(dead_code, clippy::all, clippy::pedantic)]
        mod $name {
            include!($file);
            pub(crate) fn mount() -> vieww::render::FrameDriver {
                super::mount(vieww::widget::WidgetNode::new(SayApp))
            }
            $($item)*
        }
    };
}

fixture!(image, "fixtures/image_gen.rs");
fixture!(
    scroll,
    "fixtures/scroll_gen.rs",
    pub(crate) fn offset(driver: &mut vieww::render::FrameDriver) -> (f32, f32) {
        super::state(driver, |s: &SayState| (s.ui_scroll_7, s.ui_scroll_7_max))
    }
);
fixture!(
    tabs,
    "fixtures/tabs_gen.rs",
    pub(crate) fn likes(driver: &mut vieww::render::FrameDriver) -> i64 {
        super::state(driver, |s: &SayState| s.likes)
    }
);
fixture!(
    theme,
    "fixtures/theme_gen.rs",
    pub(crate) fn themed(base: &ThemeData) -> ThemeData {
        say_theme(base)
    }
);
fixture!(
    transition,
    "fixtures/transition_gen.rs",
    pub(crate) fn nav(driver: &mut vieww::render::FrameDriver) -> (bool, f32) {
        super::state(driver, |s: &SayState| (s.nav_running, s.nav_t))
    }
);
fixture!(rust, "fixtures/rust_gen.rs");
fixture!(profile, "fixtures/profile_gen.rs");

fn mount(root: vieww::widget::WidgetNode) -> FrameDriver {
    let mut driver = FrameDriver::new(Size::new(400.0, 700.0));
    driver.set_root(root);
    driver.draw_frame();
    driver
}

/// Every line of text on screen, with where its middle is.
fn texts(driver: &FrameDriver) -> Vec<(String, Offset)> {
    let tree = driver.renders();
    let rows = tree.describe_subtree(tree.root().expect("root"), 100_000);
    let mut origin = vec![Offset::ZERO; rows.len()];
    let mut out = Vec::new();
    for i in 0..rows.len() {
        let depth = rows[i].0;
        let parent = (0..i).rev().find(|&p| rows[p].0 + 1 == depth);
        let base = parent.map_or(Offset::ZERO, |p| origin[p]);
        let node = &rows[i].1;
        origin[i] = Offset::new(base.dx + node.offset.dx, base.dy + node.offset.dy);
        if node.name == "RenderText" {
            for (key, value) in &node.properties {
                if *key == "text" {
                    out.push((
                        value.trim_matches('"').to_owned(),
                        Offset::new(
                            origin[i].dx + node.size.width / 2.0,
                            origin[i].dy + node.size.height / 2.0,
                        ),
                    ));
                }
            }
        }
    }
    out
}

fn shown(driver: &FrameDriver, text: &str) -> bool {
    texts(driver).iter().any(|(t, _)| t == text)
}

fn at(driver: &FrameDriver, text: &str) -> Offset {
    texts(driver)
        .into_iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("{text:?} is not on screen: {:?}", texts(driver)))
        .1
}

fn tap(driver: &mut FrameDriver, text: &str, t: &mut u64) {
    let point = at(driver, text);
    driver.handle_pointer(&PointerEvent::down(
        PointerId(1),
        point,
        Duration::from_millis(*t),
    ));
    driver.draw_frame_at(Duration::from_millis(*t));
    driver.handle_pointer(&PointerEvent::up(
        PointerId(1),
        point,
        Duration::from_millis(*t + 30),
    ));
    driver.draw_frame_at(Duration::from_millis(*t + 30));
    driver.draw_frame_at(Duration::from_millis(*t + 50));
    *t += 100;
}

fn state<S: vieww::widget::ElementState + 'static, R>(
    driver: &mut FrameDriver,
    read: impl FnOnce(&S) -> R,
) -> R {
    driver
        .elements()
        .find("SayApp")
        .and_then(|e| e.state_as::<S, R>(read))
        .expect("the app keeps its state")
}

/// How far right the slide transition has the page, from the widget the
/// generated code built.
fn slide_of(driver: &mut FrameDriver) -> f32 {
    let elements = driver.elements();
    let transformed = elements
        .find("Transformed")
        .expect("the page is in a transform");
    let widget = transformed
        .widget()
        .downcast_ref::<vieww::widget::Transformed>()
        .expect("a Transformed widget");
    widget.matrix().apply(Offset::ZERO).dx
}

#[test]
fn an_image_shows_the_file_and_a_missing_one_says_so() {
    let mut driver = image::mount();
    let elements = driver.elements();
    assert_eq!(elements.find_all("Image").len(), 1, "the dot decoded");
    let placeholder = elements.find_all("ErrorPlaceholder");
    assert_eq!(placeholder.len(), 1, "the missing file is shown as missing");
    let tree = driver.renders();
    let rows = tree.describe_subtree(tree.root().expect("root"), 10_000);
    let image = rows
        .iter()
        .find(|(_, n)| n.name == "RenderImage")
        .expect("a picture was laid out");
    assert_eq!(
        (image.1.size.width, image.1.size.height),
        (64.0, 64.0),
        "`64 wide, 64 tall` sizes it"
    );
}

#[test]
fn a_scroll_view_follows_a_drag_and_stops_at_the_end() {
    let mut driver = scroll::mount();
    let max = scroll::offset(&mut driver).1;
    assert!(max > 0.0, "twelve cards are taller than the screen: {max}");
    let ada = at(&driver, "Ada");

    // Drag up by 200 px, in steps the way a finger moves.
    let mut y = 500.0;
    driver.handle_pointer(&PointerEvent::down(
        PointerId(1),
        Offset::new(200.0, y),
        Duration::from_millis(0),
    ));
    for step in 1..=10u64 {
        let next = y - 20.0;
        driver.handle_pointer(&PointerEvent::moved(
            PointerId(1),
            Offset::new(200.0, y),
            Offset::new(200.0, next),
            Duration::from_millis(step * 16),
        ));
        y = next;
        driver.draw_frame_at(Duration::from_millis(step * 16));
    }
    driver.handle_pointer(&PointerEvent::up(
        PointerId(1),
        Offset::new(200.0, y),
        Duration::from_millis(200),
    ));
    driver.draw_frame_at(Duration::from_millis(220));
    let offset = scroll::offset(&mut driver).0;
    assert!(
        offset > 100.0 && offset <= max,
        "scrolled {offset} of {max}"
    );
    let moved = at(&driver, "Ada");
    assert!(
        moved.dy < ada.dy - 100.0 || !shown(&driver, "Ada"),
        "the content moved up: {ada:?} -> {moved:?}"
    );

    // A long drag stops at the end.
    driver.handle_pointer(&PointerEvent::down(
        PointerId(2),
        Offset::new(200.0, 600.0),
        Duration::from_millis(300),
    ));
    let mut y = 600.0;
    for step in 1..=40u64 {
        let next = y - 40.0;
        driver.handle_pointer(&PointerEvent::moved(
            PointerId(2),
            Offset::new(200.0, y),
            Offset::new(200.0, next),
            Duration::from_millis(300 + step * 16),
        ));
        y = next;
        driver.draw_frame_at(Duration::from_millis(300 + step * 16));
    }
    let offset = scroll::offset(&mut driver).0;
    assert!(
        (offset - max).abs() < 0.5,
        "clamped at the end: {offset} vs {max}"
    );
}

#[test]
fn tabs_switch_and_keep_their_own_state() {
    let mut driver = tabs::mount();
    let mut t = 10;
    assert!(shown(&driver, "No posts yet"));
    assert!(!shown(&driver, "0 likes"), "only the selected tab is built");
    tap(&mut driver, "Likes", &mut t);
    assert!(shown(&driver, "0 likes"), "{:?}", texts(&driver));
    assert!(!shown(&driver, "No posts yet"));
    tap(&mut driver, "Like", &mut t);
    assert!(
        shown(&driver, "1 likes"),
        "the tab's button reached the state"
    );
    tap(&mut driver, "Posts", &mut t);
    assert!(shown(&driver, "No posts yet"));
    assert_eq!(tabs::likes(&mut driver), 1);
}

#[test]
fn the_theme_block_sets_the_slots_it_names_and_no_others() {
    let base = vieww::widget::ThemeData::default();
    let themed = theme::themed(&base);
    assert_eq!(themed.colors.primary, vieww::prelude::Color::hex(0x6750A4));
    assert_eq!(
        themed.colors.on_primary,
        vieww::prelude::Color::hex(0xFFFFFF)
    );
    assert_eq!(themed.colors.surface, vieww::prelude::Color::hex(0xFFFBFE));
    assert_eq!(
        themed.colors.outline, base.colors.outline,
        "an unnamed slot is the host's"
    );
    let mut driver = theme::mount();
    let elements = driver.elements();
    assert!(
        !elements.find_all("Theme").is_empty(),
        "the app is wrapped, so widgets that read the theme see it"
    );
    assert!(shown(&driver, "Themed"));
}

#[test]
fn a_screen_comes_up_with_its_transition() {
    let mut driver = transition::mount();
    let mut t = 10;
    tap(&mut driver, "Details", &mut t);
    let running = transition::nav(&mut driver);
    assert!(running.0, "the slide is running");
    assert!(running.1 < 1.0, "and is not done: {}", running.1);
    // The page is part way in from the right: translated by a fraction of
    // the screen's width.
    let shift = slide_of(&mut driver);
    assert!(shift > 0.0 && shift < 400.0, "sliding in: {shift}");
    driver.draw_frame_at(Duration::from_millis(t + 400));
    driver.draw_frame_at(Duration::from_millis(t + 420));
    let done = transition::nav(&mut driver);
    assert_eq!(done, (false, 1.0), "finished after 300 ms");
    assert!(
        slide_of(&mut driver).abs() < 0.01,
        "settled where it belongs"
    );
    t += 500;
    tap(&mut driver, "Back", &mut t);
    tap(&mut driver, "Settings", &mut t);
    assert!(transition::nav(&mut driver).0, "the fade runs");
    assert!(
        !driver.elements().find_all("Opacity").is_empty(),
        "as an opacity"
    );
}

#[test]
fn rust_blocks_run_and_read_the_state() {
    let mut driver = rust::mount();
    let mut t = 10;
    assert!(shown(&driver, "***"), "{:?}", texts(&driver));
    assert!(shown(&driver, "3 of 5"));
    tap(&mut driver, "More", &mut t);
    assert!(shown(&driver, "****"));
    assert!(shown(&driver, "4 of 5"));
}

#[test]
fn the_phrases_compose_with_dialogs_and_loops() {
    let mut driver = profile::mount();
    let mut t = 10;
    assert!(
        shown(&driver, "a") && shown(&driver, "b"),
        "loop text in a scroll view in a tab"
    );
    tap(&mut driver, "Likes", &mut t);
    assert!(shown(&driver, "0 likes"));
    tap(&mut driver, "Ask", &mut t);
    assert!(shown(&driver, "Name"), "the dialog opened over the tabs");
}
