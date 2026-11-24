#![cfg(feature = "native")]
//! A11Y-02: a `Pressable` shows a ring when Tab reaches it, and not when a tap
//! did (the `:focus-visible` rule).

use std::time::Duration;

use vieww::foundation::{Color, KeyEvent, LogicalKey, NamedKey, PointerEvent, PointerId, Size};
use vieww::prelude::*;
use vieww::{BuildContext, Widget, WidgetKind, WidgetNode};
use vieww_test_harness::visual::render;
use vieww_widget::Pressable;

const SIDE: f32 = 100.0;

#[derive(Debug)]
struct Screen;

impl Widget for Screen {
    fn debug_name(&self) -> &'static str {
        "Screen"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        Center::new()
            .child(Pressable::new(|_| SizedBox::square(40.0).into()).on_tap(|| {}))
            .into()
    }
}
vieww_widget::widget_node_from!(Screen);

/// How many pixels in the frame differ from the white ground.
fn inked(driver: &FrameDriver) -> usize {
    let frame = render(driver, Size::square(SIDE), Color::WHITE);
    let mut n = 0;
    for y in 0..SIDE as u32 {
        for x in 0..SIDE as u32 {
            if frame.at(x, y) != (255, 255, 255, 255) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn tab_draws_a_ring_and_a_tap_does_not() {
    let mut driver = FrameDriver::new(Size::square(SIDE));
    driver.set_root(Screen);
    driver.draw_frame();
    assert_eq!(inked(&driver), 0, "nothing is drawn before any focus");

    driver.handle_key(&KeyEvent::down(LogicalKey::Named(NamedKey::Tab), Duration::ZERO));
    driver.draw_frame();
    assert!(inked(&driver) > 100, "Tab shows a ring");

    // A pointer press anywhere ends keyboard modality, ring included.
    let down = PointerEvent::down(PointerId(1), vieww::foundation::Offset::new(2.0, 2.0), Duration::ZERO);
    driver.handle_pointer(&down);
    driver.draw_frame();
    assert_eq!(inked(&driver), 0, "a pointer press hides the ring");
}

/// Writes `focus-none.ppm` / `focus-tab.ppm` / `focus-tab2.ppm` into
/// `$VIEWW_SHOT_DIR`: three buttons, before Tab, after Tab, after Tab twice.
#[test]
fn writes_screenshots_when_asked() {
    let Ok(dir) = std::env::var("VIEWW_SHOT_DIR") else { return };

    #[derive(Debug)]
    struct Row;
    impl Widget for Row {
        fn debug_name(&self) -> &'static str { "Row" }
        fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
        fn build(&self, _ctx: &BuildContext) -> WidgetNode {
            let b = |c: Color| -> WidgetNode {
                Pressable::new(move |p| {
                    ColoredBox::new(c.with_alpha((255.0 - 80.0 * p) as u8))
                        .child(SizedBox::width(96.0).child(SizedBox::height(44.0))).into()
                }).on_tap(|| {}).into()
            };
            ColoredBox::new(Color::rgb(20, 22, 26)).child(Center::new().child(
                Flex::row().spacing(20.0).children(vec![
                    b(Color::rgb(28, 113, 216)), b(Color::rgb(60, 64, 72)), b(Color::rgb(60, 64, 72)),
                ]))).into()
        }
    }
    vieww_widget::widget_node_from!(Row);

    let size = Size::new(420.0, 120.0);
    let mut driver = FrameDriver::new(size);
    driver.set_root(Row);
    driver.draw_frame();
    let dump = |driver: &FrameDriver, name: &str| {
        let f = render(driver, size, Color::BLACK);
        let mut out = format!("P6\n{} {}\n255\n", size.width as u32, size.height as u32).into_bytes();
        for y in 0..size.height as u32 {
            for x in 0..size.width as u32 {
                let (r, g, b, _) = f.at(x, y);
                out.extend([r, g, b]);
            }
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    };
    dump(&driver, "focus-none");
    for n in 1..=2 {
        driver.handle_key(&KeyEvent::down(LogicalKey::Named(NamedKey::Tab), Duration::ZERO));
        driver.draw_frame();
        dump(&driver, if n == 1 { "focus-tab" } else { "focus-tab2" });
    }
}
