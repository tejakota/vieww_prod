use std::fmt;
use std::rc::Rc;

use vieww_foundation::{Color, Key, KeyEvent};

use crate::{widget_node_from, Widget, WidgetKind, WidgetNode};

/// The handler a [`KeyListener`] calls: `true` when it used the key.
pub type KeyHandler = Rc<dyn Fn(&KeyEvent) -> bool>;

/// Hears the keys that reach its subtree and nothing inside wanted.
///
/// ```
/// use vieww_widget::prelude::*;
/// use vieww_widget::KeyListener;
///
/// // Any key dismisses an intro overlay, the way a tap does.
/// let intro = KeyListener::new(|e| e.is_down()).autofocus(true).child(Text::new("Welcome"));
/// ```
///
/// Keys bubble outward from focus, so a listener around a screen sees what
/// its focused descendant did not handle (a text field keeps ordinary
/// typing). It is focusable itself, which is what lets it hear keys when
/// nothing inside is: with [`autofocus`](Self::autofocus) it takes the
/// keyboard once, when it is mounted, and the driver hands focus back to
/// whatever had it when the listener goes.
///
/// Answer `false` for keys it does not use: a handler that claims Tab stops
/// Tab from moving focus (the driver only traverses on an unhandled Tab).
///
/// Layout, painting and hit testing are its child's: it adds no geometry.
#[derive(Clone)]
pub struct KeyListener {
    on_key: KeyHandler,
    autofocus: bool,
    focus_ring: Option<Color>,
    child: Option<WidgetNode>,
    key: Option<Key>,
}

impl KeyListener {
    /// Listen with `on_key`, which answers whether it used the key.
    #[must_use]
    pub fn new(on_key: impl Fn(&KeyEvent) -> bool + 'static) -> Self {
        Self {
            on_key: Rc::new(on_key),
            autofocus: false,
            focus_ring: None,
            child: None,
            key: None,
        }
    }

    /// Take the keyboard when mounted (once), so keys arrive here even when
    /// nothing else has focus — an overlay that any key dismisses.
    #[must_use]
    pub const fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Draw a 2 px keyboard focus ring in `color` while this has focus from
    /// the keyboard — for a listener that *is* the control (a divider, a
    /// canvas you steer with arrows). A listener around a screen wants none.
    #[must_use]
    pub const fn focus_ring(mut self, color: Option<Color>) -> Self {
        self.focus_ring = color;
        self
    }

    #[must_use]
    pub fn child(mut self, child: impl Into<WidgetNode>) -> Self {
        self.child = Some(child.into());
        self
    }

    #[must_use]
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The handler — read by the render layer.
    #[must_use]
    pub fn handler(&self) -> KeyHandler {
        Rc::clone(&self.on_key)
    }

    /// The ring colour — read by the render layer.
    #[must_use]
    pub const fn focus_ring_color(&self) -> Option<Color> {
        self.focus_ring
    }

    /// Whether it asked for focus on mount — read by the render layer.
    #[must_use]
    pub const fn wants_autofocus(&self) -> bool {
        self.autofocus
    }
}

impl fmt::Debug for KeyListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyListener")
            .field("autofocus", &self.autofocus)
            .finish_non_exhaustive()
    }
}

impl Widget for KeyListener {
    fn debug_name(&self) -> &'static str {
        "KeyListener"
    }

    fn kind(&self) -> WidgetKind<'_> {
        match &self.child {
            Some(child) => WidgetKind::RenderSingleChild(child),
            None => WidgetKind::RenderLeaf,
        }
    }

    fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    fn debug_properties(&self) -> Vec<(&'static str, String)> {
        vec![("autofocus", self.autofocus.to_string())]
    }
}

widget_node_from!(KeyListener);
