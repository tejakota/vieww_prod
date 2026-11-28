use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use vieww_foundation::{Color, Constraints, KeyEvent, Offset, Rect, Size};

use crate::{LayoutCtx, PaintCtx, RenderObject};

/// The render half of [`KeyListener`](vieww_widget::KeyListener): a focusable
/// object that offers the keys reaching it to a handler.
///
/// Keys bubble from the focused object outward, so this hears whatever its
/// focused descendants left unhandled, and — being focusable, and taking the
/// keyboard on mount when asked — the keys that arrive while it is the focus
/// itself. It adds no geometry: its size is its child's.
pub struct RenderKeyListener {
    on_key: Rc<dyn Fn(&KeyEvent) -> bool>,
    autofocus: bool,
    focus_ring: Option<Color>,
    ring_visible: Cell<bool>,
}

impl RenderKeyListener {
    #[must_use]
    pub fn new(on_key: Rc<dyn Fn(&KeyEvent) -> bool>, autofocus: bool) -> Self {
        Self {
            on_key,
            autofocus,
            focus_ring: None,
            ring_visible: Cell::new(false),
        }
    }

    /// Draw a 2 px ring in `color` around this while the keyboard is on it,
    /// as [`RenderGestureDetector`](crate::RenderGestureDetector) does.
    #[must_use]
    pub const fn focus_ring(mut self, color: Option<Color>) -> Self {
        self.focus_ring = color;
        self
    }
}

impl fmt::Debug for RenderKeyListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderKeyListener")
            .field("autofocus", &self.autofocus)
            .finish_non_exhaustive()
    }
}

impl RenderObject for RenderKeyListener {
    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, constraints: Constraints) -> Size {
        let Some(&child) = ctx.children().first() else {
            return constraints.smallest();
        };
        let size = ctx.layout_child(child, constraints);
        ctx.place_child(child, Offset::ZERO);
        size
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn wants_autofocus(&self) -> bool {
        self.autofocus
    }

    fn handle_key(&self, event: &KeyEvent) -> bool {
        (self.on_key)(event)
    }

    fn set_focus_visible(&self, visible: bool) {
        self.ring_visible.set(visible);
    }

    fn paint_children_done(&self, ctx: &mut PaintCtx<'_>) {
        let (Some(color), true) = (self.focus_ring, self.ring_visible.get()) else {
            return;
        };
        let b = ctx.bounds();
        ctx.canvas().stroke_rrect(
            Rect::new(b.left - 2.0, b.top - 2.0, b.right + 2.0, b.bottom + 2.0),
            8.0,
            vieww_paint::Stroke::new(2.0),
            vieww_paint::Paint::solid(color),
        );
    }

    crate::intrinsics::pass_through_intrinsic!();

    crate::baseline::pass_through_baseline!();

    fn layout_differs(&self, _new: &dyn RenderObject) -> bool {
        // A new handler or autofocus flag moves no geometry.
        false
    }

    fn debug_name(&self) -> &'static str {
        "RenderKeyListener"
    }
}
