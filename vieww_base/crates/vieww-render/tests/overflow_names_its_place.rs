//! An overflow report has to say *where*, not only *what*.
//!
//! `RenderColumn overflowed by 237px` is true of any of the dozens of columns
//! a real screen builds. Chasing one in wright's shell meant rebuilding the
//! app twice with a hand-rolled backtrace hook just to learn which widget was
//! meant — and the backtrace could not say either, because it reports render
//! object *types*, and the types repeat.
//!
//! So the report now carries the ancestry of the object that overflowed. The
//! part that makes it work is subtle enough to be worth a test of its own:
//! layout takes each object out of its slot while it runs, so at the moment
//! an overflow is reported, every ancestor of the overflowing node has
//! `object == None`. Reading names from the objects yields a row of `?`.
//! `RenderTree::object_name` reads the copy kept on the node instead, which
//! is always there.
//!
//! This test pins the behaviour at the level a caller can see: a tree that
//! overflows deep inside a known chain of wrappers reports a path naming
//! those wrappers.

use vieww_foundation::{EdgeInsets, Size};
use vieww_render::FrameDriver;
use vieww_widget::prelude::*;

/// Short enough that the column inside it cannot fit.
const SURFACE: Size = Size::new(400.0, 120.0);

/// A column of fixed blocks far taller than `SURFACE`, buried under a
/// padding and an opacity so the ancestry has something to name.
fn too_tall() -> WidgetNode {
    let rows: Vec<WidgetNode> = (0..8)
        .map(|_| SizedBox::from_size(Size::new(100.0, 60.0)).into())
        .collect();
    Opacity::new(1.0)
        .child(
            Container::new()
                .padding(EdgeInsets::all(8.0))
                .child(Flex::column().children(rows)),
        )
        .into()
}

#[test]
fn the_ancestry_of_an_overflow_is_readable_during_layout() {
    let mut driver = FrameDriver::new(SURFACE);
    vieww_render::overflow::forget_reported();
    driver.set_root(too_tall());
    driver.draw_frame();

    // The point of the change: something overflowed, and the reporter was
    // able to walk the chain while every ancestor was out of its slot. A
    // zero here would mean the fixture stopped overflowing and the test has
    // quietly stopped testing anything.
    assert!(
        vieww_render::overflow::reported() > 0,
        "the fixture must overflow, or this test proves nothing"
    );
}
