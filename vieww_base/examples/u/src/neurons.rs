//! The neuron field, and the companion — the mark that is U.
//!
//! The field is the animated, hand-drawn background of every screen: fourteen
//! somas on a deterministic layout, dendrites bowing between their nearest
//! neighbours, one action potential travelling each edge, and a slow chemical
//! drift filling the space between. Everything is a *function of the clock* —
//! nothing accumulates, nothing steps — so any frame is reproducible from
//! `(time, burst)` alone, and the screenshot tool can photograph a frame
//! without having lived through the ones before it.
//!
//! # The companion
//!
//! [`companion`] is the organism itself: one eye, six strands, two elliptical
//! rings. The strands are **three and three** — three violet signal strands
//! carrying a travelling spark, three solid mint strands — and the same six
//! positions in the same two colours make the small [`sigil`] that sits in
//! the top bar, so the mark and its signature cannot disagree. The rings
//! rotate *in place*, about the mark's exact centre and about nothing else:
//! an ellipse tumbling about its own centre while its rider dot revolves is
//! the whole of their motion. `Reveal` is the splash's choreography — the
//! eye scales in, each strand grows from the eye outward, the atmosphere
//! (rings, aura, float) fades in last — and `Reveal::complete()` is the
//! steady state home keeps for the rest of the app's life.
//!
//! # One additive layer for all the light
//!
//! Every glow — the pulse halos, the soma halos, the luminous edge strokes —
//! goes into a single `BlendMode::Plus` layer. That is the pricing rule from
//! `Sketchbook::layer`'s own documentation: a blurred or blended layer costs
//! one offscreen buffer *per layer*, and the expensive version of this file is
//! the one that wraps each glow separately. Group early, blend once.

use std::f32::consts::{PI, TAU};

use vieww_foundation::{
    BlendMode, Color, Dash, Gradient, Offset, Path, Size, Sketchbook, StrokeStyle, Transform,
};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::palette::{self, AXON, MEMBRANE, SOMA, SYNAPSE};

/// Where the somas sit, in fractions of the field. Hand-placed: an organic
/// network is a composition, and a hash of the index is not.
const LAYOUT: [(f32, f32); 14] = [
    (0.10, 0.050),
    (0.52, 0.100),
    (0.88, 0.045),
    (0.04, 0.210),
    (0.95, 0.180),
    (0.20, 0.330),
    (0.83, 0.300),
    (0.06, 0.480),
    (0.94, 0.460),
    (0.16, 0.640),
    (0.86, 0.620),
    (0.09, 0.830),
    (0.55, 0.900),
    (0.90, 0.870),
];

/// Which somas connect, by index into [`LAYOUT`]. Edges near the frame first,
/// so the middle of the screen stays quiet under the content.
const EDGES: [(usize, usize); 18] = [
    (0, 1),
    (1, 2),
    (2, 4),
    (4, 6),
    (6, 8),
    (8, 10),
    (10, 13),
    (0, 3),
    (3, 5),
    (3, 7),
    (7, 9),
    (5, 9),
    (9, 11),
    (11, 12),
    (12, 13),
    (7, 11),
    (2, 6),
    (1, 6),
];

/// The drifting molecules between the cells: base position in fractions of the
/// field, then drift amplitude, speed and phase.
const DRIFT: [(f32, f32, f32, f32); 18] = [
    (0.32, 0.05, 5.0, 0.1),
    (0.68, 0.09, 4.0, 1.2),
    (0.42, 0.20, 6.0, 2.3),
    (0.60, 0.26, 3.5, 0.7),
    (0.30, 0.44, 5.5, 3.1),
    (0.72, 0.40, 4.5, 1.9),
    (0.48, 0.56, 6.5, 0.4),
    (0.24, 0.74, 4.0, 2.7),
    (0.66, 0.78, 5.0, 1.5),
    (0.40, 0.86, 3.5, 0.2),
    (0.56, 0.14, 7.0, 2.9),
    (0.14, 0.36, 4.5, 1.1),
    (0.86, 0.33, 5.5, 3.3),
    (0.78, 0.52, 6.0, 0.9),
    (0.10, 0.70, 4.0, 2.1),
    (0.92, 0.72, 5.0, 1.7),
    (0.36, 0.62, 6.5, 3.7),
    (0.62, 0.68, 4.5, 0.6),
];

/// A soma's own rhythm: radius in px, firing period in seconds, phase offset
/// in seconds, and how far its hue sits between synapse cyan and axon violet.
fn cell(i: usize) -> (f32, f32, f32, f32) {
    let radius = 8.5 + (i % 4) as f32 * 3.0;
    let period = 3.6 + ((i * 7) % 5) as f32 * 0.9;
    let phase = ((i * 13) % 17) as f32 / 17.0 * period;
    let hue = (i % 7) as f32 / 6.0;
    (radius, period, phase, hue)
}

/// One smooth firing spike: `0` at rest, rising to `1` and back over the first
/// quarter of the period. A sine bump rather than a pulse, because a spike
/// with corners reads as a blink.
fn spike(t: f32, period: f32, phase: f32) -> f32 {
    let x = ((t + phase) / period).fract();
    if x < 0.25 {
        (x / 0.25 * PI).sin()
    } else {
        0.0
    }
}

/// The point at `u` on the cubic from `a` to `b`, bowed sideways by `bow`.
fn bowed(a: Offset, b: Offset, bow: f32, u: f32) -> Offset {
    let dx = b.dx - a.dx;
    let dy = b.dy - a.dy;
    // The perpendicular, normalised, times the bow distance.
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let nx = -dy / len * bow;
    let ny = dx / len * bow;
    let c1 = Offset::new(a.dx + dx / 3.0 + nx, a.dy + dy / 3.0 + ny);
    let c2 = Offset::new(a.dx + dx * 2.0 / 3.0 + nx, a.dy + dy * 2.0 / 3.0 + ny);
    let (u2, u3) = (u * u, u * u * u);
    let w0 = 1.0 - 3.0 * u + 3.0 * u2 - u3;
    let w1 = 3.0 * u - 6.0 * u2 + 3.0 * u3;
    let w2 = 3.0 * u2 - 3.0 * u3;
    Offset::new(
        w0 * a.dx + w1 * c1.dx + w2 * c2.dx + u3 * b.dx,
        w0 * a.dy + w1 * c1.dy + w2 * c2.dy + u3 * b.dy,
    )
}

/// The field as a widget: fills whatever box it is given.
///
/// `intensity` is how alive it is — the full `1.0` behind home, dimmed behind
/// settings where the form, not the biology, is the subject. `burst` is the
/// afterglow of a training session: every cell fires at once, then settles.
#[must_use]
pub fn field(time: f32, burst: f32, intensity: f32) -> Painting {
    Painting::new(PaintWith::new(move |book, size| {
        paint_field(book, size, time, burst, intensity);
    }))
}

fn paint_field(book: &mut Sketchbook, size: Size, time: f32, burst: f32, intensity: f32) {
    // Cell positions in this box.
    let at = |(x, y): (f32, f32)| Offset::new(x * size.width, y * size.height);

    // How lit each cell is, including the training afterglow.
    let firing = |i: usize| {
        let (_, period, phase, _) = cell(i);
        let own = spike(time, period, phase);
        (own + burst * 0.9).min(1.0)
    };

    // ---- the wiring: dendrites as bowed strokes --------------------------

    for (e, &(a, b)) in EDGES.iter().enumerate() {
        let pa = at(LAYOUT[a]);
        let pb = at(LAYOUT[b]);
        let (dx, dy) = (pb.dx - pa.dx, pb.dy - pa.dy);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let bow = len * 0.16 * if e % 2 == 0 { 1.0 } else { -1.0 };
        let (nx, ny) = (-dy / len * bow, dx / len * bow);
        let c1 = Offset::new(pa.dx + dx / 3.0 + nx, pa.dy + dy / 3.0 + ny);
        let c2 = Offset::new(pa.dx + dx * 2.0 / 3.0 + nx, pa.dy + dy * 2.0 / 3.0 + ny);
        let hue = ((a + b) % 7) as f32 / 6.0;
        let color = palette::mix(SYNAPSE, AXON, hue);
        let lit = firing(a).max(firing(b)) * intensity;

        let mut path = Path::new();
        path.move_to(pa);
        path.cubic_to(c1, c2, pb);
        book.stroke(path, color.with_alpha((26.0 + lit * 60.0) as u8), 1.15);
    }

    // ---- all the light, in one additive layer ----------------------------

    book.blended_layer(intensity, 0.0, BlendMode::Plus, None, |light| {
        // Pulse halos travelling the edges — one per edge, the odd edges
        // running the other way so the field has no grain. It was two per
        // edge until the first phone build: every halo is a radial gradient
        // in the one additive layer, and that layer is the size of the
        // screen.
        for (e, &(a, b)) in EDGES.iter().enumerate() {
            let pa = at(LAYOUT[a]);
            let pb = at(LAYOUT[b]);
            let dist = ((pb.dx - pa.dx).powi(2) + (pb.dy - pa.dy).powi(2)).sqrt();
            let bow = dist * 0.16 * if e % 2 == 0 { 1.0 } else { -1.0 };
            let speed = 0.16 + (e % 3) as f32 * 0.05;
            let lit = firing(a).max(firing(b));

            #[allow(clippy::cast_precision_loss)]
            let mut u = (time * speed + e as f32 * 0.37).fract();
            if e % 2 == 1 {
                u = 1.0 - u;
            }
            let p = bowed(pa, pb, bow, u);
            let hue = ((a + b) % 7) as f32 / 6.0;
            let color = palette::mix(SYNAPSE, AXON, hue);
            let glow = (0.35 + 0.65 * lit) * intensity;
            light.circle(
                p,
                7.0,
                Gradient::radial_fill().with_stops(&[
                    (0.0, color.with_alpha((90.0 * glow) as u8)),
                    (0.55, color.with_alpha((30.0 * glow) as u8)),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
            light.circle(p, 1.7, color.with_alpha((205.0 * glow) as u8));
        }

        // Soma halos.
        for (i, &pos) in LAYOUT.iter().enumerate() {
            let (radius, _, _, hue) = cell(i);
            let p = at(pos);
            let color = palette::mix(SYNAPSE, AXON, hue);
            let glow = (0.3 + 0.7 * firing(i)) * intensity;
            light.circle(
                p,
                radius * 3.1,
                Gradient::radial_fill().with_stops(&[
                    (0.0, color.with_alpha((56.0 * glow) as u8)),
                    (0.5, color.with_alpha((22.0 * glow) as u8)),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
        }

        // The chemical drift: small molecules wandering between the cells.
        for (i, &(x, y, amp, speed)) in DRIFT.iter().enumerate() {
            let phase = i as f32 * 1.7;
            let p = Offset::new(
                x * size.width + amp * (time * speed * 0.35 + phase).sin(),
                y * size.height + amp * (time * speed * 0.28 + phase * 1.3).cos(),
            );
            let twinkle = 0.5 + 0.5 * (time * speed * 0.9 + phase * 2.1).sin();
            let color = if i % 3 == 0 { MEMBRANE } else { SYNAPSE };
            light.circle(p, 1.1, color.with_alpha((46.0 * twinkle * intensity) as u8));
        }
    });

    // ---- the cells themselves --------------------------------------------

    for (i, &pos) in LAYOUT.iter().enumerate() {
        let (radius, _, _, hue) = cell(i);
        let p = at(pos);
        let color = palette::mix(SYNAPSE, AXON, hue);
        let fire = firing(i) * intensity;

        // Cytoplasm: a soft disc, brighter when firing.
        book.circle(
            p,
            radius,
            Gradient::radial_fill().with_stops(&[
                (0.0, color.with_alpha((120.0 + fire * 90.0) as u8)),
                (0.7, color.with_alpha((46.0 + fire * 60.0) as u8)),
                (1.0, Color::TRANSPARENT),
            ]),
        );
        // Membrane.
        book.ring(
            p,
            radius,
            1.2,
            color.with_alpha((70.0 + fire * 120.0) as u8),
        );
        // Nucleus — the one warm note.
        book.circle(
            p,
            radius * 0.3,
            SOMA.with_alpha((140.0 + fire * 100.0) as u8),
        );
        // The firing ring: expands out and fades as the spike peaks.
        if fire > 0.02 {
            book.ring(
                p,
                radius + 3.0 + fire * 15.0,
                1.3,
                color.with_alpha(((1.0 - fire) * 96.0 * intensity) as u8),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The companion: the mark that is U.
// ---------------------------------------------------------------------------

/// The eye's own colours — the one place the palette serves specific tints
/// rather than roles, because an iris is not a button.
const SCLERA: Color = Color::rgb(18, 26, 43);
const RIM: Color = Color::rgb(185, 165, 255);
const IRIS_RIM: Color = Color::rgb(85, 221, 204);
const IRIS_DEEP: Color = Color::rgb(39, 108, 136);
const PUPIL: Color = Color::rgb(7, 17, 31);

/// The six strands, in the order the splash grows them. Each is the angle it
/// leaves the eye at (degrees, mathematical convention, y up) and whether it
/// is one of the **three violet signal strands** — the other three are solid
/// mint. Three and three, alternating around the eye, mirrored about the
/// vertical: the same arrangement the small [`sigil`] draws, so the mark and
/// its signature cannot disagree.
const STRANDS: [(f32, bool); 6] = [
    (90.0, true),   // straight up — signal
    (30.0, false),  // upper right — mint
    (150.0, false), // upper left — mint
    (330.0, true),  // lower right — signal
    (210.0, true),  // lower left — signal
    (270.0, false), // straight down — mint
];

/// How assembled the companion is — the splash's whole choreography, handed
/// to the painter as numbers rather than replayed through it.
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    /// The eye's scale-in, `0..1`.
    pub eye: f32,
    /// How far each strand has grown from the eye outward, `0..1`, in
    /// [`STRANDS`] order.
    pub strands: [f32; 6],
    /// Each strand's tip dot popping in, `0..1`.
    pub tips: [f32; 6],
    /// The atmosphere — rings, aura pulse, sparks, the float — fading in,
    /// `0..1`.
    pub atmosphere: f32,
}

impl Reveal {
    /// The steady state: everything grown, everything lit. What home keeps
    /// for the rest of the app's life, and what the settings portrait and the
    /// splash both resolve to.
    #[must_use]
    pub const fn complete() -> Self {
        Self {
            eye: 1.0,
            strands: [1.0; 6],
            tips: [1.0; 6],
            atmosphere: 1.0,
        }
    }
}

/// One point on a strand's cubic, in the mark's unit space (radius `1`,
/// canonical strand pointing up). The curve leaves the eye, bows `bow` to one
/// side, and arrives at its tip dead on the ray.
fn strand_point(u: f32, bow: f32, reach: f32) -> Offset {
    let (x, y) = (
        bez(0.0, bow, -bow * 0.6, 0.0, u),
        bez(-0.30, -0.45, -0.62, -reach, u),
    );
    Offset::new(x, y)
}

/// One coordinate of a cubic Bézier.
fn bez(p0: f32, c1: f32, c2: f32, p3: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (1.0 - 3.0 * u + 3.0 * u2 - u3) * p0
        + (3.0 * u - 6.0 * u2 + 3.0 * u3) * c1
        + (3.0 * u2 - 3.0 * u3) * c2
        + u3 * p3
}

/// Rotate a point about the origin by `phi` radians (screen convention:
/// positive turns clockwise on screen).
fn turned(p: Offset, phi: f32) -> Offset {
    let (sin, cos) = phi.sin_cos();
    Offset::new(p.dx * cos - p.dy * sin, p.dx * sin + p.dy * cos)
}

/// An ellipse centred on `c`, tilted by `tiltr` radians, as a path of four
/// Bézier arcs — the canvas approximation, exact enough at these sizes.
fn tilted_ellipse(c: Offset, rx: f32, ry: f32, tiltr: f32) -> Path {
    const K: f32 = 0.552_284_7;
    let at = |x: f32, y: f32| c + turned(Offset::new(x, y), tiltr);
    let mut path = Path::new();
    path.move_to(at(rx, 0.0));
    path.cubic_to(at(rx, -K * ry), at(K * rx, -ry), at(0.0, -ry));
    path.cubic_to(at(-K * rx, -ry), at(-rx, -K * ry), at(-rx, 0.0));
    path.cubic_to(at(-rx, K * ry), at(-K * rx, ry), at(0.0, ry));
    path.cubic_to(at(K * rx, ry), at(rx, K * ry), at(rx, 0.0));
    path
}

/// The eye's blink: `1` open, dipping toward `0` for a moment once every
/// `period` seconds. A sine dip rather than a snap, because an eyelid that
/// closes in one frame reads as a flicker.
fn blink(time: f32, period: f32) -> f32 {
    let x = (time / period).fract();
    if (0.88..0.95).contains(&x) {
        let u = (x - 0.88) / 0.07;
        1.0 - 0.94 * (u * PI).sin().min(1.0)
    } else {
        1.0
    }
}

/// The companion: the organism that is U, at whatever `size` box it is given.
///
/// `reveal` is the splash's staging ([`Reveal::complete`] for the steady
/// state); `burst` is the training afterglow — the strands reach further and
/// the aura flares when a choice lands; `calm` is the app's own reduced-motion
/// switch, dimming the additive light to a third and stilling the travelling
/// sparks.
///
/// **The rings only rotate.** Each is drawn centred on the mark's exact
/// centre and turned about that centre and nothing else, so an ellipse tumbles
/// in place while its rider dot revolves — no offset drift, no wobble.
#[must_use]
pub fn companion(size: f32, time: f32, burst: f32, calm: f32, reveal: Reveal) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            paint_companion(book, s, time, burst, calm, reveal);
        }),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn paint_companion(
    book: &mut Sketchbook,
    s: Size,
    time: f32,
    burst: f32,
    calm: f32,
    reveal: Reveal,
) {
    let r = s.width * 0.5;
    let c = Offset::new(s.width / 2.0, s.height / 2.0);
    let dim = if calm > 0.5 { 0.34 } else { 1.0 };
    let breathe = (time * TAU / 4.4).sin();
    // The float belongs to the organism, not the rings: the eye and its
    // strands ride it, the rings stay put.
    let bob = (time * TAU / 5.8).sin() * 0.047 * r * reveal.atmosphere;
    let reach = 0.76 + burst * 0.10;

    // ---- the rings: rotation, and only rotation ---------------------------

    if reveal.atmosphere > 0.01 {
        let a = reveal.atmosphere * dim;
        // Ring A: violet, one turn every 26 s, its dot riding near the
        // ellipse's long end.
        let turn = time * TAU / 26.0;
        book.transformed(Transform::rotate_around(c, turn), |g| {
            g.stroke(
                tilted_ellipse(c, 0.84 * r, 0.327 * r, (-26.0_f32).to_radians()),
                RIM.with_alpha((97.0 * a) as u8),
                0.008 * r,
            );
            let dot = c + turned(
                Offset::new(0.84 * r * 0.14_f32.cos(), 0.327 * r * 0.14_f32.sin()),
                (-26.0_f32).to_radians(),
            );
            let twinkle = 0.8 + 0.2 * (time * TAU / 3.0).sin();
            g.circle(dot, 0.021 * r, RIM.with_alpha((215.0 * a * twinkle) as u8));
        });
        // Ring B: mint, dashed, the other way and slower — 34 s.
        let turn = -time * TAU / 34.0;
        book.transformed(Transform::rotate_around(c, turn), |g| {
            let mut dashed = StrokeStyle::rounded();
            dashed.dash = Some(Dash {
                pattern: vec![0.02 * r, 0.06 * r],
                offset: 0.0,
            });
            g.stroke_styled(
                tilted_ellipse(c, 0.627 * r, 0.867 * r, 33.0_f32.to_radians()),
                SYNAPSE.with_alpha((64.0 * a) as u8),
                0.007 * r,
                dashed,
            );
            let dot = c + turned(
                Offset::new(0.627 * r * 1.66_f32.cos(), 0.867 * r * 1.66_f32.sin()),
                33.0_f32.to_radians(),
            );
            let twinkle = 0.8 + 0.2 * (time * TAU / 3.7).sin();
            g.circle(
                dot,
                0.019 * r,
                SYNAPSE.with_alpha((200.0 * a * twinkle) as u8),
            );
        });
    }

    // ---- all the light, in one additive layer ----------------------------

    book.blended_layer(1.0, 0.0, BlendMode::Plus, None, |light| {
        // The aura: a breathing violet bloom, the eye's own halo arriving
        // with the eye and the wider atmosphere behind it.
        let halo = (0.5 + 0.5 * reveal.eye) * (0.30 + 0.05 * breathe + burst * 0.45);
        light.circle(
            c,
            0.67 * r * (1.0 + 0.02 * breathe),
            Gradient::radial_fill().with_stops(&[
                (0.0, AXON.with_alpha((115.0 * halo * dim) as u8)),
                (0.55, AXON.with_alpha((36.0 * halo * dim) as u8)),
                (1.0, Color::TRANSPARENT),
            ]),
        );
        if reveal.atmosphere > 0.01 {
            light.circle(
                c,
                r * (1.0 + 0.02 * breathe),
                Gradient::radial_fill().with_stops(&[
                    (0.0, AXON.with_alpha((38.0 * reveal.atmosphere * dim) as u8)),
                    (
                        0.55,
                        SYNAPSE.with_alpha((14.0 * reveal.atmosphere * dim) as u8),
                    ),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
            // Two pulse rings, expanding out through the rings and fading.
            for k in 0..2 {
                let phase = ((time + 1.8 * k as f32) / 3.6).fract();
                let alpha = (1.0 - phase) * 26.0 * reveal.atmosphere * dim;
                if alpha > 0.5 {
                    light.ring(
                        c,
                        0.62 * r + phase * 0.52 * r,
                        0.008 * r,
                        RIM.with_alpha(alpha as u8),
                    );
                }
            }
        }

        // The organism floats; the light it gives off floats with it.
        light.transformed(Transform::translate(Offset::new(0.0, bob)), |g| {
            let eye_scale = 0.25 + 0.75 * reveal.eye;

            // The core glow behind the eye.
            let core = (0.34 + burst * 0.5) * (0.5 + 0.5 * reveal.eye);
            g.transformed(Transform::scale_around(c, eye_scale, eye_scale), |eye| {
                eye.circle(
                    c,
                    0.32 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(201, 250, 255).with_alpha((120.0 * core * dim) as u8),
                        ),
                        (0.5, AXON.with_alpha((60.0 * core * dim) as u8)),
                        (1.0, Color::TRANSPARENT),
                    ]),
                );
            });

            // The six strands, three signal and three solid.
            for (i, &(angle, signal)) in STRANDS.iter().enumerate() {
                let grown = reveal.strands[i];
                if grown <= 0.01 {
                    continue;
                }
                let phi = (90.0 - angle).to_radians();
                let bow = if i % 2 == 0 { 0.10 } else { -0.10 };
                let at = |u: f32| c + turned(strand_point(u, bow, reach), phi).scale(r);

                // A polyline along however much of the strand has grown —
                // the splash draws the strand as it lengthens.
                let mut path = Path::new();
                path.move_to(at(0.0));
                let steps = (grown * 18.0).ceil().max(1.0);
                for step in 1..=steps as usize {
                    let u = (step as f32 / steps) * grown;
                    path.line_to(at(u));
                }

                let lit = 0.85 + 0.15 * (time * TAU / 3.4 + i as f32).sin();
                if signal {
                    // The violet signal strands: a faint solid base with a
                    // spark travelling along it.
                    g.stroke(
                        path.clone(),
                        AXON.with_alpha((70.0 * reveal.eye) as u8),
                        0.018 * r,
                    );
                    let mut dashes = StrokeStyle::rounded();
                    dashes.dash = Some(Dash {
                        pattern: vec![0.05 * r, 0.085 * r],
                        offset: if calm > 0.5 {
                            0.0
                        } else {
                            -(time * 0.30 * r * (1.0 + 0.1 * i as f32)) % (0.135 * r)
                        },
                    });
                    g.stroke_styled(
                        path,
                        AXON.with_alpha((205.0 * lit * dim) as u8),
                        0.018 * r,
                        dashes,
                    );
                } else {
                    // The mint strands: solid, fully there.
                    g.stroke(
                        path,
                        MEMBRANE.with_alpha((190.0 * reveal.eye * lit) as u8),
                        0.016 * r,
                    );
                }

                // The tip: a dot riding the growing end, settling at the
                // strand's end with a small pop once grown.
                let tip = reveal.tips[i];
                if tip > 0.01 {
                    let pop = 0.8 + 0.35 * tip;
                    let p = at(grown.min(1.0));
                    g.circle(
                        p,
                        0.022 * r * pop,
                        (if signal { AXON } else { MEMBRANE })
                            .with_alpha((235.0 * reveal.eye) as u8),
                    );
                } else if grown < 1.0 {
                    let p = at(grown);
                    g.circle(p, 0.016 * r, Color::rgb(214, 202, 255).with_alpha(220));
                }
            }
        });
    });

    // ---- the organism itself: eye and strands ----------------------------

    book.transformed(Transform::translate(Offset::new(0.0, bob)), |g| {
        let eye_scale = 0.25 + 0.75 * reveal.eye;
        let eye_alpha = (reveal.eye * 1.6).min(1.0);
        g.transformed(Transform::scale_around(c, eye_scale, eye_scale), |eye| {
            // The dark sclera and its lit rim — the one part that does not
            // take part in the blink.
            eye.circle(c, 0.193 * r, SCLERA.with_alpha((255.0 * eye_alpha) as u8));
            eye.ring(
                c,
                0.193 * r,
                0.010 * r,
                RIM.with_alpha((184.0 * eye_alpha) as u8),
            );
            // The eyeball, blinking: squashed about its own centre.
            let lid = blink(time, 5.1);
            eye.transformed(Transform::scale_around(c, 1.0, lid), |ball| {
                ball.circle(
                    c,
                    0.143 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(255, 255, 255).with_alpha((255.0 * eye_alpha) as u8),
                        ),
                        (
                            0.62,
                            Color::rgb(220, 232, 255).with_alpha((245.0 * eye_alpha) as u8),
                        ),
                        (
                            1.0,
                            Color::rgb(139, 164, 210).with_alpha((235.0 * eye_alpha) as u8),
                        ),
                    ]),
                );
                // The iris: violet into teal, deep water at the rim.
                ball.circle(
                    c,
                    0.081 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(228, 215, 255).with_alpha((255.0 * eye_alpha) as u8),
                        ),
                        (0.35, AXON.with_alpha((250.0 * eye_alpha) as u8)),
                        (0.72, IRIS_RIM.with_alpha((240.0 * eye_alpha) as u8)),
                        (1.0, IRIS_DEEP.with_alpha((230.0 * eye_alpha) as u8)),
                    ]),
                );
                ball.circle(c, 0.045 * r, PUPIL.with_alpha((255.0 * eye_alpha) as u8));
                // The catchlights.
                ball.circle(
                    c + Offset::new(-0.034 * r, -0.043 * r),
                    0.026 * r,
                    Color::rgba(255, 255, 255, (244.0 * eye_alpha) as u8),
                );
                ball.circle(
                    c + Offset::new(0.046 * r, 0.046 * r),
                    0.012 * r,
                    Color::rgb(217, 255, 248).with_alpha((200.0 * eye_alpha) as u8),
                );
            });
        });

        // The flare: a ring racing out when a choice is recorded.
        if burst > 0.02 {
            g.ring(
                c,
                0.22 * r + burst * 0.34 * r,
                0.010 * r,
                SYNAPSE.with_alpha(((1.0 - burst) * 190.0) as u8),
            );
        }
    });
}

/// The sigil: the companion's signature, small — the mark that sits in the
/// top bar and on the ask card.
///
/// The same six positions in the same two colours as [`companion`] — three
/// violet, three mint, mirrored about the vertical — drawn to proportions
/// that survive at 30 px: a heavier stroke, a larger eye, no rings and no
/// atmosphere. It blinks on the same clock as its big sibling, so the two
/// read as one organism at two scales.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn sigil(size: f32, time: f32, burst: f32) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;

            let r = s.width * 0.5;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let reach = 0.92 + burst * 0.06;
            let style = StrokeStyle::rounded();

            // Six strands, three and three, grown solid — a dash at this
            // scale is a dotted line, not a signal.
            for (i, &(angle, signal)) in STRANDS.iter().enumerate() {
                let phi = (90.0 - angle).to_radians();
                let bow = if i % 2 == 0 { 0.08 } else { -0.08 };
                let from = c + turned(strand_point(0.0, bow, reach), phi).scale(r);
                let tip = c + turned(strand_point(1.0, bow, reach), phi).scale(r);
                let mut path = Path::new();
                path.move_to(from).line_to(tip);
                book.stroke_styled(
                    path,
                    if signal {
                        AXON.with_alpha((215.0 + burst * 40.0).min(255.0) as u8)
                    } else {
                        MEMBRANE.with_alpha((235.0 + burst * 20.0).min(255.0) as u8)
                    },
                    0.055 * r,
                    style.clone(),
                );
                book.circle(
                    tip,
                    0.075 * r,
                    if signal { AXON } else { MEMBRANE }.with_alpha(235),
                );
            }

            // The eye, blinking with its sibling.
            book.circle(c, 0.30 * r, SCLERA);
            book.ring(c, 0.30 * r, 0.030 * r, RIM.with_alpha(190));
            let lid = blink(time, 5.1);
            book.transformed(Transform::scale_around(c, 1.0, lid), |ball| {
                ball.circle(
                    c,
                    0.225 * r,
                    Gradient::radial_fill().with_stops(&[
                        (0.0, Color::rgb(255, 255, 255)),
                        (0.7, Color::rgb(220, 232, 255)),
                        (1.0, Color::rgb(139, 164, 210)),
                    ]),
                );
                ball.circle(
                    c,
                    0.13 * r,
                    Gradient::radial_fill().with_stops(&[
                        (0.0, Color::rgb(228, 215, 255)),
                        (0.35, AXON),
                        (0.72, IRIS_RIM),
                        (1.0, IRIS_DEEP),
                    ]),
                );
                ball.circle(c, 0.072 * r, PUPIL);
                ball.circle(
                    c + Offset::new(-0.055 * r, -0.068 * r),
                    0.042 * r,
                    Color::rgba(255, 255, 255, 246),
                );
            });
        }),
    )
}

/// A settings gear, drawn rather than stocked: a ring with eight round-capped
/// teeth, rotating by `angle` about its own centre.
///
/// The built-in icon set is ten shapes and deliberately not an icon library;
/// an app wanting a gear supplies its own geometry, which is what this is.
#[must_use]
pub fn gear(size: f32, color: Color, angle: f32) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let r = s.width * 0.26;
            book.ring(c, r, s.width * 0.11, color);
            let style = StrokeStyle::rounded();
            for i in 0..8 {
                let a = angle + i as f32 / 8.0 * std::f32::consts::TAU;
                let (sin, cos) = a.sin_cos();
                let from = Offset::new(
                    c.dx + cos * (r + s.width * 0.03),
                    c.dy + sin * (r + s.width * 0.03),
                );
                let to = Offset::new(
                    c.dx + cos * (r + s.width * 0.17),
                    c.dy + sin * (r + s.width * 0.17),
                );
                let mut path = Path::new();
                path.move_to(from).line_to(to);
                book.stroke_styled(path, color, s.width * 0.085, style.clone());
            }
        }),
    )
}
