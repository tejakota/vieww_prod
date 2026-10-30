//! The palette, lifted from the reference SVGs verbatim.
//!
//! The theme is the human neuron seen through a microscope in the dark: a deep
//! membrane ground, cyan synapse light, violet axons, mint dendrites, and one
//! warm amber for the soma — the single warm note in a cold field, which is
//! what makes it read as *alive* rather than as a blue interface.
//!
//! Every colour here is the reference's own hex, at the alpha the reference
//! composes it at — this file is the single place those numbers live, so the
//! screens can quote roles and stay exact.

use vieww_foundation::{Color, Gradient};
use vieww_widget::{ColorScheme, ThemeData};

/// The ground the whole app floats on — `#060a13`.
pub const GROUND: Color = Color::rgb(6, 10, 19);
/// The gradient's top — `#111a2d`.
pub const GROUND_TOP: Color = Color::rgb(17, 26, 45);
/// The gradient's middle — `#080e1a`.
pub const GROUND_MID: Color = Color::rgb(8, 14, 26);
/// A raised card — `#121b2d`, the numbered sections.
pub const CARD: Color = Color::rgb(18, 27, 45);
/// The identity hero's card — `#141d30`.
pub const CARD_HERO: Color = Color::rgb(20, 29, 48);
/// Hairline edges and dividers — white at 7.5%.
pub const EDGE: Color = Color::rgba(255, 255, 255, 19);
/// Primary ink — `#eff2fa`, the reference's headline white.
pub const INK: Color = Color::rgb(239, 242, 250);
/// The greeting's ink — `#f2f3fa`.
pub const INK_GREET: Color = Color::rgb(242, 243, 250);
/// Secondary text — `#8d9bb1`.
pub const MUTED: Color = Color::rgb(141, 155, 177);
/// Faint section labels — `#8392aa`.
pub const FAINT: Color = Color::rgb(131, 146, 170);

/// Synapse light — `#5eead4`, the atmosphere's mint.
pub const SYNAPSE: Color = Color::rgb(94, 234, 212);
/// Axon violet — `#a78bfa`, the persona's own colour.
pub const AXON: Color = Color::rgb(167, 139, 250);
/// Dendrite mint — `#52d9c6`, the slider's fill.
pub const MEMBRANE: Color = Color::rgb(82, 217, 198);
/// Soma amber — the warm centre; training and memory.
pub const SOMA: Color = Color::rgb(245, 196, 110);

/// The alive chip's stroke — `#66e1d0` at 28%.
pub const CHIP_EDGE: Color = Color::rgb(102, 225, 208);
/// The alive dot — `#65e6d4`.
pub const CHIP_DOT: Color = Color::rgb(101, 230, 212);
/// The life glyph and its ring — `#70dfd0`.
pub const LIFE_TINT: Color = Color::rgb(112, 223, 208);
/// The slider's mint, lit — `#6ce6d5`.
pub const SLIDER_MINT: Color = Color::rgb(108, 230, 213);
/// The toggle's on-fill — `#28645c`.
pub const PILL_ON: Color = Color::rgb(40, 100, 92);
/// The toggle's on-stroke — `#5fe2d0` at 45%.
pub const PILL_ON_EDGE: Color = Color::rgb(95, 226, 208);
/// The toggle's knob when on — `#d8fff7`.
pub const PILL_KNOB_ON: Color = Color::rgb(216, 255, 247);
/// The toggle's off-fill — `#2b3649`.
pub const PILL_OFF: Color = Color::rgb(43, 54, 73);
/// The toggle's off-stroke — `#56657c` at 50%.
pub const PILL_OFF_EDGE: Color = Color::rgb(86, 101, 124);
/// The toggle's knob when off — `#9aa8bc`.
pub const PILL_KNOB_OFF: Color = Color::rgb(154, 168, 188);

/// The window's background wash, behind everything — the reference `bg`
/// gradient, stops and positions verbatim.
pub fn ground_gradient() -> Gradient {
    Gradient::vertical().with_stops(&[
        (0.0, GROUND_TOP),
        (0.48, GROUND_MID),
        (1.0, GROUND),
    ])
}

/// A card's surface for the ask sheet: faintly lighter at the top, as if lit
/// from above.
pub fn card_gradient() -> Gradient {
    Gradient::vertical().with_stops(&[(0.0, Color::rgb(20, 29, 48)), (1.0, CARD)])
}

/// The action button's fill — the reference `action` gradient, corner to
/// corner: `#a28afa` into `#8065e0` into `#5b45b8`.
pub fn action_gradient() -> Gradient {
    Gradient::linear(
        vieww_foundation::Offset::new(0.0, 0.0),
        vieww_foundation::Offset::new(1.0, 1.0),
    )
    .with_stops(&[
        (0.0, Color::rgb(162, 138, 250)),
        (0.55, Color::rgb(128, 101, 224)),
        (1.0, Color::rgb(91, 69, 184)),
    ])
}

/// The slider fill's mint gradient — `#c1fff5` into `#52d9c6`.
pub fn mint_gradient() -> Gradient {
    Gradient::horizontal().with_stops(&[
        (0.0, Color::rgb(193, 255, 245)),
        (1.0, MEMBRANE),
    ])
}

/// Linear interpolation between two colours, in sRGB. Good enough for the
/// hues being mixed here, which are neighbours on the wheel.
#[must_use]
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color::rgb(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b))
}

/// `U`'s own `ThemeData`: the dark scheme with the neuron palette mapped onto
/// its roles, so every control in the catalogue draws in-theme with nothing
/// per-control work.
#[must_use]
pub fn theme() -> ThemeData {
    let dark = ThemeData::dark();
    ThemeData {
        colors: ColorScheme {
            primary: SYNAPSE,
            on_primary: Color::rgb(5, 14, 13),
            surface: GROUND,
            on_surface: INK,
            surface_variant: CARD,
            on_surface_variant: MUTED,
            outline: EDGE,
            ..dark.colors
        },
        ..dark
    }
}
