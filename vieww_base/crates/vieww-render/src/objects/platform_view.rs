//! The render half of [`PlatformView`](vieww_widget::PlatformView): a leaf
//! that takes the biggest size it is allowed, paints nothing (the OS draws
//! the view above vieww's surface), and reports where layout put it.

use vieww_foundation::{Constraints, Offset, Rect, Size};
use vieww_widget::PlatformViewController;

use crate::{LayoutCtx, PaintCtx, RenderObject};

/// Where a native view sits in the layout. See the widget's docs.
#[derive(Debug)]
pub struct RenderPlatformView {
    controller: PlatformViewController,
}

impl RenderPlatformView {
    #[must_use]
    pub fn new(controller: PlatformViewController) -> Self {
        controller.mounted();
        Self { controller }
    }
}

impl Drop for RenderPlatformView {
    fn drop(&mut self) {
        // The last render object holding the controller destroys the view;
        // a rebuild's replacement was made before this one goes.
        self.controller.unmounted();
    }
}

impl RenderObject for RenderPlatformView {
    fn layout(&mut self, _ctx: &mut LayoutCtx<'_>, constraints: Constraints) -> Size {
        // A native view has no intrinsic size to offer; the space it is given
        // is the space it fills, and an unbounded axis falls back to zero
        // rather than infinity (a sized parent is how one gets a size).
        let biggest = constraints.biggest();
        let pick = |v: f32| if v.is_finite() { v } else { 0.0 };
        constraints.constrain(Size::new(pick(biggest.width), pick(biggest.height)))
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>) {}

    fn hit_test_self(&self, _point: Offset, _size: Size) -> bool {
        // The OS view gets its own input; vieww's pointer under it would be a
        // second consumer of the same click.
        false
    }

    fn wants_placement(&self) -> bool {
        true
    }

    fn placed(&self, bounds: Rect) {
        self.controller.place(bounds);
    }

    fn layout_differs(&self, _new: &dyn RenderObject) -> bool {
        false
    }

    fn debug_name(&self) -> &'static str {
        "RenderPlatformView"
    }
}
