//! The stage: the reference's own coordinate system, on any screen.
//!
//! Both reference screens are drawn on a 420 × 920 canvas, and the app is an
//! exact clone of them — every block is positioned at the reference's own
//! coordinates inside a box of exactly that size. Real screens are not
//! 420 × 920, so the stage puts the design on whatever it is given:
//!
//! * the **background** — the vertical ground wash — is stretched to fill the
//!   whole window, because a vertical gradient stretched is the same gradient;
//! * the **design** — everything positioned, every spacing — is shrunk
//!   *uniformly* by [`Fitted`] until it fits the safe area, and centred.
//!
//! Uniform, because a circle scaled non-uniformly is an ellipse and the
//! companion's eye is a circle. Centred, because the leftover band is small on
//! every phone this app targets (a 360 × 780 dp screen leaves a 4 dp band; a
//! 411 × 891 one leaves 5) and it is painted in the ground wash, where it
//! reads as the membrane the screen happens to have more of than the design
//! needed. A window exactly the design's size — the desktop harness, the
//! screenshot tool — is the identity: every coordinate lands where the
//! reference put it.
//!
//! Nothing is ever cut off and nothing overlaps differently than the reference
//! drew it — a shorter phone sees the whole design smaller, not a crop of it.
//! Hit testing follows the drawing through the scale, so taps land where the
//! eye appears.
//!
//! # The safe area
//!
//! The stage is inset by the platform's safe area on the **vertical** edges
//! only, so on a phone the design's own top margin — the reference draws its
//! top bar at y = 31 — sits below the status bar rather than under it, and
//! the home indicator gets the same respect at the bottom. The background
//! wash still runs full-bleed behind both.
//!
//! The horizontal edges are *not* inset, and that is the fix for the screen
//! that read as "offset": a phone whose platform reports a side inset — a
//! display cutout on the left, say — pushed the whole centred design right by
//! that amount, and the settings cards, drawn at the reference's own x = 24,
//! showed the extra space as a lopsided gutter. The app is portrait-locked,
//! and in portrait every notch and cutout lives inside the status-bar band
//! the top inset already clears, so a horizontal inset can only shift the
//! design — never save it from anything.

use vieww::prelude::*;
use vieww_foundation::Size;

use crate::palette;

/// The reference canvas: the coordinate system every screen is laid out in.
pub const DESIGN: Size = Size::new(420.0, 920.0);

/// One screen: the full-bleed wash behind, the scaled design in front.
///
/// `design` is the screen's whole content, built inside a
/// [`SizedBox::from_size`](SizedBox::from_size) of [`DESIGN`] — the stage does
/// the sizing itself, so callers only hand it the tree.
#[must_use]
pub fn screen(design: impl Into<WidgetNode>) -> WidgetNode {
    Stack::new()
        .fit(StackFit::Expand)
        .push(
            Positioned::fill().child(
                Container::new()
                    .gradient(palette::ground_gradient())
                    .child(SizedBox::expand()),
            ),
        )
        .push(
            Positioned::fill().child(
                SafeArea::new().horizontal(false).child(
                    Align::new(Alignment::CENTER)
                        .child(Fitted::new().child(SizedBox::from_size(DESIGN).child(design))),
                ),
            ),
        )
        .into()
}
