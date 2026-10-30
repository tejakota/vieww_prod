//! The palette: one dark ground, three signal colours, and the roles they
//! land in.
//!
//! The theme is the human neuron seen through a microscope in the dark: a deep
//! membrane ground, cyan synapse light, violet axons, mint dendrites, and one
//! warm amber for the soma — the single warm note in a cold field, which is
//! what makes it read as *alive* rather than as a blue interface.

use vieww_foundation::{Color, Gradient};
use vieww_widget::{ColorScheme, ThemeData};

/// The ground the whole app floats on.
pub const GROUND: Color = Color::rgb(7, 10, 18);
/// One step up from the ground — cards and wells sit on this.
pub const GROUND_2: Color = Color::rgb(11, 16, 30);
/// A raised card, lighter again.
pub const CARD: Color = Color::rgb(16, 22, 40);
/// Hairline edges and dividers.
pub const EDGE: Color = Color::rgba(255, 255, 255, 18);
/// Primary ink.
pub const INK: Color = Color::rgb(232, 238, 247);
/// Secondary text.
pub const MUTED: Color = Color::rgb(140, 158, 186);
/// Faint section labels.
pub const FAINT: Color = Color::rgb(96, 112, 140);

/// Synapse light — the accent. Buttons, sliders, active states.
pub const SYNAPSE: Color = Color::rgb(94, 234, 212);
/// Axon violet — the persona's own colour.
pub const AXON: Color = Color::rgb(167, 139, 250);
/// Dendrite mint — success and confirmation.
pub const MEMBRANE: Color = Color::rgb(110, 231, 183);
/// Soma amber — the warm centre; training and memory.
pub const SOMA: Color = Color::rgb(245, 196, 110);

/// The window's background wash, behind everything.
pub fn ground_gradient() -> Gradient {
    Gradient::vertical().with_stops(&[
        (0.0, Color::rgb(13, 19, 36)),
        (0.55, GROUND),
        (1.0, Color::rgb(4, 6, 12)),
    ])
}

/// A card's surface: faintly lighter at the top, as if lit from above.
pub fn card_gradient() -> Gradient {
    Gradient::vertical().with_stops(&[(0.0, Color::rgb(20, 27, 48)), (1.0, CARD)])
}

/// The one full-width action — the button that begins the daily rhythm, in
/// the persona's own colour.
pub fn action_gradient() -> Gradient {
    Gradient::vertical().with_stops(&[
        (0.0, Color::rgb(182, 152, 252)),
        (1.0, Color::rgb(148, 116, 236)),
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
