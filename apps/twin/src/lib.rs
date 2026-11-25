//! twin — a digital life: a pixel twin that lives in a shared world, with
//! morse code in its pixels and an encrypted mind (see `CONCEPT.md`).
//!
//! This crate is the client's screens (TW-01): eight of them, built from
//! `design/screens-v2`, rendering headless through the native rasteriser.
//! Networking, the encrypted store and the phone hosts are separate work.
//!
//! # The tier-low rules, from day one
//!
//! * **No blur, no glass, no shadows** — every surface is a flat fill and a
//!   1 px edge, so the low tier draws exactly what the high tier does.
//! * **One 3 px pixel grid** — sprites and the world are filled rectangles
//!   on multiples of [`PX`]; nothing is scaled, so nothing is resampled.
//! * **Ambient motion is gated** — the idle sway and the morse blink read
//!   [`Motion`]: `Still` when the person asks for reduced motion or the tier
//!   is low, and the frame is then the same picture, held.

use std::rc::Rc;

use vieww::element::{Runtime, Signal};
use vieww::foundation::{Color, FontWeight, Size, TextStyle};
use vieww::prelude::*;

mod pixel;
mod screens;

pub use pixel::{morse_runs as pixel_runs, Avatar, Morse};
pub use screens::{screen_view, Screen};

/// The logical size the screens are drawn for (a 390×844 phone).
pub const SURFACE: Size = Size {
    width: 390.0,
    height: 844.0,
};

/// The pixel grid, in logical pixels.
pub const PX: f32 = 3.0;

// ---- tokens (DESIGN_SPEC: twin skin) ---------------------------------------

pub const GROUND: Color = Color::rgb(0x0B, 0x0D, 0x0B);
pub const PANEL: Color = Color::rgb(0x11, 0x16, 0x14);
pub const EDGE: Color = Color::rgb(0x23, 0x2B, 0x27);
/// Phosphor mint: the twin, and every action. 15.6:1 on [`GROUND`].
pub const MINT: Color = Color::rgb(0x7D, 0xFF, 0xA8);
/// Amber: morse, and only morse.
pub const AMBER: Color = Color::rgb(0xF6, 0xB9, 0x5A);
pub const BLUE: Color = Color::rgb(0x7A, 0xA2, 0xF7);
pub const INK: Color = Color::rgb(0xE8, 0xEE, 0xE8);
pub const MUTED: Color = Color::rgb(0x8C, 0x97, 0x8C);

/// Ambient motion, decided once from the tier and the accessibility setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// The twin sways and the morse strip blinks.
    Ambient,
    /// A held frame: low tier, or reduced motion asked for.
    Still,
}

impl Motion {
    /// `Still` when the tier is low or reduced motion is on.
    #[must_use]
    pub const fn gate(low_tier: bool, reduce_motion: bool) -> Self {
        if low_tier || reduce_motion {
            Self::Still
        } else {
            Self::Ambient
        }
    }
}

/// twin's skin for the shared kit (UX-01): the night plate and phosphor mint.
#[must_use]
pub fn skin() -> vieww_ui_kit::Skin {
    use vieww::foundation::FontWeight as W;
    vieww_ui_kit::Skin {
        ground: GROUND,
        panel: PANEL,
        edge: EDGE,
        accent: MINT,
        accent_tint: Color::rgb(0x12, 0x2A, 0x1E),
        ink: INK,
        muted: MUTED,
        danger: (Color::rgb(0x3A, 0x14, 0x18), Color::rgb(0xFF, 0x8A, 0x94), Color::rgb(0x6A, 0x24, 0x2C)),
        radius: 4.0,
        chip_radius: 4.0,
        badge_radius: 3.0,
        button_pad: (14.0, 12.0),
        button_text: 14.0,
        card_pad: (14.0, 12.0),
        chip_pad: (12.0, 8.0),
        chip_height: 40.0,
        chip_text: (12.0, 0.2),
        badge_pad: (6.0, 3.0),
        badge_text: (9.0, 0.8),
        press_dim: 0.25,
        toast_fill: PANEL,
        toast_ink: INK,
        toast_pad: (16.0, 10.0),
        toast_text: (13.0, W::Medium),
        toast_max_width: 310.0,
        toast_shadow: None,
    }
}

// ---- state ---------------------------------------------------------------

/// What the screens read and the tab bar changes.
#[derive(Debug)]
pub struct TwinState {
    pub screen: Signal<Screen>,
    pub motion: Motion,
}

pub fn build_state(rt: &Runtime, motion: Motion) -> Rc<TwinState> {
    Rc::new(TwinState {
        screen: rt.signal(Screen::Home),
        motion,
    })
}

// ---- shared text helpers --------------------------------------------------

#[must_use]
pub fn style(size: f32, weight: FontWeight, color: Color, spacing: f32) -> TextStyle {
    TextStyle::new(size)
        .weight(weight)
        .color(color)
        .letter_spacing(spacing)
}

/// The Geist faces, bundled; the pixel face (Silkscreen) is not yet, so
/// `pixel_label` approximates it with tracked capitals.
#[must_use]
pub fn fonts() -> vieww::text::FontStore {
    const REGULAR: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Regular.ttf");
    const MEDIUM: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Bold.ttf");
    vieww::text::FontStore::with_application_faces(
        [REGULAR.to_vec(), MEDIUM.to_vec(), BOLD.to_vec()],
        Some("Geist"),
        Some("Geist"),
    )
}

#[must_use]
pub fn theme() -> ThemeData {
    ThemeData::dark()
}

/// The root widget: the current screen inside the theme.
#[must_use]
pub fn root(state: &Rc<TwinState>) -> WidgetNode {
    Theme::new(theme()).child(screens::shell(state)).into()
}

/// Mount the app on a driver.
pub fn mount(driver: &mut vieww::FrameDriver, motion: Motion) -> Rc<TwinState> {
    let state = build_state(driver.elements().runtime(), motion);
    driver.set_fonts(fonts());
    driver.set_root(root(&state));
    state
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run() {
    use vieww_platform_winit::App;
    let app = App::new().title("twin").size(SURFACE).theme(theme());
    match app.run(|driver| {
        mount(driver, Motion::Ambient);
    }) {
        Ok(report) => println!("{report}"),
        Err(error) => eprintln!("twin failed to start: {error}"),
    }
}
