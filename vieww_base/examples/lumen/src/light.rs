//! Light — the screen is a lit scene.
//!
//! One [`Light`] for the whole screen (where it comes from, how high it
//! stands, how large it is, its colour and strength) and a [`Material`] per
//! surface (finish, metal or not, how much light passes through, how high it
//! sits). Every surface's colour, highlight, rim and shadow is *computed*
//! from the two, in linear light, so nothing is painted by hand and nothing
//! can disagree: move the light and the whole interface answers together.
//!
//! A prototype for looking at, not an API: the shading is a 2D
//! approximation of the physical model `vieww-3d` already implements
//! (Lambert diffuse, a roughness-shaped specular lobe, Fresnel-ish rims).

use vieww::prelude::*;
use vieww_foundation::{Border, BoxDecoration, Color, FontWeight, Gradient, Offset, Shadow};

use super::{text, HAIR};

// ---- light ---------------------------------------------------------------

/// The one light the screen is lit by.
#[derive(Clone, Copy)]
pub struct Light {
    /// Where the light is, projected onto the screen: a unit vector pointing
    /// *from the surface toward the light* (screen y grows downward).
    pub from: Offset,
    /// How high above the screen plane, in degrees: low light rakes across
    /// and throws long shadows, high light falls straight on.
    pub elevation: f32,
    /// The light's apparent size, 0 (a point: hard shadows) to 1 (an overcast
    /// sky: shadows dissolve).
    pub size: f32,
    /// Its colour, linear RGB.
    pub color: [f32; 3],
    /// Its strength, 0..1.
    pub intensity: f32,
    /// The sky's fill — light arriving from everywhere at once, linear RGB.
    pub ambient: [f32; 3],
    /// How bright the screen is overall, kept apart from the light's shape:
    /// the theme (and later the room's ambient sensor and the battery) sets
    /// this, so a light theme stays light at dusk and noon costs no more
    /// than morning.
    pub exposure: f32,
    /// What this light is called, for the caption.
    pub name: &'static str,
}

impl Light {
    /// The sun at time `t` through the day: 0 dawn, 0.5 noon, 1 dusk; past 1,
    /// night under one warm lamp.
    pub fn at(t: f32) -> Self {
        // The light stays up and to the left all day — a fixed, familiar
        // key light. Time only nudges it: a little lower and warmer at the
        // ends of the day, higher and more neutral at midday.
        if t > 1.0 {
            return Self {
                from: norm(Offset::new(-0.5, -0.86)),
                elevation: 34.0,
                size: 0.25,
                color: [1.0, 0.82, 0.62],
                intensity: 0.7,
                ambient: [0.05, 0.05, 0.07],
                exposure: 0.82,
                name: "Night · warm lamp",
            };
        }
        let arc = (t * std::f32::consts::PI).sin();
        let warm = 1.0 - arc;
        let lerp = |a: f32, b: f32, u: f32| a + (b - a) * u;
        Self {
            from: norm(Offset::new(lerp(-0.85, -0.15, t), lerp(-0.55, -0.99, arc))),
            elevation: 30.0 + 38.0 * arc,
            size: 0.3,
            color: [1.0, 0.97 - 0.12 * warm, 0.94 - 0.24 * warm],
            intensity: 0.75 + 0.25 * arc,
            ambient: [0.06, 0.065, 0.08],
            exposure: 0.95,
            name: if t < 0.2 {
                "Morning"
            } else if t < 0.8 {
                "Midday"
            } else {
                "Evening"
            },
        }
    }

    fn tan_elevation(self) -> f32 {
        self.elevation.to_radians().tan().max(0.12)
    }
}

fn norm(v: Offset) -> Offset {
    let l = (v.dx * v.dx + v.dy * v.dy).sqrt().max(1e-6);
    Offset::new(v.dx / l, v.dy / l)
}

// ---- material ------------------------------------------------------------

/// What a surface is made of.
#[derive(Clone, Copy)]
pub struct Material {
    /// Base colour, linear RGB.
    pub base: [f32; 3],
    /// 0 a mirror-smooth gloss, 1 a chalky matte.
    pub roughness: f32,
    /// A metal reflects in its own colour and has almost no diffuse light.
    pub metal: bool,
    /// How much light passes through: 0 opaque, 1 clear glass.
    pub transmission: f32,
    /// How high the surface sits above the ground.
    pub z: f32,
}

pub const MATTE: Material = Material { base: [0.11, 0.12, 0.16], roughness: 1.0, metal: false, transmission: 0.0, z: 1.0 };
pub const SATIN: Material = Material { base: [0.13, 0.14, 0.19], roughness: 0.55, metal: false, transmission: 0.0, z: 2.0 };
pub const GLOSS: Material = Material { base: [0.18, 0.10, 0.72], roughness: 0.3, metal: false, transmission: 0.0, z: 3.0 };
pub const METAL: Material = Material { base: [0.72, 0.62, 0.42], roughness: 0.45, metal: true, transmission: 0.0, z: 2.0 };
pub const GLASS: Material = Material { base: [0.55, 0.70, 0.95], roughness: 0.35, metal: false, transmission: 0.8, z: 3.0 };

// ---- colour math, in linear light -----------------------------------------

fn encode(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn rgb(c: [f32; 3]) -> Color {
    Color::rgb(encode(c[0]), encode(c[1]), encode(c[2]))
}

fn rgba(c: [f32; 3], a: f32) -> Color {
    rgb(c).with_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

// ---- shading -------------------------------------------------------------

/// The light falling on a surface that faces the viewer, in linear RGB.
fn irradiance(light: Light) -> [f32; 3] {
    let lambert = light.elevation.to_radians().sin() * 0.85 + 0.15;
    let raw = add(light.ambient, scale(light.color, light.intensity * lambert));
    // Exposure: keep the light's colour, set its brightness to the theme's.
    let lum = (0.2126 * raw[0] + 0.7152 * raw[1] + 0.0722 * raw[2]).max(1e-4);
    scale(raw, light.exposure / lum)
}

/// A surface's lit colour, and the colour of its highlight.
fn shade(m: Material, light: Light) -> ([f32; 3], [f32; 3]) {
    // Height lifts a surface toward the light: each level up catches a
    // little more of it, and anything below the ground sinks into shade.
    let e = scale(irradiance(light), (1.0 + m.z * 0.09).clamp(0.55, 1.8));
    let diffuse = if m.metal { scale(mul(m.base, e), 0.18) } else { mul(m.base, e) };
    // The specular lobe: a smooth surface concentrates the light into a
    // bright, small highlight; a rough one spreads it thin.
    let strength = light.intensity * (1.0 - m.roughness).powf(1.5) * 0.55;
    let tint = if m.metal { m.base } else { [1.0, 1.0, 1.0] };
    let spec = scale(mul(light.color, tint), strength);
    (diffuse, add(diffuse, spec))
}

/// The shadow a surface casts: away from the light, longer the lower the
/// light, softer the larger it is and the higher the surface sits.
fn shadow(m: Material, light: Light) -> Shadow {
    // Low layers get short, tight shadows — a list of cards must not spill
    // into each other; reach grows with height, faster above z 4.
    let reach = (m.z * 2.2 / light.tan_elevation()).min(if m.z <= 4.0 { 2.0 + m.z * 1.2 } else { m.z * 9.0 });
    let blur = 2.0 + m.z * (2.0 + light.size * 9.0) + reach * 0.35;
    let darkness = (0.6 + 0.35 * light.intensity) * (1.0 - m.transmission * 0.5);
    // A shadow is where only the sky reaches: tinted by the ambient fill.
    let tint = scale(light.ambient, 0.15);
    Shadow::new(
        rgba(tint, darkness),
        Offset::new(-light.from.dx * reach, -light.from.dy * reach),
        blur,
    )
}

/// Where a surface touches what is under it, the light is blocked from
/// every side: a tight, dark shadow that hugs the edge. The gap between this
/// and the cast shadow is what tells the eye how high the surface stands.
fn contact(m: Material, light: Light) -> Shadow {
    let reach = (m.z * 0.5 / light.tan_elevation()).min(m.z * 1.2);
    Shadow::new(
        rgba([0.0, 0.0, 0.0], 0.8 * (1.0 - m.transmission * 0.4)),
        Offset::new(-light.from.dx * reach, -light.from.dy * reach + 0.5),
        1.0 + m.z * 0.6,
    )
}

/// The edge facing the light catches it: brighter on smooth surfaces and at
/// grazing light (a Fresnel-shaped rim).
fn rim(m: Material, light: Light) -> Shadow {
    let grazing = 1.0 - light.elevation.to_radians().sin();
    let a = (0.12 + 0.5 * (1.0 - m.roughness) * (0.4 + grazing)) * light.intensity;
    let edge = if m.metal { mul(light.color, m.base) } else { light.color };
    Shadow::inset(
        rgba(add(scale(edge, 0.9), [0.1, 0.1, 0.1]), a),
        Offset::new(-light.from.dx * HAIR * 3.0, -light.from.dy * HAIR * 3.0),
        0.0,
    )
}

/// The highlight sweeping across a surface, oriented by the light.
/// The sheen's three tones — shaded side, body, highlight — narrowed until
/// every text colour the surface carries keeps 4.5:1 against all three.
/// Readability outranks the light: the highlight gives way, never the text.
fn range(m: Material, light: Light, inks: &[[f32; 3]]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (lit, hot) = shade(m, light);
    let spec = [hot[0] - lit[0], hot[1] - lit[1], hot[2] - lit[2]];
    let (mut k, mut f) = (1.0_f32, 0.78_f32);
    for _ in 0..24 {
        let h = add(lit, scale(spec, k));
        let d = scale(lit, f);
        if inks.iter().all(|&t| contrast(t, h) >= 4.5 && contrast(t, d) >= 4.5) {
            return (d, lit, h);
        }
        k *= 0.75;
        f += (1.0 - f) * 0.3;
    }
    (lit, lit, lit)
}

fn sheen(m: Material, light: Light) -> Gradient {
    sheen_for(m, light, &[])
}

fn sheen_for(m: Material, light: Light, inks: &[[f32; 3]]) -> Gradient {
    let (far, lit, hot) = range(m, light, inks);
    // From the side facing the light to the side away from it.
    let start = Offset::new(0.5 - light.from.dx * 0.5, 0.5 - light.from.dy * 0.5);
    let end = Offset::new(0.5 + light.from.dx * 0.5, 0.5 + light.from.dy * 0.5);
    let width = 0.12 + 0.6 * m.roughness;
    Gradient::linear(start, end).with_dither().with_stops(&[
        (0.0, rgb(far)),
        ((1.0 - width).max(0.05), rgb(lit)),
        (1.0, rgb(hot)),
    ])
}

/// A surface of material `m` under `light`.
pub fn surface(m: Material, light: Light, radius: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    surface_for(m, light, radius, &[], child)
}

/// A surface that carries text in the colours `inks`.
pub fn surface_for(m: Material, light: Light, radius: f32, inks: &[[f32; 3]], child: impl Into<WidgetNode>) -> WidgetNode {
    let border = Border::new(Color::rgba(255, 255, 255, (10.0 + 30.0 * (1.0 - m.roughness)) as u8), HAIR);
    if m.transmission > 0.0 {
        // Glass: the real backdrop, blurred by how rough the glass is, tinted
        // by its colour, with the light's sheen and rim on its face.
        let (lit, hot) = shade(m, light);
        let start = Offset::new(0.5 + light.from.dx * 0.5, 0.5 + light.from.dy * 0.5);
        let end = Offset::new(0.5 - light.from.dx * 0.5, 0.5 - light.from.dy * 0.5);
        let face = Gradient::linear(start, end).with_dither().with_stops(&[
            (0.0, rgba(hot, 0.34 * light.intensity + 0.06)),
            (0.45, rgba(lit, 0.10)),
            (1.0, rgba(lit, 0.04)),
        ]);
        return Container::new()
            .decoration(BoxDecoration::new().radius(radius).shadow(shadow(m, light)))
            .child(
                Container::new().decoration(BoxDecoration::new().radius(radius).shadow(contact(m, light))).child(
                Clip::rounded(radius).child(
                    Filtered::blur(3.0 + 14.0 * m.roughness)
                        .with_backdrop()
                        .tint(rgb(m.base), 0.18)
                        .child(
                            Container::new()
                                .decoration(
                                    BoxDecoration::new()
                                        .radius(radius)
                                        .gradient(face)
                                        .border(border)
                                        .shadow(rim(m, light)),
                                )
                                .child(child),
                        ),
                ),
            ),
            )
            .into();
    }
    if m.z < 0.0 {
        // Below the ground: no shadow cast, the wall facing the light shades
        // the floor of the well instead — the opposite of a raised surface.
        let depth = -m.z;
        let (lit, _) = shade(m, light);
        return Container::new()
            .decoration(
                BoxDecoration::new()
                    .radius(radius)
                    .color(rgb(lit))
                    .border(Border::new(Color::rgba(255, 255, 255, 12), HAIR))
                    .shadow(Shadow::inset(
                        rgba([0.0, 0.0, 0.0], 0.85),
                        Offset::new(-light.from.dx * depth * 3.0, -light.from.dy * depth * 3.0),
                        2.0 + depth * 4.0,
                    )),
            )
            .child(child)
            .into();
    }
    Container::new()
        .decoration(BoxDecoration::new().radius(radius).shadow(shadow(m, light)))
        .child(
            Container::new()
                .decoration(BoxDecoration::new().radius(radius).shadow(contact(m, light)))
                .child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .radius(radius)
                                .gradient(sheen_for(m, light, inks))
                                .border(border)
                                .shadow(rim(m, light)),
                        )
                        .child(child),
                ),
        )
        .into()
}

// ---- the demo screen -------------------------------------------------------

// ---- palettes ----------------------------------------------------------------

/// The colours an app brings: an accent and whether its ground is light or
/// dark. Everything else — every surface's lit colour, every text colour —
/// is worked out from these and the light.
#[derive(Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    /// The accent, as an sRGB hex like a brand guide would give it.
    pub accent: u32,
    /// A light ground and light surfaces instead of dark ones.
    pub light: bool,
}

pub const PALETTES: [Palette; 7] = [
    Palette { name: "Dark · violet", accent: 0x5B3BE8, light: false },
    Palette { name: "Light · violet", accent: 0x5B3BE8, light: true },
    Palette { name: "Red", accent: 0xE5383B, light: false },
    Palette { name: "Green", accent: 0x2BA84A, light: true },
    Palette { name: "Yellow", accent: 0xF5C518, light: false },
    Palette { name: "White", accent: 0xF2F2F2, light: false },
    Palette { name: "Black", accent: 0x141414, light: true },
];

fn decode(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn hex_linear(hex: u32) -> [f32; 3] {
    [decode((hex >> 16) as u8), decode((hex >> 8) as u8), decode(hex as u8)]
}

impl Palette {
    /// The theme's brightness: a dark theme is exposed low — on OLED that is
    /// the battery — and a light theme high, whatever the light is doing.
    /// The time of day only trims it a little.
    fn expose(self, light: Light) -> Light {
        let base = if self.light { 1.0 } else { 0.5 };
        // A light theme is a lit page, not a sunset: it keeps a trace of the
        // light's warmth and stays white. A dark theme keeps all of it.
        let keep = if self.light { 0.25 } else { 1.0 };
        let color = add(scale(light.color, keep), scale([1.0, 1.0, 1.0], 1.0 - keep));
        Light { exposure: base * (light.exposure / 0.95), color, ..light }
    }

    fn surface(self) -> [f32; 3] {
        if self.light { [0.93, 0.935, 0.95] } else { [0.16, 0.17, 0.23] }
    }
    fn pill(self) -> [f32; 3] {
        if self.light { [0.70, 0.71, 0.75] } else { [0.085, 0.09, 0.12] }
    }
    fn floor(self) -> [f32; 3] {
        if self.light { [0.66, 0.67, 0.71] } else { [0.07, 0.075, 0.09] }
    }
    fn content(self) -> Material {
        Material { base: self.surface(), ..SATIN }
    }
    fn chip(self) -> Material {
        Material { base: self.pill(), ..MATTE }
    }
    fn field(self) -> Material {
        Material { base: scale(self.pill(), if self.light { 0.92 } else { 0.8 }), ..MATTE }
    }
    /// The featured surface: the content surface, faintly carrying the accent.
    fn feature(self) -> Material {
        let tint = if self.light { 0.10 } else { 0.22 };
        Material { base: add(scale(self.surface(), 1.0 - tint), scale(hex_linear(self.accent), tint)), ..SATIN }
    }
    fn action(self) -> Material {
        Material { base: hex_linear(self.accent), ..GLOSS }
    }
}

// ---- legibility ----------------------------------------------------------------

/// Relative luminance of a linear colour (WCAG).
fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0].clamp(0.0, 1.0) + 0.7152 * c[1].clamp(0.0, 1.0) + 0.0722 * c[2].clamp(0.0, 1.0)
}

/// The WCAG contrast ratio between two linear colours.
pub fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text for a surface whose lit colour is `under`: light or dark, whichever
/// reads, faintly tinted by the light; `strong` for headings and labels,
/// otherwise the secondary shade. The theme picks it — so changing the
/// light, or the brand colour, can never make text unreadable.
fn text_on(under: [f32; 3], light: Light, strong: bool) -> [f32; 3] {
    let warm = scale(light.color, 1.0 / light.color[0].max(light.color[1]).max(light.color[2]).max(0.01));
    let pale = add(scale(warm, 0.90), [0.08, 0.08, 0.08]);
    let dark = add(scale(warm, 0.004), [0.003, 0.003, 0.004]);
    let pick = if contrast(pale, under) >= contrast(dark, under) { pale } else { dark };
    if strong {
        return pick;
    }
    // The secondary shade: as far toward the surface as it can go while
    // still clearing 4.5:1.
    let mut t = 0.45;
    loop {
        let c = add(scale(pick, 1.0 - t), scale(under, t));
        if contrast(c, under) >= 4.5 || t <= 0.0 {
            return c;
        }
        t -= 0.05;
    }
}

/// A brand colour that cannot carry readable text as it is — a mid-tone red,
/// say, that holds neither white nor black at 4.5:1 — is moved just far
/// enough, darker or lighter, behind the text. The hue stays; only its
/// lightness gives way.
fn legible(m: Material, light: Light) -> Material {
    let mut m = m;
    for _ in 0..30 {
        let lit = shade(m, light).0;
        let ink = text_on(lit, light, true);
        if contrast(ink, lit) >= 4.8 {
            break;
        }
        let toward_dark = luminance(ink) > luminance(lit);
        m.base = if toward_dark { scale(m.base, 0.92) } else { m.base.map(|c| (c * 1.08).min(1.0)) };
    }
    m
}

/// Secondary text on the ground: readable from the lit end of its gradient
/// to the shaded end, or it takes the strong colour.
fn ground_caption(light: Light, pal: Palette) -> [f32; 3] {
    let g = ground_lit(light, pal);
    let c = text_on(g, light, false);
    if contrast(c, g).min(contrast(c, scale(g, 0.6))) >= 4.5 {
        c
    } else {
        text_on(g, light, true)
    }
}

/// One checked pairing: where, the ratio, and whether it passes.
pub struct Check {
    pub place: &'static str,
    pub ratio: f32,
}

/// Every text-on-surface pairing on the screen, measured at the surface's
/// darkest and lightest lit point (its sheen runs between them).
pub fn checks(light: Light, pal: Palette) -> Vec<Check> {
    let light = pal.expose(light);
    let mut out = Vec::new();
    let mut pair = |place: &'static str, m: Material, z: f32, strong: bool| {
        let m = legible(Material { z, ..m }, light);
        let lit = shade(m, light).0;
        let inks = [text_on(lit, light, true), text_on(lit, light, false)];
        let (far, lit, hot) = range(m, light, &inks);
        let t = inks[if strong { 0 } else { 1 }];
        let ratio = contrast(t, far).min(contrast(t, lit)).min(contrast(t, hot));
        out.push(Check { place, ratio });
    };
    pair("card heading", pal.content(), Role::Content.z(), true);
    pair("card body", pal.content(), Role::Content.z(), false);
    pair("feature heading", pal.feature(), Role::Feature.z(), true);
    pair("feature body", pal.feature(), Role::Feature.z(), false);
    pair("pill label", pal.chip(), Role::Pill.z(), false);
    pair("field hint", pal.field(), Role::Field.z(), false);
    pair("action label", pal.action(), Role::Action.z(), true);
    let g = ground_lit(light, pal);
    let t = text_on(g, light, true);
    out.push(Check { place: "title on ground", ratio: contrast(t, g).min(contrast(t, scale(g, 0.6))) });
    let t = ground_caption(light, pal);
    out.push(Check { place: "caption on ground", ratio: contrast(t, g).min(contrast(t, scale(g, 0.6))) });
    out
}

// ---- the ground ----------------------------------------------------------------

fn ground_lit(light: Light, pal: Palette) -> [f32; 3] {
    add(mul(pal.floor(), irradiance(light)), scale(light.ambient, 0.4))
}

/// The ground, lit too: brighter on the side the light comes from.
fn ground(light: Light, pal: Palette) -> WidgetNode {
    let lit = ground_lit(light, pal);
    let near = add(lit, scale(light.color, 0.09 * light.intensity));
    let start = Offset::new(0.5 + light.from.dx * 0.6, 0.5 + light.from.dy * 0.6);
    let end = Offset::new(0.5 - light.from.dx * 0.6, 0.5 - light.from.dy * 0.6);
    Container::new()
        .gradient(
            Gradient::linear(start, end)
                .with_dither()
                .with_stops(&[(0.0, rgb(near)), (1.0, rgb(scale(lit, 0.6)))]),
        )
        .child(SizedBox::expand())
        .into()
}

// ---- layers by role --------------------------------------------------------

/// What a widget *is* decides how near it sits — the way a portrait puts
/// the subject close and lets the background fall away. The developer never
/// picks a height: using a button already says "this is near".
#[derive(Clone, Copy)]
pub enum Role {
    Action,
    Secondary,
    Control,
    Content,
    /// The one featured surface on a screen — its focal point.
    Feature,
    Pill,
    Field,
}

impl Role {
    pub const fn z(self) -> f32 {
        match self {
            Self::Action => 8.0,
            Self::Secondary => 6.0,
            Self::Control => 4.5,
            Self::Content => 3.0,
            Self::Feature => 4.5,
            Self::Pill => 0.3,
            Self::Field => -1.5,
        }
    }
}

fn layered(role: Role, m: Material, light: Light, tilt: f32, radius: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    let z = role.z();
    let m = legible(Material { z, ..m }, light);
    let lit = shade(m, light).0;
    let inks = [text_on(lit, light, true), text_on(lit, light, false)];
    Transformed::new(vieww_foundation::Transform::translate(Offset::new(tilt * z * 1.6, 0.0)))
        .child(surface_for(m, light, radius, &inks, child))
        .into()
}

/// One screen whose depth comes from roles, lit by one light, in one palette.
pub fn screen(light: Light, tilt: f32, pal: Palette) -> WidgetNode {
    let light = pal.expose(light);
    let lit_of = |m: Material, z: f32| shade(legible(Material { z, ..m }, light), light).0;
    let on = |m: Material, z: f32, strong: bool| rgb(text_on(lit_of(m, z), light, strong));
    let content = pal.content();
    let chip = pal.chip();
    let field = pal.field();
    let action = pal.action();
    let g = ground_lit(light, pal);
    let title_c = rgb(text_on(g, light, true));
    let sub_c = rgb(ground_caption(light, pal));

    let pill = |label: &str| {
        layered(
            Role::Pill,
            chip,
            light,
            tilt,
            13.0,
            Container::new()
                .padding(EdgeInsets::symmetric(12.0, 6.0))
                .child(text(label, 12.0, FontWeight::Medium, on(chip, Role::Pill.z(), false))),
        )
    };

    let search = layered(
        Role::Field,
        field,
        light,
        tilt,
        14.0,
        Container::new()
            .height(46.0)
            .padding(EdgeInsets::symmetric(16.0, 0.0))
            .alignment(Alignment::CENTER_LEFT)
            .child(text("Search your evening…", 14.0, FontWeight::Regular, on(field, Role::Field.z(), false))),
    );

    let z = Role::Content.z();
    let feature = pal.feature();
    let article = layered(
        Role::Feature,
        feature,
        light,
        tilt,
        22.0,
        Padding::new(EdgeInsets::all(18.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(6.0)
                .children(children![
                    text("Your evening, in focus", 20.0, FontWeight::Bold, on(feature, Role::Feature.z(), true)),
                    text(
                        "Three things left, one call at 9. The rest can wait until tomorrow morning.",
                        14.0,
                        FontWeight::Regular,
                        on(feature, Role::Feature.z(), false),
                    ),
                    SizedBox::height(6.0),
                    progress(pal, 0.62),
                    SizedBox::height(2.0),
                    Flex::row().spacing(8.0).children(children![pill("Focus"), pill("Calls only"), pill("50 min")]),
                ]),
        ),
    );

    let control_row = layered(
        Role::Content,
        content,
        light,
        tilt,
        20.0,
        Container::new().padding(EdgeInsets::symmetric(18.0, 14.0)).child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    Flexible::expanded(1).child(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(3.0)
                            .children(children![
                                text("Do not disturb", 15.0, FontWeight::Medium, on(content, z, true)),
                                text("Until the session ends", 12.0, FontWeight::Regular, on(content, z, false)),
                            ]),
                    ),
                    layered(
                        Role::Control,
                        Material { base: hex_linear(pal.accent), ..METAL },
                        light,
                        tilt,
                        16.0,
                        Container::new()
                            .size(56.0, 32.0)
                            .padding(EdgeInsets::all(4.0))
                            .alignment(Alignment::CENTER_RIGHT)
                            .child(
                                Container::new()
                                    .decoration(BoxDecoration::new().stadium().color(Color::rgba(255, 255, 255, 235)))
                                    .size(24.0, 24.0),
                            ),
                    ),
                ]),
        ),
    );

    let actions = Flex::row().spacing(12.0).children(children![
        Flexible::expanded(1).child(layered(
            Role::Secondary,
            Material { base: scale(add(pal.surface(), hex_linear(pal.accent)), 0.5), ..GLASS },
            light,
            tilt,
            26.0,
            Container::new()
                .height(52.0)
                .alignment(Alignment::CENTER)
                .child(text("Later", 15.0, FontWeight::Bold, title_c)),
        )),
        Flexible::expanded(2).child(layered(
            Role::Action,
            action,
            light,
            tilt,
            26.0,
            Container::new()
                .height(52.0)
                .alignment(Alignment::CENTER)
                .child(text("Start focus", 16.0, FontWeight::Bold, on(action, Role::Action.z(), true))),
        )),
    ]);

    Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(ground(light, pal)))
        .push(
            Positioned::fill().child(
                Padding::new(EdgeInsets::only(20.0, 48.0, 20.0, 0.0)).child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .spacing(18.0)
                        .children(children![
                            text(&format!("{} · {}", pal.name, light.name), 13.0, FontWeight::Bold, sub_c),
                            text("Good evening, Teja", 28.0, FontWeight::Bold, title_c),
                            search,
                            article,
                            control_row,
                            Flex::row().spacing(8.0).children(children![pill("Today"), pill("3 left"), pill("Quiet")]),
                            SizedBox::height(6.0),
                            actions,
                        ]),
                ),
            ),
        )
        .into()
}

// ---- more screens, same rules --------------------------------------------------

/// The roles, bound to one light, tilt and palette — so a screen is written
/// in terms of *what things are* and never mentions a shadow or a colour.
#[derive(Clone, Copy)]
struct Kit {
    light: Light,
    tilt: f32,
    pal: Palette,
}

impl Kit {
    fn new(light: Light, tilt: f32, pal: Palette) -> Self {
        Self { light: pal.expose(light), tilt, pal }
    }
    fn ink(self, m: Material, role: Role, strong: bool) -> Color {
        let lit = shade(legible(Material { z: role.z(), ..m }, self.light), self.light).0;
        rgb(text_on(lit, self.light, strong))
    }
    fn on_ground(self, strong: bool) -> Color {
        if strong {
            rgb(text_on(ground_lit(self.light, self.pal), self.light, true))
        } else {
            rgb(ground_caption(self.light, self.pal))
        }
    }
    fn title(self, s: &str) -> WidgetNode {
        text(s, 28.0, FontWeight::Bold, self.on_ground(true))
    }
    fn label(self, s: &str) -> WidgetNode {
        text(s, 12.0, FontWeight::Bold, self.on_ground(false))
    }
    fn card(self, radius: f32, child: impl Into<WidgetNode>) -> WidgetNode {
        layered(Role::Content, self.pal.content(), self.light, self.tilt, radius, child)
    }
    fn heading(self, s: &str, size: f32) -> WidgetNode {
        text(s, size, FontWeight::Bold, self.ink(self.pal.content(), Role::Content, true))
    }
    fn body(self, s: &str) -> WidgetNode {
        text(s, 13.0, FontWeight::Regular, self.ink(self.pal.content(), Role::Content, false))
    }
    fn pill(self, s: &str) -> WidgetNode {
        layered(
            Role::Pill,
            self.pal.chip(),
            self.light,
            self.tilt,
            12.0,
            Container::new()
                .padding(EdgeInsets::symmetric(10.0, 5.0))
                .child(text(s, 11.0, FontWeight::Medium, self.ink(self.pal.chip(), Role::Pill, false))),
        )
    }
    fn toggle(self, on: bool) -> WidgetNode {
        let base = if on { hex_linear(self.pal.accent) } else { scale(self.pal.pill(), 0.9) };
        layered(
            Role::Control,
            Material { base, ..METAL },
            self.light,
            self.tilt,
            16.0,
            Container::new()
                .size(52.0, 30.0)
                .padding(EdgeInsets::all(4.0))
                .alignment(if on { Alignment::CENTER_RIGHT } else { Alignment::CENTER_LEFT })
                .child(
                    Container::new()
                        .decoration(BoxDecoration::new().stadium().color(Color::rgba(255, 255, 255, 235)))
                        .size(22.0, 22.0),
                ),
        )
    }
    fn action(self, s: &str) -> WidgetNode {
        let m = self.pal.action();
        layered(
            Role::Action,
            m,
            self.light,
            self.tilt,
            24.0,
            Container::new()
                .height(50.0)
                .alignment(Alignment::CENTER)
                .child(text(s, 15.0, FontWeight::Bold, self.ink(m, Role::Action, true))),
        )
    }
    fn secondary(self, s: &str) -> WidgetNode {
        layered(
            Role::Secondary,
            Material { base: scale(add(self.pal.surface(), hex_linear(self.pal.accent)), 0.5), ..GLASS },
            self.light,
            self.tilt,
            24.0,
            Container::new()
                .height(50.0)
                .alignment(Alignment::CENTER)
                .child(text(s, 15.0, FontWeight::Bold, self.on_ground(true))),
        )
    }
    fn field(self, hint: &str) -> WidgetNode {
        let m = self.pal.field();
        layered(
            Role::Field,
            m,
            self.light,
            self.tilt,
            14.0,
            Container::new()
                .height(44.0)
                .padding(EdgeInsets::symmetric(14.0, 0.0))
                .alignment(Alignment::CENTER_LEFT)
                .child(text(hint, 14.0, FontWeight::Regular, self.ink(m, Role::Field, false))),
        )
    }
    fn page(self, children: Vec<WidgetNode>) -> WidgetNode {
        let mut col = Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(14.0);
        for c in children {
            col = col.push(c);
        }
        Stack::new()
            .fit(StackFit::Expand)
            .push(Positioned::fill().child(ground(self.light, self.pal)))
            .push(Positioned::fill().child(Padding::new(EdgeInsets::only(20.0, 48.0, 20.0, 0.0)).child(col)))
            .into()
    }
}

fn row_between(left: WidgetNode, right: WidgetNode) -> WidgetNode {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(children![Flexible::expanded(1).child(left), right])
        .into()
}

/// A message list: many cards, each with pills — the middle layers under
/// load.
pub fn list_screen(light: Light, tilt: f32, pal: Palette) -> WidgetNode {
    let k = Kit::new(light, tilt, pal);
    let msg = |from: &str, line: &str, tags: [&str; 2]| {
        k.card(
            18.0,
            Padding::new(EdgeInsets::symmetric(16.0, 12.0)).child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .spacing(5.0)
                    .children(children![
                        k.heading(from, 15.0),
                        k.body(line),
                        Flex::row().spacing(6.0).children(children![k.pill(tags[0]), k.pill(tags[1])]),
                    ]),
            ),
        )
    };
    k.page(vec![
        k.label("INBOX · 4 NEW"),
        k.title("Messages"),
        k.field("Search messages…"),
        msg("Asha", "The studio can do Friday at 4. Does that work?", ["Work", "Today"]),
        msg("Ravi", "Photos from the weekend are up — have a look.", ["Family", "2 h"]),
        msg("Billing", "Your invoice for October is ready.", ["Receipts", "Yesterday"]),
        msg("Meera", "Running ten minutes late, sorry!", ["Friends", "Mon"]),
        k.action("New message"),
    ])
}

/// A dense settings page: controls on cards, rows close together.
pub fn settings_screen(light: Light, tilt: f32, pal: Palette) -> WidgetNode {
    let k = Kit::new(light, tilt, pal);
    let line = || Container::new().color(Color::rgba(128, 128, 128, 40)).height(HAIR).into();
    let row = |title: &str, sub: &str, on: bool| -> WidgetNode {
        Container::new()
            .height(54.0)
            .child(row_between(
                Flex::column()
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .spacing(2.0)
                    .children(children![k.heading(title, 14.0), k.body(sub)])
                    .into(),
                k.toggle(on),
            ))
            .into()
    };
    let group = |rows: Vec<WidgetNode>| {
        let mut col = Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch);
        for r in rows {
            col = col.push(r);
        }
        k.card(18.0, Padding::new(EdgeInsets::symmetric(16.0, 2.0)).child(col))
    };
    k.page(vec![
        k.label("SETTINGS"),
        k.title("Notifications"),
        group(vec![
            row("Allow notifications", "On this phone", true),
            line(),
            row("Show previews", "When unlocked", true),
            line(),
            row("Sounds", "For messages and calls", false),
        ]),
        k.label("QUIET HOURS"),
        group(vec![
            row("Scheduled", "10 pm – 7 am", true),
            line(),
            row("Allow repeat callers", "Second call within 3 min", false),
            line(),
            row("Favourites break through", "Calls only", true),
        ]),
        Flex::row().spacing(6.0).children(children![k.pill("Work"), k.pill("Family"), k.pill("Friends"), k.pill("+ Add")]).into(),
    ])
}

/// A chat: bubbles are content, the composer is a field, send is the action.
pub fn chat_screen(light: Light, tilt: f32, pal: Palette) -> WidgetNode {
    let k = Kit::new(light, tilt, pal);
    let bubble = |s: &str, mine: bool| -> WidgetNode {
        let b = k.card(
            18.0,
            Container::new().padding(EdgeInsets::symmetric(14.0, 10.0)).child(k.body(s)),
        );
        Flex::row()
            .main_axis_alignment(if mine { MainAxisAlignment::End } else { MainAxisAlignment::Start })
            .children(children![Container::new().width(250.0).child(b)])
            .into()
    };
    k.page(vec![
        k.label("ASHA · ONLINE"),
        k.title("Asha"),
        Flex::row().main_axis_alignment(MainAxisAlignment::Center).children(children![k.pill("Today")]).into(),
        bubble("Hi! Are we still on for the studio this week?", false),
        bubble("Yes — Friday works best for me.", true),
        bubble("Great. They can do 4 pm. I'll book it.", false),
        bubble("Perfect, see you there.", true),
        SizedBox::height(60.0).into(),
        Flex::row()
            .spacing(10.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children![
                Flexible::expanded(1).child(k.field("Message…")),
                Container::new().width(96.0).child(k.action("Send")),
            ])
            .into(),
    ])
}

/// The home screen with a dialog open: the dialog becomes the nearest thing
/// on the screen, above even the actions it covers.
pub fn dialog_screen(light: Light, tilt: f32, pal: Palette) -> WidgetNode {
    let k = Kit::new(light, tilt, pal);
    // The dialog is the nearest thing on the screen, and it looks it: a
    // lighter surface than anything under it, the longest shadow, and the
    // screen beneath stepping back, darkening and softening.
    let lift = if pal.light { [1.0, 1.0, 1.0] } else { scale(pal.surface(), 1.7) };
    let dialog_m = Material { z: 10.0, base: lift, ..pal.content() };
    let dl = legible(dialog_m, k.light);
    let lit = shade(dl, k.light).0;
    let inks = [text_on(lit, k.light, true), text_on(lit, k.light, false)];
    let dialog = surface_for(
        dl,
        k.light,
        24.0,
        &inks,
        Padding::new(EdgeInsets::all(20.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(10.0)
                .children(children![
                    text("Start a focus session?", 19.0, FontWeight::Bold, rgb(inks[0])),
                    text("Calls from favourites still ring. Everything else waits for 50 minutes.", 13.0, FontWeight::Regular, rgb(inks[1])),
                    SizedBox::height(4.0),
                    k.action("Start"),
                    k.secondary("Not now"),
                ]),
        ),
    );
    Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(ground(k.light, pal)))
        .push(Positioned::fill().child(
            Filtered::blur(2.5).child(
                Transformed::new(vieww_foundation::Transform::scale_around(Offset::new(195.0, 422.0), 0.96, 0.96))
                    .child(screen(light, tilt, pal)),
            ),
        ))
        .push(Positioned::fill().child(Container::new().color(Color::rgba(0, 0, 0, if pal.light { 90 } else { 165 })).child(SizedBox::expand())))
        .push(Positioned::new().left(24.0).right(24.0).top(250.0).child(dialog))
        .into()
}

/// A progress track sunk into its surface, filled with the accent.
fn progress(pal: Palette, value: f32) -> WidgetNode {
    let width = 300.0;
    Stack::new()
        .push(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .color(if pal.light { Color::rgba(0, 0, 0, 30) } else { Color::rgba(0, 0, 0, 120) })
                        .shadow(Shadow::inset(Color::rgba(0, 0, 0, 120), Offset::new(0.0, 1.0), 2.0)),
                )
                .size(width, 8.0),
        )
        .push(
            Container::new()
                .decoration(BoxDecoration::new().stadium().color(rgb(hex_linear(pal.accent))))
                .size(width * value, 8.0),
        )
        .into()
}
