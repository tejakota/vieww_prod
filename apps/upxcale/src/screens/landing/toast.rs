//! The line at the bottom of the screen that says what a background save is
//! doing, and how it ended.

use vieww::prelude::*;

use crate::state::AppState;
use crate::theme;

/// The toast layer: nothing when there is no message, otherwise a pill above
/// the bottom edge. Reading `toast` here subscribes this subtree to it.
#[must_use]
pub fn layer(state: &AppState) -> WidgetNode {
    let Some(message) = state.toast.get() else {
        return SizedBox::shrink().into();
    };
    Positioned::new()
        .left(16.0)
        .right(16.0)
        .bottom(96.0)
        .child(
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::Center)
                .children(children![vieww_ui_kit::toast(&theme::skin(), &message)]),
        )
        .into()
}
