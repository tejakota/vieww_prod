//! `KeyListener`: an autofocused listener hears a key when nothing else has
//! focus, and a focused control inside it keeps the keys it uses.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::{KeyEvent, LogicalKey, NamedKey, Size};
use vieww::prelude::*;
use vieww_widget::KeyListener;

fn down(k: NamedKey) -> KeyEvent {
    KeyEvent::down(LogicalKey::Named(k), Duration::ZERO)
}

#[test]
fn an_autofocused_listener_hears_a_key_with_nothing_else_focused() {
    let heard = Rc::new(Cell::new(0));
    let mut driver = FrameDriver::new(Size::new(200.0, 200.0));
    let h = heard.clone();
    driver.set_root(
        KeyListener::new(move |e| {
            if e.is_down() {
                h.set(h.get() + 1);
            }
            e.is_down()
        })
        .autofocus(true)
        .child(SizedBox::square(50.0)),
    );
    driver.draw_frame();
    assert!(driver.handle_key(&down(NamedKey::Escape)));
    assert_eq!(heard.get(), 1);
}

#[test]
fn without_autofocus_it_hears_nothing_until_something_inside_is_focused() {
    let heard = Rc::new(Cell::new(0));
    let mut driver = FrameDriver::new(Size::new(200.0, 200.0));
    let h = heard.clone();
    let tapped = Rc::new(Cell::new(0));
    let t = tapped.clone();
    driver.set_root(
        // Only Escape is the listener's: answering `false` for the rest is
        // what leaves Tab to move focus.
        KeyListener::new(move |e| {
            let mine = e.is_down() && e.key == LogicalKey::Named(NamedKey::Escape);
            if mine {
                h.set(h.get() + 1);
            }
            mine
        })
        .child(Center::new().child(
            Pressable::new(|_| SizedBox::square(40.0).into()).on_tap(move || t.set(t.get() + 1)),
        )),
    );
    driver.draw_frame();
    assert!(
        !driver.handle_key(&down(NamedKey::Escape)),
        "no focus, no listener"
    );
    assert_eq!(heard.get(), 0);
    // Tab until the button has focus (the listener is a tab stop too), and
    // Enter is the button's.
    for _ in 0..3 {
        driver.handle_key(&down(NamedKey::Tab));
        driver.draw_frame();
        driver.handle_key(&down(NamedKey::Enter));
        if tapped.get() > 0 {
            break;
        }
    }
    assert_eq!(
        tapped.get(),
        1,
        "Enter on the focused button is the button's"
    );
    // Escape is not the button's, so it bubbles to the listener.
    assert!(driver.handle_key(&down(NamedKey::Escape)));
    assert_eq!(heard.get(), 1);
}
