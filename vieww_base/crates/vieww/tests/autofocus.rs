//! `TextField::autofocus`: a field that takes the keyboard as it appears.
//!
//! The command palette's case (ST-18) in framework terms: an overlay opened
//! from a shortcut has to be typed into with no click, must not keep taking
//! the keyboard back on every rebuild after, and — once it closes — leaves
//! the keyboard where it found it.

use std::rc::Rc;
use std::time::Duration;

use vieww::prelude::*;
use vieww::{BuildContext, Widget, WidgetKind, WidgetNode};
use vieww_element::Signal;
use vieww_foundation::{KeyEvent, Size, TextEditingValue};
use vieww_render::FrameDriver;
use vieww_widget::TextField;

#[derive(Debug, Clone)]
struct Screen {
    base: Signal<String>,
    overlay: Signal<String>,
    open: Signal<bool>,
    rebuilds: Signal<u32>,
}

impl Widget for Screen {
    fn debug_name(&self) -> &'static str {
        "Screen"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        // Read, so bumping it rebuilds the whole screen.
        let _ = self.rebuilds.get();
        let field = |text: &Signal<String>, autofocus: bool| -> WidgetNode {
            let held = text.clone();
            TextField::text(text.get())
                .single_line()
                .autofocus(autofocus)
                .on_changed(Rc::new(move |v: TextEditingValue| held.set(v.text)))
                .into()
        };
        let mut rows = vec![field(&self.base, false)];
        if self.open.get() {
            rows.push(field(&self.overlay, true));
        }
        Flex::column().children(rows).into()
    }
}
vieww_widget::widget_node_from!(Screen);

fn mounted() -> (FrameDriver, Screen) {
    let mut driver = FrameDriver::new(Size::new(400.0, 200.0));
    let runtime = driver.elements().runtime().clone();
    let screen = Screen {
        base: runtime.signal(String::new()),
        overlay: runtime.signal(String::new()),
        open: runtime.signal(false),
        rebuilds: runtime.signal(0),
    };
    driver.set_root(screen.clone());
    driver.draw_frame();
    // The base field holds the keyboard, as after a click.
    assert!(driver.focus_next(true));
    (driver, screen)
}

fn type_char(driver: &mut FrameDriver, c: &str) {
    driver.handle_key(&KeyEvent::character(c, Duration::ZERO));
    driver.draw_frame();
}

#[test]
fn a_field_that_asks_takes_the_keyboard_when_it_appears() {
    let (mut driver, screen) = mounted();
    type_char(&mut driver, "a");
    assert_eq!(screen.base.get(), "a", "the base field holds the keyboard");

    screen.open.set(true);
    driver.draw_frame();
    type_char(&mut driver, "b");
    assert_eq!(
        screen.overlay.get(),
        "b",
        "the new field did not take the keyboard"
    );
    assert_eq!(screen.base.get(), "a");
}

#[test]
fn it_takes_the_keyboard_once_not_on_every_rebuild() {
    let (mut driver, screen) = mounted();
    screen.open.set(true);
    driver.draw_frame();
    // Tab back to the base field, then rebuild: the overlay must not steal it.
    assert!(driver.focus_next(false));
    screen.rebuilds.set(1);
    driver.draw_frame();
    type_char(&mut driver, "c");
    assert_eq!(screen.base.get(), "c", "a rebuild took the keyboard back");
    assert_eq!(screen.overlay.get(), "");
}

#[test]
fn closing_it_gives_the_keyboard_back() {
    let (mut driver, screen) = mounted();
    screen.open.set(true);
    driver.draw_frame();
    screen.open.set(false);
    driver.draw_frame();
    type_char(&mut driver, "d");
    assert_eq!(
        screen.base.get(),
        "d",
        "the keyboard did not return to the base field"
    );

    // A second opening is a new field, and asks again.
    screen.open.set(true);
    driver.draw_frame();
    type_char(&mut driver, "e");
    assert_eq!(
        screen.overlay.get(),
        "e",
        "a reopened field did not take the keyboard"
    );
}
