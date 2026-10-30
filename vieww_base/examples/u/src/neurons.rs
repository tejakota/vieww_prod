//! The neuron field: the animated, hand-drawn background of every screen.
//!
//! Fourteen somas on a deterministic layout, dendrites bowing between their
//! nearest neighbours, one action potential travelling each edge, and a slow
//! chemical drift filling the space between. Everything is a *function of the
//! clock* — nothing accumulates, nothing steps — so any frame is reproducible
//! from `(time, burst)` alone, and the screenshot tool can photograph a frame
//! without having lived through the ones before it.
//!
//! # One additive layer for all the light
//!
//! Every glow — the pulse halos, the soma halos, the luminous edge strokes —
//! goes into a single `BlendMode::Plus` layer. That is the pricing rule from
//! `Sketchbook::layer`'s own documentation: a blurred or blended layer costs
//! one offscreen buffer *per layer*, and the expensive version of this file is
//! the one that wraps each glow separately. Group early, blend once.

use std::f32::consts::{PI, TAU};

use vieww_foundation::{BlendMode, Color, Gradient, Offset, Path, Size, Sketchbook};
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

/// The persona's own cell, for the question card: a soma that breathes with
/// the clock and flares when a choice lands.
///
/// This is what replaced the 3D neuron — the organ reduced to the one cell
/// that carried it. The viewport was rendering ~350k supersampled pixels a
/// frame through a software rasterizer, which on a phone is the whole frame
/// budget several times over; this is four circles, a ring, and six strokes,
/// and it keeps the identity the organ was standing in for.
#[must_use]
pub fn persona_cell(size: f32, time: f32, burst: f32) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let breathe = (time * 0.8).sin();
            let r = s.width * 0.30 * (1.0 + 0.03 * breathe);

            // Six dendrites, reaching a little further when a choice lands.
            for i in 0..6 {
                let a = i as f32 / 6.0 * TAU - PI / 2.0;
                let (sin, cos) = a.sin_cos();
                let reach = 1.45 + 0.28 * (i as f32 / 5.0) + burst * 0.3;
                let from = Offset::new(c.dx + cos * r * 1.02, c.dy + sin * r * 1.02);
                let to = Offset::new(c.dx + cos * r * reach, c.dy + sin * r * reach);
                let mut path = Path::new();
                path.move_to(from).line_to(to);
                book.stroke_styled(
                    path,
                    AXON.with_alpha((58.0 + burst * 120.0) as u8),
                    s.width * 0.032,
                    StrokeStyle::rounded(),
                );
            }

            // The halo, additive so it glows against the card.
            book.blended_layer(1.0, 0.0, BlendMode::Plus, None, |light| {
                light.circle(
                    c,
                    r * 2.4,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            AXON.with_alpha((52.0 + 18.0 * breathe + burst * 88.0) as u8),
                        ),
                        (0.5, AXON.with_alpha(24)),
                        (1.0, Color::TRANSPARENT),
                    ]),
                );
            });

            // Cytoplasm.
            book.circle(
                c,
                r,
                Gradient::radial_fill().with_stops(&[
                    (0.0, AXON.with_alpha(232)),
                    (0.75, AXON.with_alpha(160)),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
            // Membrane.
            book.ring(c, r, 1.4, SYNAPSE.with_alpha((190.0 + burst * 65.0) as u8));
            // Nucleus — the warm centre.
            book.circle(c, r * 0.32, SOMA.with_alpha((235.0 + burst * 20.0) as u8));
            // The flare: a ring racing out when a choice is recorded.
            if burst > 0.02 {
                book.ring(
                    c,
                    r + 3.0 + burst * s.width * 0.18,
                    1.5,
                    SYNAPSE.with_alpha(((1.0 - burst) * 150.0) as u8),
                );
            }
        }),
    )
}

/// The app's own mark: one small soma — a violet cell with a warm nucleus and
/// a lit membrane, standing in for the persona wherever a wordmark would.
#[must_use]
pub fn soma_mark(size: f32) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let r = s.width * 0.34;
            // The halo, additive so it glows against the field behind it.
            book.blended_layer(1.0, 0.0, BlendMode::Plus, None, |light| {
                light.circle(
                    c,
                    r * 2.1,
                    Gradient::radial_fill()
                        .with_stops(&[(0.0, AXON.with_alpha(70)), (1.0, Color::TRANSPARENT)]),
                );
            });
            book.circle(
                c,
                r,
                Gradient::radial_fill().with_stops(&[
                    (0.0, AXON.with_alpha(235)),
                    (0.75, AXON.with_alpha(170)),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
            book.ring(c, r, 1.1, SYNAPSE.with_alpha(200));
            book.circle(c, r * 0.3, SOMA);
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

/// The train glyph: a small soma with three radiating sparks — the app's
/// "lightning bolt", in a vocabulary that stays biological.
#[must_use]
pub fn spark(size: f32, color: Color) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height * 0.56);
            book.circle(c, s.width * 0.14, color);
            for i in 0..3 {
                let a = -PI / 2.0 + (i as f32 - 1.0) * 0.75;
                let (sin, cos) = a.sin_cos();
                let from = Offset::new(c.dx + cos * s.width * 0.22, c.dy + sin * s.width * 0.22);
                let to = Offset::new(c.dx + cos * s.width * 0.44, c.dy + sin * s.width * 0.44);
                let mut path = Path::new();
                path.move_to(from).line_to(to);
                book.stroke_styled(path, color, s.width * 0.075, StrokeStyle::rounded());
            }
        }),
    )
}
